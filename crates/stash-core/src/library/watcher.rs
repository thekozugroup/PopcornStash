//! inotify watcher plus periodic rescans.
//!
//! Events only mark paths as "pending". A 2 s tick checks each pending path and acts once
//! its size and modification time have not changed for 10 s (copies over SMB take time):
//! top-level entries of `Inbox/` and `Downloads/Completed/` are queued for sorting, files in
//! the library and the owner's folders are indexed in place, and paths that disappeared are
//! marked missing. A Scan job is queued every 30 minutes because inotify misses changes on
//! some network shares, and if the watcher cannot start at all the rescans carry on alone.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use notify::event::{AccessKind, AccessMode};
use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::{Library, scanner};
use crate::jobs::JobKind;
use crate::paths::{self, StashLayout};

/// Timing for the watcher loop.
#[derive(Debug, Clone, Copy)]
pub struct WatchOptions {
    /// How often pending paths are checked.
    pub tick: Duration,
    /// How long a path must stay unchanged before it is acted on.
    pub stable_for: Duration,
    /// How often a full rescan is queued.
    pub rescan_every: Duration,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            tick: Duration::from_secs(2),
            stable_for: Duration::from_secs(10),
            rescan_every: Duration::from_secs(30 * 60),
        }
    }
}

pub fn spawn(lib: Arc<Library>, cancel: CancellationToken) -> JoinHandle<()> {
    spawn_with(lib, cancel, WatchOptions::default())
}

/// [`spawn`] with custom timing (tests use short windows).
pub fn spawn_with(
    lib: Arc<Library>,
    cancel: CancellationToken,
    options: WatchOptions,
) -> JoinHandle<()> {
    tokio::spawn(async move { run(lib, cancel, options).await })
}

/// Size, modification time (ns) and entry count of a file or folder tree.
type Signature = (u64, i128, u64);

/// Events buffered between the notify thread and the watcher loop. When the loop falls
/// behind (a huge copy), further events are dropped and a full rescan is queued instead.
const EVENT_BUFFER: usize = 4096;
/// Paths waiting to become stable. Beyond this the watcher stops tracking single paths and
/// lets a full rescan pick everything up.
const MAX_PENDING: usize = 20_000;
/// A changed path that stays unreadable this long is dropped (the rescans still cover it).
const GIVE_UP_AFTER: Duration = Duration::from_secs(600);

/// The notify callback's end of the channel.
struct EventSink {
    tx: mpsc::Sender<notify::Result<notify::Event>>,
    overflowed: Arc<AtomicBool>,
}

impl EventSink {
    fn push(&self, event: notify::Result<notify::Event>) {
        if let Err(mpsc::error::TrySendError::Full(_)) = self.tx.try_send(event) {
            self.overflowed.store(true, Ordering::Relaxed);
        }
    }
}

pub(crate) struct Pending {
    signature: Option<Signature>,
    changed_at: Instant,
}

async fn run(lib: Arc<Library>, cancel: CancellationToken, options: WatchOptions) {
    let (tx, mut rx) = mpsc::channel::<notify::Result<notify::Event>>(EVENT_BUFFER);
    let overflowed = Arc::new(AtomicBool::new(false));
    let sink = EventSink {
        tx,
        overflowed: overflowed.clone(),
    };
    let watcher = start_watcher(lib.layout.root().to_path_buf(), sink).await;
    let watching = watcher.is_some();
    let mut pending: HashMap<String, Pending> = HashMap::new();
    let mut tick = tokio::time::interval(options.tick);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut rescan =
        tokio::time::interval_at(Instant::now() + options.rescan_every, options.rescan_every);
    rescan.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            Some(event) = rx.recv(), if watching => handle_event(&lib, &mut pending, event).await,
            _ = tick.tick() => {
                if overflowed.swap(false, Ordering::Relaxed) {
                    tracing::info!("too many file changes at once; a full rescan will pick them up");
                    queue_scan(&lib).await;
                }
                process_pending(&lib, &mut pending, options.stable_for).await;
            }
            _ = rescan.tick() => queue_scan(&lib).await,
        }
    }
    drop(watcher);
    tracing::debug!("library watcher stopped");
}

async fn start_watcher(root: PathBuf, sink: EventSink) -> Option<notify::RecommendedWatcher> {
    let started =
        tokio::task::spawn_blocking(move || -> notify::Result<notify::RecommendedWatcher> {
            let mut watcher = notify::recommended_watcher(move |event| sink.push(event))?;
            watcher.watch(&root, RecursiveMode::Recursive)?;
            Ok(watcher)
        })
        .await;
    match started {
        Ok(Ok(watcher)) => Some(watcher),
        Ok(Err(e)) => {
            tracing::warn!(
                error = %e,
                "Could not watch the stash folder for changes (this happens on some network shares or when the \
                 inotify watch limit is reached). New files will still be found by the rescan every 30 minutes."
            );
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "The stash folder watcher failed to start. Relying on periodic rescans.");
            None
        }
    }
}

async fn queue_scan(lib: &Library) {
    if let Err(e) = lib
        .jobs
        .enqueue(JobKind::Scan, None, serde_json::json!({}))
        .await
    {
        tracing::warn!(error = %e, "could not queue a library rescan");
    }
}

pub(crate) async fn handle_event(
    lib: &Library,
    pending: &mut HashMap<String, Pending>,
    event: notify::Result<notify::Event>,
) {
    let event = match event {
        Ok(event) => event,
        Err(e) => {
            tracing::warn!(error = %e, "file watcher error; queueing a rescan");
            queue_scan(lib).await;
            return;
        }
    };
    if event.need_rescan() {
        queue_scan(lib).await;
    }
    // Reads (our own probes and thumbnails included) are not changes.
    if let EventKind::Access(kind) = event.kind
        && !matches!(kind, AccessKind::Close(AccessMode::Write))
    {
        return;
    }
    for path in &event.paths {
        if let Some(rel) = lib.layout.rel(path) {
            note(pending, &rel);
        }
    }
    if pending.len() > MAX_PENDING {
        tracing::info!(
            paths = pending.len(),
            "too many changed paths to follow one by one; queueing a full rescan"
        );
        pending.clear();
        queue_scan(lib).await;
    }
}

/// Remember that `rel` changed. In the sorter's zones the top-level entry is what gets sorted.
fn note(pending: &mut HashMap<String, Pending>, rel: &str) {
    let zone = StashLayout::zone(rel);
    if !zone.indexed() || rel.split('/').any(scanner::is_junk_name) {
        return;
    }
    let now = Instant::now();
    let mut touch = |key: &str| {
        pending
            .entry(key.to_string())
            .and_modify(|p| p.changed_at = now)
            .or_insert(Pending {
                signature: None,
                changed_at: now,
            });
    };
    touch(rel);
    if zone.auto_sorts()
        && let Some(top) = top_level_entry(rel)
        && top != rel
    {
        touch(&top);
    }
}

/// `Inbox/a/b.mkv` → `Inbox/a`; `Downloads/Completed/x/y` → `Downloads/Completed/x`.
/// None for the zone folders themselves and for anything outside the two sorted zones
/// (another client's files in `Downloads/` are not sorted).
pub(crate) fn top_level_entry(rel: &str) -> Option<String> {
    let base = [paths::COMPLETED, paths::INBOX]
        .into_iter()
        .find(|base| rel.starts_with(&format!("{base}/")))?;
    let rest = &rel[base.len() + 1..];
    let first = rest.split('/').next().filter(|s| !s.is_empty())?;
    Some(format!("{base}/{first}"))
}

/// `Ok(None)` when the path is gone; `Err` when it could not be read (a share hiccup), in
/// which case nothing is concluded.
async fn signature(abs: &Path) -> std::io::Result<Option<Signature>> {
    let abs = abs.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let meta = match std::fs::symlink_metadata(&abs) {
            Ok(meta) => meta,
            Err(e) if scanner::is_not_found(&e) => return Ok(None),
            Err(e) => return Err(e),
        };
        let mtime = |m: &std::fs::Metadata| {
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as i128)
        };
        if !meta.is_dir() {
            return Ok(Some((meta.len(), mtime(&meta), 1)));
        }
        let (mut size, mut newest, mut count) = (0u64, mtime(&meta), 0u64);
        for entry in walkdir::WalkDir::new(&abs)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if let Ok(m) = entry.metadata() {
                size = size.saturating_add(if m.is_file() { m.len() } else { 0 });
                newest = newest.max(mtime(&m));
                count += 1;
            }
        }
        Ok(Some((size, newest, count)))
    })
    .await
    .unwrap_or_else(|e| Err(std::io::Error::other(e)))
}

pub(crate) async fn process_pending(
    lib: &Library,
    pending: &mut HashMap<String, Pending>,
    stable_for: Duration,
) {
    if pending.is_empty() {
        return;
    }
    let now = Instant::now();
    let mut keys: Vec<String> = pending.keys().cloned().collect();
    keys.sort();
    for rel in keys {
        let Ok(abs) = lib.layout.abs(&rel) else {
            pending.remove(&rel);
            continue;
        };
        let sig = match signature(&abs).await {
            Ok(Some(sig)) => sig,
            Ok(None) => {
                // Gone: whatever was indexed there is missing now (a rename shows up again as a new path).
                pending.remove(&rel);
                if let Err(e) = scanner::mark_missing(lib, &rel).await {
                    tracing::warn!(error = %e, path = %rel, "could not mark an item missing");
                }
                continue;
            }
            Err(e) => {
                // Unreadable for now (a network share hiccup): try again on the next tick.
                tracing::debug!(error = %e, path = %rel, "could not read a changed path");
                if pending
                    .get(&rel)
                    .is_some_and(|p| now.duration_since(p.changed_at) > GIVE_UP_AFTER)
                {
                    pending.remove(&rel);
                }
                continue;
            }
        };
        let Some(entry) = pending.get_mut(&rel) else {
            continue;
        };
        if entry.signature != Some(sig) {
            entry.signature = Some(sig);
            entry.changed_at = now;
            continue;
        }
        if now.duration_since(entry.changed_at) < stable_for {
            continue;
        }
        pending.remove(&rel);
        act_on_stable(lib, &rel, &abs).await;
    }
}

async fn act_on_stable(lib: &Library, rel: &str, abs: &Path) {
    let zone = StashLayout::zone(rel);
    if zone.auto_sorts() {
        if top_level_entry(rel).as_deref() != Some(rel) {
            return;
        }
        if let Err(e) = lib.enqueue_sort(rel, None).await {
            tracing::warn!(error = %e, path = %rel, "could not queue a sort");
        }
        return;
    }
    let files = if abs.is_dir() {
        let abs = abs.to_path_buf();
        tokio::task::spawn_blocking(move || scanner::files_under(&abs))
            .await
            .unwrap_or_default()
    } else {
        vec![abs.to_path_buf()]
    };
    for file in files {
        let Some(file_rel) = lib.layout.rel(&file) else {
            continue;
        };
        if let Err(e) = lib.index_path(&file_rel).await {
            tracing::warn!(error = %e, path = %file_rel, "could not index a changed file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_entries() {
        assert_eq!(top_level_entry("Inbox/a/b.mkv").as_deref(), Some("Inbox/a"));
        assert_eq!(
            top_level_entry("Inbox/a.mkv").as_deref(),
            Some("Inbox/a.mkv")
        );
        assert_eq!(
            top_level_entry("Downloads/Completed/x/y/z").as_deref(),
            Some("Downloads/Completed/x")
        );
        // Only Inbox/ and Downloads/Completed/ have entries to sort.
        assert_eq!(top_level_entry("Downloads/loose.mkv"), None);
        assert_eq!(top_level_entry("Downloads/Other Client/a.mkv"), None);
        assert_eq!(top_level_entry("Downloads/In Progress/x"), None);
        assert_eq!(top_level_entry("Downloads/Completed"), None);
        assert_eq!(top_level_entry("Inbox"), None);
        assert_eq!(top_level_entry("Movies/x"), None);
    }

    #[test]
    fn a_full_event_buffer_asks_for_a_rescan() {
        let (tx, mut rx) = mpsc::channel(1);
        let overflowed = Arc::new(AtomicBool::new(false));
        let sink = EventSink {
            tx,
            overflowed: overflowed.clone(),
        };
        sink.push(Ok(notify::Event::new(EventKind::Any)));
        assert!(!overflowed.load(Ordering::Relaxed));
        sink.push(Ok(notify::Event::new(EventKind::Any)));
        assert!(
            overflowed.load(Ordering::Relaxed),
            "the second event did not fit"
        );
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn signatures_tell_gone_from_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file"), b"12345").unwrap();
        let sig = signature(&dir.path().join("file")).await.unwrap().unwrap();
        assert_eq!((sig.0, sig.2), (5, 1));
        assert_eq!(signature(&dir.path().join("nope")).await.unwrap(), None);
        // A parent that is a file: the path cannot exist.
        assert_eq!(
            signature(&dir.path().join("file/child")).await.unwrap(),
            None
        );
        // A symlink loop cannot be read, which is not the same as gone.
        std::os::unix::fs::symlink("loop", dir.path().join("loop")).unwrap();
        assert!(signature(&dir.path().join("loop/child")).await.is_err());
    }

    #[test]
    fn notes_paths() {
        let mut pending = HashMap::new();
        note(&mut pending, "Inbox/Folder/a.mkv");
        note(&mut pending, "Downloads/In Progress/t/a.mkv");
        note(&mut pending, ".popcornstash/Trash/x");
        note(&mut pending, "Movies/.DS_Store");
        note(&mut pending, "Movies/Heat (1995)/Heat (1995).mkv");
        note(&mut pending, "Downloads/loose.mkv");
        note(&mut pending, "Downloads/Other Client/a.mkv");
        note(&mut pending, "Downloads");
        let mut keys: Vec<_> = pending.keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "Inbox/Folder",
                "Inbox/Folder/a.mkv",
                "Movies/Heat (1995)/Heat (1995).mkv"
            ]
        );
    }
}
