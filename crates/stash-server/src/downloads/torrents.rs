//! Torrent poller: mirrors engine state into `downloads`, handles completion and
//! the seeding policy (docs/architecture.md section 5, Torrents).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use stash_core::internal_api::{
    AddTorrentRequest, AddTorrentResponse, EngineRunState, EngineTorrent, EngineTorrentState,
    LimitsRequest, MSG_CHECKING_FILES, MSG_WAITING_FOR_ENGINE, TorrentFiles,
};
use stash_core::models::{Download, DownloadKind, DownloadRow, DownloadState};
use stash_core::paths::{self, StashLayout, Zone};
use stash_core::{new_id, now};
use tokio_util::sync::CancellationToken;

use super::manager::{
    DownloadManager, MSG_WAITING_FOR_VPN, NewDownload, active_engine_id, engine_error,
    engine_wait_message, slot_message,
};
use crate::error::{ApiError, ApiResult};
use crate::torrentd_client::TorrentdError;

#[path = "torrent_manifest.rs"]
pub(super) mod torrent_manifest;
use stash_core::internal_api::TorrentCompletion;
use torrent_manifest::Frozen;

pub const MSG_LOST: &str = "The torrent engine lost track of this download.";
const MSG_ENGINE_REFUSED: &str = "The torrent engine could not add this torrent.";
const MSG_RESTORE: &str = "This torrent could not be restored after a restart. Add it again.";
const MSG_FILES_MISSING: &str = "The finished torrent's files are not in Downloads/In Progress.";
pub const MSG_MOVE_RETRY: &str = "The finished torrent could not be moved to Downloads/Completed yet. PopcornStash keeps trying.";
const MSG_ENGINE_ERROR: &str = "The torrent engine reported a problem with this torrent.";

// --- .torrent validation ------------------------------------------------------------

enum Bencode<'a> {
    Int,
    Bytes(&'a [u8]),
    List,
    Dict(Vec<(&'a [u8], Bencode<'a>)>),
}

const MAX_DEPTH: usize = 64;

fn parse_value(data: &[u8], pos: usize, depth: usize) -> Result<(Bencode<'_>, usize), ()> {
    if depth > MAX_DEPTH {
        return Err(());
    }
    match data.get(pos).copied().ok_or(())? {
        b'i' => {
            let end = pos + 1 + data[pos + 1..].iter().position(|&b| b == b'e').ok_or(())?;
            let digits = std::str::from_utf8(&data[pos + 1..end]).map_err(|_| ())?;
            digits.parse::<i64>().map_err(|_| ())?;
            Ok((Bencode::Int, end + 1))
        }
        b'l' => {
            let mut p = pos + 1;
            while data.get(p).copied().ok_or(())? != b'e' {
                let (_, next) = parse_value(data, p, depth + 1)?;
                p = next;
            }
            Ok((Bencode::List, p + 1))
        }
        b'd' => {
            let mut p = pos + 1;
            let mut entries = Vec::new();
            while data.get(p).copied().ok_or(())? != b'e' {
                let (key, next) = parse_value(data, p, depth + 1)?;
                let Bencode::Bytes(key) = key else {
                    return Err(());
                };
                let (value, next) = parse_value(data, next, depth + 1)?;
                entries.push((key, value));
                p = next;
            }
            Ok((Bencode::Dict(entries), p + 1))
        }
        b'0'..=b'9' => {
            let colon = pos + data[pos..].iter().position(|&b| b == b':').ok_or(())?;
            let len: usize = std::str::from_utf8(&data[pos..colon])
                .map_err(|_| ())?
                .parse()
                .map_err(|_| ())?;
            let start = colon + 1;
            let end = start.checked_add(len).ok_or(())?;
            let bytes = data.get(start..end).ok_or(())?;
            Ok((Bencode::Bytes(bytes), end))
        }
        _ => Err(()),
    }
}

/// The bytes are not a usable `.torrent` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTorrent;

/// Validate a `.torrent` file (a bencoded dictionary with an `info` dictionary) and
/// return `info.name` when present.
pub fn torrent_name(data: &[u8]) -> Result<Option<String>, InvalidTorrent> {
    if data.first() != Some(&b'd') {
        return Err(InvalidTorrent);
    }
    let (value, end) = parse_value(data, 0, 0).map_err(|()| InvalidTorrent)?;
    if end != data.len() {
        return Err(InvalidTorrent);
    }
    let Bencode::Dict(entries) = value else {
        return Err(InvalidTorrent);
    };
    let info = entries
        .iter()
        .find(|(k, _)| *k == b"info")
        .map(|(_, v)| v)
        .ok_or(InvalidTorrent)?;
    let Bencode::Dict(info) = info else {
        return Err(InvalidTorrent);
    };
    let name = info
        .iter()
        .find(|(k, _)| *k == b"name")
        .and_then(|(_, v)| match v {
            Bencode::Bytes(b) => Some(String::from_utf8_lossy(b).trim().to_string()),
            _ => None,
        });
    Ok(name.filter(|n| !n.is_empty()))
}

// --- adding ---------------------------------------------------------------------------

/// Marks a row as being added right now, so the torrent poller does not add it too and
/// the web workers do not start it before its first event is out. Removed on drop.
pub(crate) struct InFlight<'a> {
    set: &'a dashmap::DashSet<String>,
    id: String,
}

impl<'a> InFlight<'a> {
    pub(crate) fn new(set: &'a dashmap::DashSet<String>, id: &str) -> Self {
        set.insert(id.to_string());
        Self {
            set,
            id: id.to_string(),
        }
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.set.remove(&self.id);
    }
}

impl DownloadManager {
    /// Create the row and hand the torrent to the engine. When the VPN or the engine is
    /// down, or every slot of "Torrents at once" is taken, the row stays `queued` and the
    /// poller adds it later.
    pub(crate) async fn add_torrent(
        &self,
        user_id: &str,
        request: AddTorrentRequest,
        name: String,
        source: String,
        library_id: Option<&str>,
    ) -> ApiResult<Download> {
        // Held from before the row exists until the add is over (removed on drop, even
        // when the request is abandoned mid-way), so the poller never takes a row that
        // is being added for one waiting for the VPN.
        let id = new_id();
        let _in_flight = InFlight::new(&self.adding, &id);
        self.insert_row_as(
            &id,
            NewDownload {
                library_id,
                kind: DownloadKind::Torrent,
                name: &name,
                source: &source,
                state: DownloadState::Queued,
                created_by: user_id,
                rel_path: None,
                total_bytes: None,
                done_bytes: 0,
                error: None,
            },
        )
        .await?;
        self.add_new_torrent(&id, &request).await
    }

    async fn add_new_torrent(&self, id: &str, request: &AddTorrentRequest) -> ApiResult<Download> {
        let id = id.to_string();
        if let Err(e) = self.save_pending(&id, request).await {
            tracing::error!(error = %e, download = %id, "could not save the torrent request");
            let _ = sqlx::query("DELETE FROM downloads WHERE id = ?")
                .bind(&id)
                .execute(&self.db)
                .await;
            return Err(ApiError::internal());
        }
        // Deciding and starting happen under one lock, so two adds never take one slot.
        let _slots = self.torrent_slots.lock().await;
        if let Err(waiting) = self.torrent_slot(&id).await? {
            sqlx::query("UPDATE downloads SET error = ?, updated_at = ? WHERE id = ?")
                .bind(&waiting)
                .bind(now())
                .bind(&id)
                .execute(&self.db)
                .await?;
            tracing::info!(download = %id, "torrent waits for a free slot");
            self.torrent_wake.notify_one();
            return self.publish_id(&id).await;
        }
        match self.torrentd.add(request).await {
            Ok(added) => {
                let id = self.attach_engine(&id, &added).await?;
                tracing::info!(download = %id, "torrent added");
                self.publish_id(&id).await
            }
            Err(e @ (TorrentdError::Blocked(_) | TorrentdError::Unreachable(_))) => {
                let waiting = match e {
                    TorrentdError::Blocked(_) => self.engine_wait_reason().await,
                    _ => MSG_WAITING_FOR_VPN,
                };
                sqlx::query("UPDATE downloads SET error = ?, updated_at = ? WHERE id = ?")
                    .bind(waiting)
                    .bind(now())
                    .bind(&id)
                    .execute(&self.db)
                    .await?;
                tracing::info!(download = %id, "torrent queued until the VPN connects");
                self.torrent_wake.notify_one();
                self.publish_id(&id).await
            }
            Err(e) => {
                self.forget_torrent_row(&id).await;
                match e {
                    TorrentdError::Api { status, message } => {
                        tracing::warn!(status, message = %message, "the torrent engine refused a torrent");
                        Err(ApiError::bad_request(MSG_ENGINE_REFUSED))
                    }
                    other => Err(other.api_error()),
                }
            }
        }
    }

    /// Fail a torrent that is still waiting to reach the engine (`queued`, no engine id).
    /// The poller works from rows it read a moment earlier, and an add that finished in
    /// between (attached to the engine, its pending request gone) must not be failed.
    async fn fail_waiting(&self, id: &str, message: &str) {
        let failed = sqlx::query(
            "UPDATE downloads SET state = 'failed', error = ?, down_bps = 0, up_bps = 0, eta_s = NULL, updated_at = ?
             WHERE id = ? AND state = 'queued' AND engine_id IS NULL",
        )
        .bind(message)
        .bind(now())
        .bind(id)
        .execute(&self.db)
        .await;
        match failed {
            Ok(done) if done.rows_affected() > 0 => {
                let _ = self.publish_id(id).await;
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(error = %e, download = %id, "could not record a failed download")
            }
        }
    }

    async fn forget_torrent_row(&self, id: &str) {
        let _ = sqlx::query("DELETE FROM downloads WHERE id = ?")
            .bind(id)
            .execute(&self.db)
            .await;
        let _ = tokio::fs::remove_file(self.pending_path(id)).await;
    }

    async fn save_pending(&self, id: &str, request: &AddTorrentRequest) -> std::io::Result<()> {
        let path = self.pending_path(id);
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        tokio::fs::create_dir_all(&dir).await?;
        // Magnet tracker URLs and .torrent announce URLs can carry private passkeys.
        crate::torrentd_client::write_private_file(&path, &serde_json::to_vec(request)?, 0o600)
            .await
    }

    /// Record the engine's id. When the engine already had this torrent under another
    /// active row, the new row is dropped and the existing id returned.
    async fn attach_engine(&self, id: &str, added: &AddTorrentResponse) -> ApiResult<String> {
        let existing: Option<DownloadRow> = sqlx::query_as(
            "SELECT * FROM downloads WHERE kind = 'torrent' AND engine_id = ? AND id != ?
             AND (state IN ('queued', 'downloading', 'paused', 'seeding') OR (state = 'completed' AND rel_path LIKE 'Downloads/In Progress/%')) LIMIT 1",
        )
        .bind(&added.id)
        .bind(id)
        .fetch_optional(&self.db)
        .await?;
        if let Some(existing) = existing {
            let same = if existing.state == DownloadState::Completed {
                // Release may have succeeded before the old row recorded it.
                // A re-add in that window has a new output folder or pending metadata.
                let live = self
                    .torrentd
                    .list()
                    .await
                    .map_err(|e| engine_error(&e))?
                    .into_iter()
                    .find(|t| t.id == added.id)
                    .ok_or_else(|| ApiError::conflict(MSG_LOST))?;
                live.output_dir
                    .as_deref()
                    .and_then(|p| self.engine_path_to_local(p))
                    .and_then(|p| self.layout.rel(&p))
                    == existing.rel_path
            } else {
                true
            };
            if same {
                self.forget_torrent_row(id).await;
                return Ok(existing.id);
            }
            sqlx::query("UPDATE downloads SET engine_id = NULL WHERE id = ? AND state = 'completed' AND engine_id = ?")
                .bind(&existing.id).bind(&added.id).execute(&self.db).await?;
        }
        let name = added
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty());
        let updated = sqlx::query(
            "UPDATE downloads SET engine_id = ?, info_hash = ?, name = COALESCE(?, name), state = 'downloading',
                    error = NULL, updated_at = ?
             WHERE id = ?",
        )
        .bind(&added.id)
        .bind(&added.info_hash)
        .bind(name)
        .bind(now())
        .bind(id)
        .execute(&self.db)
        .await?
        .rows_affected();
        let _ = tokio::fs::remove_file(self.pending_path(id)).await;
        if updated == 0 {
            // Removed while the engine was adding it: do not leave it running there.
            if let Err(e) = self.torrentd.remove(&added.id, true).await {
                tracing::warn!(error = %e, "could not drop a torrent whose download was removed");
            }
            return Err(ApiError::not_found("That download no longer exists."));
        }
        self.torrent_wake.notify_one();
        Ok(id.to_string())
    }

    /// Engine path (`<STASH_ROOT>/Downloads/...`, or `/stash/Downloads/...` from a
    /// separate container) to a local path under the stash.
    fn engine_path_to_local(&self, engine_path: &str) -> Option<PathBuf> {
        let path = Path::new(engine_path);
        let rest = path
            .strip_prefix(self.layout.root())
            .or_else(|_| path.strip_prefix("/stash"))
            .ok()?;
        self.layout.abs(rest).ok()
    }

    /// Where a torrent's file or folder lives, strictly inside `In Progress`.
    fn torrent_content(&self, torrent: &EngineTorrent, name: &str) -> Option<PathBuf> {
        let in_progress = self.layout.in_progress();
        let single = |name: &str| {
            let ok = !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\']);
            ok.then(|| in_progress.join(name))
        };
        let candidate = match torrent
            .output_dir
            .as_deref()
            .and_then(|d| self.engine_path_to_local(d))
        {
            Some(base) if base == in_progress => single(name)?,
            Some(base) => base,
            None => single(name)?,
        };
        let rel = self.layout.rel(&candidate)?;
        is_torrent_content_rel(&rel).then_some(candidate)
    }
}

// --- poller ---------------------------------------------------------------------------

struct Poller {
    /// None until the first answer.
    reachable: Option<bool>,
    backoff: Duration,
    /// First time each row's engine id was missing from the engine list.
    missing: HashMap<String, Instant>,
    /// Finished torrents whose move failed, and when to try again.
    retry_at: HashMap<String, Instant>,
    /// Limits last accepted by the engine; cleared when it becomes unreachable.
    pushed_limits: Option<LimitsRequest>,
}

/// Spawn the loop that mirrors the engine into `downloads`.
pub(crate) fn spawn_poller(manager: Arc<DownloadManager>, cancel: CancellationToken) {
    tokio::spawn(async move {
        let mut poller = Poller {
            reachable: None,
            backoff: Duration::ZERO,
            missing: HashMap::new(),
            retry_at: HashMap::new(),
            pushed_limits: None,
        };
        loop {
            if cancel.is_cancelled() {
                break;
            }
            let wait = poll_once(&manager, &mut poller).await;
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = manager.torrent_wake.notified() => {}
                _ = tokio::time::sleep(wait) => {}
            }
        }
        tracing::debug!("torrent poller stopped");
    });
}

async fn active_torrent_rows(manager: &DownloadManager) -> ApiResult<Vec<DownloadRow>> {
    Ok(sqlx::query_as::<_, DownloadRow>(
        "SELECT * FROM downloads WHERE kind = 'torrent' AND state IN ('queued', 'downloading', 'paused', 'seeding')
         ORDER BY created_at",
    )
    .fetch_all(&manager.db)
    .await?)
}

/// Torrents already judged finished whose files have not left `In Progress` (a restart
/// or a failed move interrupted [`finish_completed`]).
async fn finishing_torrent_rows(manager: &DownloadManager) -> ApiResult<Vec<DownloadRow>> {
    let rows = sqlx::query_as::<_, DownloadRow>(
        "SELECT * FROM downloads WHERE kind = 'torrent' AND state = 'completed' AND rel_path IS NOT NULL
         ORDER BY created_at",
    )
    .fetch_all(&manager.db)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            row.rel_path
                .as_deref()
                .is_some_and(|rel| StashLayout::zone(rel) == Zone::InProgress)
        })
        .collect())
}

/// Carry on with finished torrents that are still in `In Progress`, each at most once
/// per `finish_retry` after a failure.
async fn finish_pending(manager: &DownloadManager, poller: &mut Poller) {
    let rows = match finishing_torrent_rows(manager).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "could not read finished torrents");
            return;
        }
    };
    poller
        .retry_at
        .retain(|id, _| rows.iter().any(|row| &row.id == id));
    let now = Instant::now();
    for row in rows {
        if poller.retry_at.get(&row.id).is_some_and(|at| now < *at) {
            continue;
        }
        if finish_completed(manager, &row).await {
            poller.retry_at.remove(&row.id);
        } else {
            poller
                .retry_at
                .insert(row.id.clone(), now + manager.tuning().finish_retry);
        }
    }
}

async fn poll_once(manager: &Arc<DownloadManager>, poller: &mut Poller) -> Duration {
    let tuning = manager.tuning();
    // Selection can change without changing the download state. Serialize the engine
    // snapshot, DB updates and handoff with controls so an old poll cannot finish a new selection.
    let controls = manager.torrent_slots.lock().await;
    let list = match manager.torrentd.list().await {
        Ok(list) => list,
        Err(TorrentdError::Disabled) => return tuning.torrent_slow * 6,
        Err(e) => {
            if poller.reachable != Some(false) {
                tracing::warn!(error = %e, "the torrent engine is not reachable");
            }
            poller.reachable = Some(false);
            poller.pushed_limits = None;
            // Nothing can be judged missing while the engine is away.
            poller.missing.clear();
            poller.backoff =
                (poller.backoff * 2).clamp(tuning.torrent_fast, tuning.engine_backoff_max);
            return poller.backoff;
        }
    };
    if poller.reachable != Some(true) {
        tracing::info!("the torrent engine is reachable");
    }
    poller.reachable = Some(true);
    poller.backoff = Duration::ZERO;
    let rows = match active_torrent_rows(manager).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "could not read torrents");
            return tuning.torrent_slow;
        }
    };
    push_limits(manager, poller).await;
    finish_pending(manager, poller).await;
    // Forget rows that are no longer active (removed, finished or failed).
    poller
        .missing
        .retain(|id, _| rows.iter().any(|row| &row.id == id));

    let by_id: HashMap<&str, &EngineTorrent> = list.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut busy = false;
    for row in rows {
        // Not in the engine yet: [`fill_slots`] adds it when it may start.
        let Some(engine_id) = row.engine_id.clone() else {
            continue;
        };
        match by_id.get(engine_id.as_str()) {
            Some(torrent) => {
                poller.missing.remove(&row.id);
                busy |= matches!(
                    torrent.state,
                    EngineTorrentState::Initializing
                        | EngineTorrentState::Downloading
                        | EngineTorrentState::Seeding
                );
                if let Err(e) = apply(manager, &row, torrent).await {
                    tracing::warn!(error = %e, download = %row.id, "could not update a torrent");
                }
            }
            None => {
                let since = *poller
                    .missing
                    .entry(row.id.clone())
                    .or_insert_with(Instant::now);
                if since.elapsed() >= tuning.torrent_missing {
                    tracing::warn!(download = %row.id, "torrent missing from the engine");
                    poller.missing.remove(&row.id);
                    manager.fail(&row.id, MSG_LOST).await;
                } else {
                    busy = true;
                }
            }
        }
    }
    drop(controls);
    busy |= fill_slots(manager).await;
    if busy {
        tuning.torrent_fast
    } else {
        tuning.torrent_slow
    }
}

/// Push the owner's speed limits whenever the engine (re)appears or they change.
async fn push_limits(manager: &DownloadManager, poller: &mut Poller) {
    let settings = manager.settings.get().await.torrent;
    let limits = LimitsRequest {
        down_kbps: settings.down_limit_kbps,
        up_kbps: settings.up_limit_kbps,
        seed: settings.seed_after_complete,
        add_public_trackers: settings.add_public_trackers,
    };
    if poller.pushed_limits == Some(limits) {
        return;
    }
    match manager.torrentd.set_limits(limits).await {
        Ok(()) => poller.pushed_limits = Some(limits),
        Err(e) => tracing::debug!(error = %e, "could not push torrent speed limits"),
    }
}

/// Torrents that hold a slot (running in the engine) or wait for one, oldest first.
async fn slot_rows(manager: &DownloadManager) -> ApiResult<Vec<DownloadRow>> {
    Ok(sqlx::query_as::<_, DownloadRow>(
        "SELECT * FROM downloads WHERE kind = 'torrent'
           AND ((state = 'downloading' AND engine_id IS NOT NULL) OR state = 'queued')
         ORDER BY created_at, id",
    )
    .fetch_all(&manager.db)
    .await?)
}

/// "Torrents at once" (`torrent.max_active`, read on every poll so a change applies at
/// once). Torrents beyond the limit wait in line as `queued`, oldest first: a lowered
/// limit pauses the newest running torrents in the engine and puts them back in line,
/// and a free slot starts the next one (adding it to the engine if it waited for the
/// VPN). Seeding and paused torrents take no slot. Returns true while any torrent waits.
pub(super) async fn fill_slots(manager: &DownloadManager) -> bool {
    let _slots = manager.torrent_slots.lock().await;
    let max_active = manager.settings.get().await.torrent.max_active.max(1);
    let max = usize::try_from(max_active).unwrap_or(usize::MAX);
    let rows = match slot_rows(manager).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "could not read torrents waiting for a slot");
            return false;
        }
    };
    // A row still being added decides for itself once it holds the lock.
    let (mut running, waiting): (Vec<DownloadRow>, Vec<DownloadRow>) = rows
        .into_iter()
        .filter(|row| !manager.adding.contains(&row.id))
        .partition(|row| row.state == DownloadState::Downloading);
    while running.len() > max {
        let Some(newest) = running.pop() else { break };
        if !requeue(manager, &newest, &slot_message(max_active)).await {
            // Still running; it is tried again on the next poll.
            running.push(newest);
            break;
        }
    }
    if waiting.is_empty() {
        return false;
    }
    let health = manager.torrentd.health().await.ok();
    let engine_runs = health
        .as_ref()
        .is_some_and(|health| health.engine.state == EngineRunState::Running);
    let engine_wait = engine_wait_message(health.as_ref());
    let mut free = max.saturating_sub(running.len());
    for row in &waiting {
        if !engine_runs {
            note_waiting(manager, row, engine_wait).await;
        } else if free == 0 {
            note_waiting(manager, row, &slot_message(max_active)).await;
        } else {
            match start_waiting(manager, row).await {
                Started::Yes => free -= 1,
                Started::No => {}
                Started::EngineDown => break,
            }
        }
    }
    true
}

enum Started {
    Yes,
    No,
    /// The engine stopped taking torrents (VPN down, unreachable): try again later.
    EngineDown,
}

/// Start one torrent that waited in line: resume it in the engine when it was paused
/// there for a slot, else add it from its saved request.
async fn start_waiting(manager: &DownloadManager, row: &DownloadRow) -> Started {
    let Some(engine_id) = row.engine_id.as_deref() else {
        return add_waiting(manager, row).await;
    };
    match manager.torrentd.resume(engine_id).await {
        Ok(()) => {
            let started = sqlx::query(
                "UPDATE downloads SET state = 'downloading', error = NULL, updated_at = ?
                 WHERE id = ? AND state = 'queued'",
            )
            .bind(now())
            .bind(&row.id)
            .execute(&manager.db)
            .await;
            if started.is_ok_and(|r| r.rows_affected() > 0) {
                tracing::info!(download = %row.id, "torrent took a free slot");
                let _ = manager.publish_id(&row.id).await;
                Started::Yes
            } else {
                Started::No
            }
        }
        Err(
            TorrentdError::Blocked(_) | TorrentdError::Unreachable(_) | TorrentdError::Disabled,
        ) => Started::EngineDown,
        // Gone from the engine: the poll judges it missing and fails it.
        Err(e) => {
            tracing::warn!(error = %e, download = %row.id, "could not resume a torrent that waited for a slot");
            Started::No
        }
    }
}

/// Add a torrent that has not reached the engine yet (it waited for the VPN or a slot).
async fn add_waiting(manager: &DownloadManager, row: &DownloadRow) -> Started {
    let request: Option<AddTorrentRequest> =
        match tokio::fs::read(manager.pending_path(&row.id)).await {
            Ok(bytes) => serde_json::from_slice(&bytes).ok(),
            Err(_) => None,
        };
    let Some(request) = request else {
        manager.fail_waiting(&row.id, MSG_RESTORE).await;
        return Started::No;
    };
    match manager.torrentd.add(&request).await {
        Ok(added) => match manager.attach_engine(&row.id, &added).await {
            Ok(id) => {
                tracing::info!(download = %id, "queued torrent added to the engine");
                let _ = manager.publish_id(&id).await;
                Started::Yes
            }
            Err(e) => {
                tracing::warn!(error = %e, download = %row.id, "could not record a queued torrent");
                Started::No
            }
        },
        Err(
            TorrentdError::Blocked(_) | TorrentdError::Unreachable(_) | TorrentdError::Disabled,
        ) => Started::EngineDown,
        Err(TorrentdError::Api { status, message }) => {
            tracing::warn!(status, message = %message, download = %row.id, "the engine refused a queued torrent");
            let _ = tokio::fs::remove_file(manager.pending_path(&row.id)).await;
            manager.fail_waiting(&row.id, MSG_ENGINE_REFUSED).await;
            Started::No
        }
    }
}

/// Add torrents that waited, once the engine runs, without looking at slots (tests drive
/// a poll's stale rows through this).
#[cfg(test)]
pub(super) async fn add_pending(manager: &DownloadManager, rows: Vec<DownloadRow>) {
    match manager.torrentd.health().await {
        Ok(health) if health.engine.state == EngineRunState::Running => {}
        _ => return,
    }
    for row in rows {
        if let Started::EngineDown = add_waiting(manager, &row).await {
            return;
        }
    }
}

/// Pause a running torrent in the engine and put it back in line (the limit was lowered).
async fn requeue(manager: &DownloadManager, row: &DownloadRow, message: &str) -> bool {
    let Some(engine_id) = row.engine_id.as_deref() else {
        return false;
    };
    if let Err(e) = manager.torrentd.pause(engine_id).await {
        tracing::warn!(error = %e, download = %row.id, "could not pause a torrent beyond the limit");
        return false;
    }
    let queued = sqlx::query(
        "UPDATE downloads SET state = 'queued', error = ?, down_bps = 0, up_bps = 0, eta_s = NULL, updated_at = ?
         WHERE id = ? AND state = 'downloading'",
    )
    .bind(message)
    .bind(now())
    .bind(&row.id)
    .execute(&manager.db)
    .await;
    match queued {
        Ok(done) if done.rows_affected() > 0 => {
            tracing::info!(download = %row.id, "torrent waits for a free slot again");
            let _ = manager.publish_id(&row.id).await;
            true
        }
        Ok(_) => true,
        Err(e) => {
            tracing::warn!(error = %e, download = %row.id, "could not put a torrent back in line");
            false
        }
    }
}

/// Say why a queued torrent waits, when the reason changed.
async fn note_waiting(manager: &DownloadManager, row: &DownloadRow, message: &str) {
    if row.error.as_deref() == Some(message) {
        return;
    }
    let noted = sqlx::query(
        "UPDATE downloads SET error = ?, updated_at = ? WHERE id = ? AND state = 'queued'",
    )
    .bind(message)
    .bind(now())
    .bind(&row.id)
    .execute(&manager.db)
    .await;
    if noted.is_ok_and(|r| r.rows_affected() > 0) {
        let _ = manager.publish_id(&row.id).await;
    }
}

/// Mirror one engine torrent into its row; finish it when the seeding policy allows.
pub(super) async fn apply(
    manager: &DownloadManager,
    row: &DownloadRow,
    torrent: &EngineTorrent,
) -> ApiResult<()> {
    let t = now();
    let name = torrent
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(&row.name)
        .to_string();
    let total = torrent.total_bytes.filter(|&b| b > 0);
    // Pending magnets have no output folder. A paused integrity check also
    // carries the explicit engine reason so it cannot look like a finished pause.
    let resolved = torrent.output_dir.as_deref().is_some_and(|p| !p.is_empty());
    let checking = resolved
        && (torrent.state == EngineTorrentState::Initializing
            || torrent.error.as_deref() == Some(MSG_CHECKING_FILES));
    let waiting_for_engine = resolved && torrent.error.as_deref() == Some(MSG_WAITING_FOR_ENGINE);
    let provisional = checking || waiting_for_engine;
    let complete = !provisional
        && total.is_some_and(|total| torrent.done_bytes >= total)
        && matches!(
            torrent.state,
            EngineTorrentState::Downloading
                | EngineTorrentState::Paused
                | EngineTorrentState::Seeding
        );

    if torrent.state == EngineTorrentState::Error && !complete {
        let message = torrent
            .error
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(MSG_ENGINE_ERROR);
        tracing::warn!(download = %row.id, reason = %message, "torrent failed in the engine");
        manager.fail(&row.id, MSG_ENGINE_ERROR).await;
        return Ok(());
    }

    let mut completed_at = row.completed_at;
    let state = if complete {
        let since = *completed_at.get_or_insert(t);
        let policy = manager.settings.get().await.torrent;
        let size = total.unwrap_or(torrent.done_bytes).max(1);
        let ratio = torrent.uploaded_bytes as f64 / size as f64;
        let seeded_long_enough = t - since >= i64::from(policy.seed_hours) * 3600;
        if !policy.seed_after_complete || ratio >= policy.seed_ratio || seeded_long_enough {
            return finish(manager, row, torrent, &name).await;
        }
        if torrent.state == EngineTorrentState::Paused {
            DownloadState::Paused
        } else {
            DownloadState::Seeding
        }
    } else if torrent.state == EngineTorrentState::Paused {
        // Paused in the engine while it waits for a free slot: still in line, not
        // paused by hand.
        if row.state == DownloadState::Queued {
            DownloadState::Queued
        } else {
            DownloadState::Paused
        }
    } else {
        DownloadState::Downloading
    };
    let error = if torrent.error.as_deref() == Some("Ready to choose files.") {
        torrent.error.clone()
    } else if state == DownloadState::Queued {
        row.error.clone()
    } else if checking {
        Some(MSG_CHECKING_FILES.into())
    } else if waiting_for_engine {
        torrent.error.clone()
    } else {
        None
    };

    let as_i64 = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
    let (total_bytes, done_bytes, progress) =
        if provisional && row.total_bytes.is_some_and(|b| b > 0) {
            // SQLite survives both a cold engine and a server restart. Preserve the
            // selected-byte denominator too: initialization reports the whole torrent.
            (row.total_bytes, row.done_bytes, row.progress)
        } else {
            let done = if checking { 0 } else { torrent.done_bytes };
            let progress = match total {
                Some(total) => (done as f64 / total as f64).clamp(0.0, 1.0),
                None if checking => 0.0,
                None => torrent.progress.clamp(0.0, 1.0),
            };
            (total.map(as_i64), as_i64(done), progress)
        };
    let rel_path = manager
        .torrent_content(torrent, &name)
        .and_then(|p| manager.layout.rel(&p));
    let updated = DownloadRow {
        name,
        state,
        progress,
        total_bytes,
        done_bytes,
        down_bps: as_i64(torrent.down_bps),
        up_bps: as_i64(torrent.up_bps),
        peers: torrent.peers.map(i64::from),
        eta_s: torrent.eta_s.map(as_i64),
        error,
        rel_path: rel_path.or_else(|| row.rel_path.clone()),
        info_hash: Some(torrent.info_hash.clone())
            .filter(|h| !h.is_empty())
            .or_else(|| row.info_hash.clone()),
        completed_at,
        ..row.clone()
    };
    let unchanged = updated.name == row.name
        && updated.state == row.state
        && (updated.progress - row.progress).abs() < f64::EPSILON
        && updated.total_bytes == row.total_bytes
        && updated.done_bytes == row.done_bytes
        && updated.down_bps == row.down_bps
        && updated.up_bps == row.up_bps
        && updated.peers == row.peers
        && updated.eta_s == row.eta_s
        && updated.error == row.error
        && updated.rel_path == row.rel_path
        && updated.info_hash == row.info_hash
        && updated.completed_at == row.completed_at;
    if unchanged {
        return Ok(());
    }
    let written = sqlx::query(
        "UPDATE downloads SET name = ?, state = ?, progress = ?, total_bytes = ?, done_bytes = ?, down_bps = ?,
                up_bps = ?, peers = ?, eta_s = ?, error = ?, rel_path = ?, info_hash = ?, completed_at = ?,
                updated_at = ?
         WHERE id = ? AND state = ?",
    )
    .bind(&updated.name)
    .bind(updated.state.as_str())
    .bind(updated.progress)
    .bind(updated.total_bytes)
    .bind(updated.done_bytes)
    .bind(updated.down_bps)
    .bind(updated.up_bps)
    .bind(updated.peers)
    .bind(updated.eta_s)
    .bind(&updated.error)
    .bind(&updated.rel_path)
    .bind(&updated.info_hash)
    .bind(updated.completed_at)
    .bind(t)
    .bind(&row.id)
    // Only over the state this poll read: a pause, resume or removal since then wins.
    .bind(row.state.as_str())
    .execute(&manager.db)
    .await?;
    if written.rows_affected() == 0 {
        return Ok(());
    }
    if updated.state != row.state {
        tracing::info!(download = %row.id, state = %updated.state, "torrent state changed");
    }
    manager.publish_row(&updated);
    Ok(())
}

/// Only explicit deselections in this torrent's manifest are removed, after the
/// engine has let go and adoption succeeded. Never follow links, touch siblings,
/// remove selected files, or act outside Downloads/In Progress.
async fn inspect_payload(
    layout: &StashLayout,
    content: &Path,
    frozen: Arc<Frozen>,
    prune: bool,
) -> std::io::Result<std::fs::Metadata> {
    let layout = layout.clone();
    let content = content.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let witness = frozen
            .release_witness
            .as_ref()
            .ok_or_else(|| std::io::Error::other("Missing release evidence"))?;
        let leaf = content
            .strip_prefix(layout.in_progress())
            .map_err(std::io::Error::other)?;
        torrent_manifest::inspect_payload(layout.root(), leaf, &frozen.files, witness, prune)
    })
    .await
    .map_err(std::io::Error::other)?
}

/// Freeze the exact files that may leave the engine. Old manifests can be read after
/// restart, but missing verification in a new engine response never authorizes an import.
fn verified_selection(files: &mut TorrentFiles, allow_partial: bool) -> ApiResult<i64> {
    if !files.ready || files.review_required {
        return Err(ApiError::conflict(
            "Start the download and wait for verified files.",
        ));
    }
    let mut total = None;
    for file in files.files.iter_mut().filter(|f| f.selected) {
        if file.complete && file.verified_bytes == Some(file.size) {
            total = Some(
                total
                    .unwrap_or(0u64)
                    .checked_add(file.size)
                    .ok_or_else(ApiError::internal)?,
            );
        } else if allow_partial
            && !file.complete
            && file.verified_bytes.is_some_and(|bytes| bytes < file.size)
        {
            file.selected = false;
        } else {
            return Err(ApiError::conflict(
                "Some selected files are still being verified.",
            ));
        }
    }
    let total = total.ok_or_else(|| ApiError::conflict("No selected files are complete yet."))?;
    i64::try_from(total).map_err(|_| ApiError::internal())
}

impl DownloadManager {
    pub async fn finish_verified_files(
        &self,
        id: &str,
        expected_selected: Option<Vec<usize>>,
        expected_complete: &[usize],
    ) -> ApiResult<()> {
        let _controls = self.torrent_slots.lock().await;
        let row = self.load_row(id).await?;
        let engine_id = active_engine_id(&row)?;
        let snapshot = self
            .torrentd
            .completion(engine_id, false)
            .await
            .map_err(|e| engine_error(&e))?;
        let mut files = snapshot.files;
        if let Some(mut expected) = expected_selected {
            // Compare the whole selection before filtering by completion. More data
            // arriving is fine; another device choosing different files needs review.
            expected.sort_unstable();
            expected.dedup();
            let mut selected: Vec<_> = files
                .files
                .iter()
                .filter(|f| f.selected)
                .map(|f| f.index)
                .collect();
            selected.sort_unstable();
            if selected != expected {
                return Err(ApiError::conflict(
                    "The selected files changed. Refresh the file list and review it again.",
                ));
            }
        }
        let total = verified_selection(&mut files, true)?;
        if !expected_complete.is_empty() {
            let verified: std::collections::HashSet<_> = files
                .files
                .iter()
                .filter(|file| file.selected)
                .map(|file| file.index)
                .collect();
            if !expected_complete
                .iter()
                .all(|index| verified.contains(index))
            {
                return Err(ApiError::conflict(
                    "A previously complete file needs verification. Review the files again.",
                ));
            }
        }
        let rel = format!("{}/{}", paths::IN_PROGRESS, snapshot.folder);
        if !is_torrent_content_rel(&rel) || snapshot.folder.contains('\\') {
            return Err(ApiError::conflict(
                "The torrent's output folder is unavailable.",
            ));
        }
        record_completion(
            self,
            &row,
            &row.name,
            &rel,
            total,
            Frozen {
                files,
                release_witness: Some(snapshot.release_witness),
            },
        )
        .await?;
        let decided = self.load_row(id).await?;
        finish_completed(self, &decided).await;
        self.torrent_wake.notify_one();
        let outcome = self.load_row(id).await?;
        if outcome.error.as_deref() == Some(MSG_RELEASE_HOLD) {
            return Err(ApiError::conflict(MSG_RELEASE_HOLD));
        }
        if outcome.state == DownloadState::Paused {
            return Err(ApiError::conflict(
                "The verified files changed. Review the torrent again.",
            ));
        }
        Ok(())
    }
}

/// The torrent is done: record that first (state `completed`, its files' place in
/// `In Progress`), so a restart or a failed step carries on in [`finish_completed`]
/// instead of losing track of a torrent the engine already let go.
async fn finish(
    manager: &DownloadManager,
    row: &DownloadRow,
    torrent: &EngineTorrent,
    name: &str,
) -> ApiResult<()> {
    let Some(rel) = manager
        .torrent_content(torrent, name)
        .and_then(|content| manager.layout.rel(&content))
    else {
        // Keep this row active until the engine has let go. A failed DELETE must
        // never strand a seeder outside the poller's active rows.
        if !remove_finished(manager, row, None).await {
            return Ok(());
        }
        manager.fail(&row.id, MSG_FILES_MISSING).await;
        return Ok(());
    };
    let Some(engine_id) = row.engine_id.as_deref() else {
        return Ok(());
    };
    if let Ok(path) = manager.layout.abs(&rel)
        && tokio::fs::symlink_metadata(path)
            .await
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        if remove_finished(manager, row, None).await {
            manager.fail(&row.id, MSG_FILES_MISSING).await;
        }
        return Ok(());
    }
    let snapshot = match manager.torrentd.completion(engine_id, true).await {
        Ok(snapshot) if rel == format!("{}/{}", paths::IN_PROGRESS, snapshot.folder) => snapshot,
        _ => return Ok(()),
    };
    let mut files = snapshot.files;
    let Ok(total) = verified_selection(&mut files, false) else {
        return Ok(());
    };
    record_completion(
        manager,
        row,
        name,
        &rel,
        total,
        Frozen {
            files,
            release_witness: Some(snapshot.release_witness),
        },
    )
    .await?;
    let row = manager.load_row(&row.id).await?;
    finish_completed(manager, &row).await;
    Ok(())
}

async fn record_completion(
    manager: &DownloadManager,
    row: &DownloadRow,
    name: &str,
    rel: &str,
    total: i64,
    frozen: Frozen,
) -> ApiResult<()> {
    let manifest = serde_json::to_string(&frozen).map_err(|_| ApiError::internal())?;
    let t = now();
    let mut conn = manager.db.acquire().await?;
    sqlx::query("PRAGMA synchronous=FULL")
        .execute(&mut *conn)
        .await?;
    let updated = sqlx::query(
        "UPDATE downloads SET state = 'completed', name = ?, rel_path = ?, progress = 1.0,
                total_bytes = COALESCE(?, total_bytes), done_bytes = COALESCE(?, total_bytes, done_bytes),
                down_bps = 0, up_bps = 0, peers = NULL, eta_s = NULL, error = NULL,
                completed_at = COALESCE(completed_at, ?), updated_at = ?, torrent_files_json = ?
         WHERE id = ? AND state = ? AND engine_id = ?",
    )
    .bind(name)
    .bind(rel)
    .bind(total)
    .bind(total)
    .bind(t)
    .bind(t)
    .bind(&manifest)
    .bind(&row.id)
    .bind(row.state.as_str())
    .bind(&row.engine_id)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    drop(conn);
    if updated == 0 {
        return Err(ApiError::conflict(
            "This download has changed. Refresh its files.",
        ));
    }
    let row = manager.load_row(&row.id).await?;
    manager.publish_row(&row);
    Ok(())
}

const MSG_RELEASE_HOLD: &str = "The torrent files could not be matched to the verified download. Its files are being kept in place for review.";

/// Only a live engine can upgrade a pre-witness decision. Preserve its selected
/// membership; an absent engine is idempotent only after evidence was committed.
async fn frozen_for_release(
    manager: &DownloadManager,
    row: &DownloadRow,
    rel: &str,
) -> ApiResult<Frozen> {
    let stored = row
        .torrent_files_json
        .as_deref()
        .map(serde_json::from_str::<Frozen>)
        .transpose()
        .map_err(|_| ApiError::conflict(MSG_RELEASE_HOLD))?;
    if let Some(
        frozen @ Frozen {
            release_witness: Some(_),
            ..
        },
    ) = stored
    {
        return Ok(frozen);
    }
    let engine = row
        .engine_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict(MSG_RELEASE_HOLD))?;
    let TorrentCompletion {
        folder,
        mut files,
        release_witness,
    } = manager
        .torrentd
        .completion(engine, true)
        .await
        .map_err(|e| engine_error(&e))?;
    if rel != format!("{}/{}", paths::IN_PROGRESS, folder) || !is_torrent_content_rel(rel) {
        return Err(ApiError::conflict(MSG_RELEASE_HOLD));
    }
    if let Some(old) = stored {
        let by_id: HashMap<_, _> = old.files.files.iter().map(|f| (f.index, f)).collect();
        if !old.files.ready
            || old.files.review_required
            || by_id.len() != files.files.len()
            || by_id.len() != old.files.files.len()
        {
            return Err(ApiError::conflict(MSG_RELEASE_HOLD));
        }
        for f in &mut files.files {
            let was = by_id
                .get(&f.index)
                .ok_or_else(|| ApiError::conflict(MSG_RELEASE_HOLD))?;
            if was.path != f.path || was.size != f.size || was.selected && !f.selected {
                return Err(ApiError::conflict(MSG_RELEASE_HOLD));
            }
            f.selected = was.selected;
        }
    } else if files.files.iter().any(|f| !f.selected) {
        // A historical whole-torrent decision contains no proof of a subset.
        return Err(ApiError::conflict(MSG_RELEASE_HOLD));
    }
    verified_selection(&mut files, false)?;
    let frozen = Frozen {
        files,
        release_witness: Some(release_witness),
    };
    let mut conn = manager.db.acquire().await?;
    sqlx::query("PRAGMA synchronous=FULL")
        .execute(&mut *conn)
        .await?;
    let updated = sqlx::query(
        "UPDATE downloads SET torrent_files_json=? WHERE id=? AND state='completed'
        AND engine_id=? AND rel_path=? AND torrent_files_json IS ?",
    )
    .bind(serde_json::to_string(&frozen).map_err(|_| ApiError::internal())?)
    .bind(&row.id)
    .bind(engine)
    .bind(rel)
    .bind(&row.torrent_files_json)
    .execute(&mut *conn)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(ApiError::conflict(MSG_RELEASE_HOLD));
    }
    Ok(frozen)
}

async fn hold_release(manager: &DownloadManager, row: &DownloadRow) -> bool {
    let changed = sqlx::query("UPDATE downloads SET error=?,updated_at=? WHERE id=? AND state='completed' AND rel_path=? AND error IS NOT ?")
        .bind(MSG_RELEASE_HOLD).bind(now()).bind(&row.id).bind(&row.rel_path).bind(MSG_RELEASE_HOLD)
        .execute(&manager.db).await;
    if changed.is_ok_and(|r| r.rows_affected() == 1) {
        let _ = manager.publish_id(&row.id).await;
    }
    false
}

/// A finished torrent whose files are still in `In Progress`: take it out of the engine,
/// have the supervisor adopt its files, move them to `Completed/` and queue the sort.
/// Returns false when it should be tried again later (engine unreachable, move failed).
pub(crate) async fn finish_completed(manager: &DownloadManager, row: &DownloadRow) -> bool {
    let Some(rel) = row
        .rel_path
        .as_deref()
        .filter(|rel| StashLayout::zone(rel) == Zone::InProgress)
    else {
        return true;
    };
    // A rename may already have succeeded. In that case the old name can
    // belong to a different transfer: never adopt, prune or move it again.
    match manager.library.handoff_torrent(&row.id, rel, None).await {
        Ok(true) => {
            let _ = manager.publish_id(&row.id).await;
            return true;
        }
        Ok(false) => {}
        Err(e) => return move_retry(manager, row, &e).await,
    }
    let Some(content) = manager
        .layout
        .abs(rel)
        .ok()
        .filter(|_| is_torrent_content_rel(rel))
    else {
        return hold_release(manager, row).await;
    };
    let Ok(frozen) = frozen_for_release(manager, row, rel).await else {
        return hold_release(manager, row).await;
    };
    let frozen = Arc::new(frozen);
    if inspect_payload(&manager.layout, &content, frozen.clone(), false)
        .await
        .is_err()
    {
        return hold_release(manager, row).await;
    }
    if !remove_finished(manager, row, Some(&frozen.files)).await {
        return false;
    }
    if row.engine_id.is_some() {
        let cleared = sqlx::query(
            "UPDATE downloads SET engine_id=NULL WHERE id=? AND state='completed' AND engine_id=?",
        )
        .bind(&row.id)
        .bind(&row.engine_id)
        .execute(&manager.db)
        .await;
        if !cleared.is_ok_and(|r| r.rows_affected() == 1) {
            return false;
        }
    }
    let component = content
        .file_name()
        .and_then(|n| n.to_str())
        .expect("validated torrent component");
    // A release may have taken time, or completed before the previous process stopped.
    if row.engine_id.is_some()
        && inspect_payload(&manager.layout, &content, frozen.clone(), false)
            .await
            .is_err()
    {
        return hold_release(manager, row).await;
    }
    // Files written by the engine user become PUID:PGID before they move on.
    // Never let engine-owned files into the library: retry later instead of moving.
    if let Err(e) = crate::supervisor_client::adopt(&manager.config, component).await {
        tracing::warn!(error = %format!("{e:#}"), download = %row.id, "the supervisor could not adopt a torrent; will retry");
        return false;
    }
    let prepared = match inspect_payload(&manager.layout, &content, frozen, true).await {
        Ok(meta) => meta,
        Err(_) => return hold_release(manager, row).await,
    };
    match manager
        .library
        .handoff_torrent(&row.id, rel, Some(&prepared))
        .await
    {
        Ok(true) => {
            let _ = manager.publish_id(&row.id).await;
            true
        }
        Ok(false) => false,
        Err(e) => move_retry(manager, row, &e).await,
    }
}

async fn move_retry(
    manager: &DownloadManager,
    row: &DownloadRow,
    error: &stash_core::CoreError,
) -> bool {
    tracing::warn!(%error, download = %row.id, "could not publish a finished torrent; will retry");
    let noted = sqlx::query(
        "UPDATE downloads SET error=?, updated_at=? WHERE id=? AND state='completed' AND rel_path=?",
    )
    .bind(MSG_MOVE_RETRY)
    .bind(now())
    .bind(&row.id)
    .bind(&row.rel_path)
    .execute(&manager.db)
    .await;
    if noted.is_ok_and(|r| r.rows_affected() > 0) {
        let _ = manager.publish_id(&row.id).await;
    }
    false
}

/// Stop uploading even when removal fails, and leave the row eligible for retry.
async fn remove_finished(
    manager: &DownloadManager,
    row: &DownloadRow,
    manifest: Option<&TorrentFiles>,
) -> bool {
    let Some(engine_id) = row.engine_id.as_deref() else {
        return true;
    };
    let checked = manifest.is_some();
    let result = if let Some(manifest) = manifest {
        let Some(folder) = row
            .rel_path
            .as_deref()
            .filter(|r| is_torrent_content_rel(r))
            .and_then(|r| r.rsplit('/').next())
        else {
            return false;
        };
        let indices = manifest
            .files
            .iter()
            .filter(|f| f.selected)
            .map(|f| f.index)
            .collect();
        manager.torrentd.release(engine_id, folder, indices).await
    } else {
        manager.torrentd.remove(engine_id, false).await
    };
    match result {
        Ok(()) => true,
        Err(TorrentdError::Api { status: 404, .. }) if !checked => true,
        Err(TorrentdError::Api { status: 412, .. }) if checked => {
            if manager.torrentd.pause(engine_id).await.is_ok() {
                let reset = sqlx::query("UPDATE downloads SET state='paused', completed_at=NULL, torrent_files_json=NULL,
                    progress=0, done_bytes=0, error=?, updated_at=? WHERE id=? AND state='completed' AND engine_id=?")
                    .bind("The verified files changed. Review this torrent before continuing.")
                    .bind(now()).bind(&row.id).bind(engine_id).execute(&manager.db).await;
                if reset.is_ok() {
                    let _ = manager.publish_id(&row.id).await;
                }
            }
            false
        }
        Err(TorrentdError::Api { status: 409, .. } | TorrentdError::Blocked(_)) if checked => false,
        Err(e) => {
            tracing::warn!(error = %e, download = %row.id, "could not remove a finished torrent; pausing and retrying");
            if let Err(error) = manager.torrentd.pause(engine_id).await {
                tracing::warn!(%error, download = %row.id, "could not pause the finished torrent");
            }
            false
        }
    }
}

/// A torrent's file or folder: one entry directly inside `In Progress`, where the
/// engine creates them. Never the folder itself, anything deeper, the old partial
/// uploads folder or a web download's `.part` file, whatever an engine reports.
fn is_torrent_content_rel(rel: &str) -> bool {
    let Some(name) = rel
        .strip_prefix(paths::IN_PROGRESS)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return false;
    };
    !name.is_empty()
        && !name.contains('/')
        && name != "."
        && name != ".."
        && rel != paths::LEGACY_UPLOADS_TMP
        && !name.ends_with(".part")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine names where a torrent's files are; only one entry directly in In
    /// Progress is ever taken out, so an engine cannot hand over anything else there.
    #[tokio::test]
    async fn selective_completion_removes_only_omitted_files_inside_this_download() {
        use stash_core::internal_api::{TorrentFile, TorrentFileKind, TorrentFiles};
        let dir = tempfile::tempdir().unwrap();
        let layout = StashLayout::new(dir.path());
        layout.ensure().await.unwrap();
        let content = layout.in_progress().join("pack");
        std::fs::create_dir_all(content.join("extras")).unwrap();
        std::fs::write(content.join("film.mkv"), b"selected").unwrap();
        std::fs::write(content.join("extras/instructions.txt"), b"fragment").unwrap();
        let manifest = TorrentFiles {
            review_required: false,
            rules: None,
            ready: true,
            files: vec![
                TorrentFile {
                    index: 0,
                    path: "film.mkv".into(),
                    size: 8,
                    kind: TorrentFileKind::Video,
                    selected: true,
                    verified_bytes: Some(8),
                    complete: true,
                },
                TorrentFile {
                    index: 1,
                    path: "extras/instructions.txt".into(),
                    size: 8,
                    kind: TorrentFileKind::Other,
                    selected: false,
                    verified_bytes: Some(2),
                    complete: false,
                },
            ],
        };
        let proof = torrent_manifest::fixture_witness(&content, &manifest);
        let discard_unselected = |layout: &StashLayout, content: &Path, files: &TorrentFiles| {
            let layout = layout.clone();
            let content = content.to_owned();
            let frozen = Arc::new(Frozen {
                files: files.clone(),
                release_witness: Some(proof.clone()),
            });
            async move { inspect_payload(&layout, &content, frozen, true).await }
        };
        discard_unselected(&layout, &content, &manifest)
            .await
            .unwrap();
        assert!(content.join("film.mkv").exists());
        assert!(!content.join("extras").exists());
        // Interrupted completion can safely retry the same manifest.
        discard_unselected(&layout, &content, &manifest)
            .await
            .unwrap();
        let outside = layout.in_progress().join("other.txt");
        std::fs::write(&outside, b"keep").unwrap();
        let mut invalid = manifest.clone();
        invalid.files[1].path = "../other.txt".into();
        assert!(
            discard_unselected(&layout, &content, &invalid)
                .await
                .is_err()
        );
        assert!(outside.exists());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(layout.in_progress(), content.join("link")).unwrap();
            invalid.files[1].path = "link/other.txt".into();
            assert!(
                discard_unselected(&layout, &content, &invalid)
                    .await
                    .is_err()
            );
            assert!(outside.exists());
            std::os::unix::fs::symlink(&outside, content.join("linked.txt")).unwrap();
            invalid.files[1].path = "linked.txt".into();
            assert!(
                discard_unselected(&layout, &content, &invalid)
                    .await
                    .is_err()
            );
            assert!(outside.exists());
            assert!(content.join("linked.txt").is_symlink());
            std::os::unix::fs::symlink(&content, layout.in_progress().join("linked-pack")).unwrap();
            assert!(
                discard_unselected(
                    &layout,
                    &layout.in_progress().join("linked-pack"),
                    &manifest
                )
                .await
                .is_err()
            );
            assert!(content.join("film.mkv").exists());
        }
    }

    #[test]
    fn torrent_content_is_one_entry_in_in_progress() {
        for ok in [
            "Downloads/In Progress/Big Movie (2019)",
            "Downloads/In Progress/movie.mkv",
        ] {
            assert!(is_torrent_content_rel(ok), "{ok}");
        }
        for bad in [
            "Downloads/In Progress",
            "Downloads/In Progress/",
            "Downloads/In Progress/a/b",
            "Downloads/In Progress/.uploads",
            "Downloads/In Progress/clip.mp4.part",
            "Downloads/In Progressive/x",
            "Downloads/Completed/x",
            "Movies/x",
            ".popcornstash/uploads/x.part",
        ] {
            assert!(!is_torrent_content_rel(bad), "{bad}");
        }
    }

    #[test]
    fn bencode_validation() {
        let good = b"d8:announce14:http://t.local4:infod6:lengthi12e4:name9:movie.mkv12:piece lengthi16384e6:pieces0:ee";
        assert_eq!(torrent_name(good).unwrap().as_deref(), Some("movie.mkv"));
        assert!(torrent_name(b"<html>login</html>").is_err());
        assert!(torrent_name(b"d4:infod4:name1:x").is_err(), "truncated");
        assert!(torrent_name(b"d3:foo3:bare").is_err(), "no info");
        assert!(torrent_name(b"d4:infod4:name1:xeetrailing").is_err());
        assert!(torrent_name(b"li1ee").is_err(), "not a dict");
        assert!(torrent_name(b"d4:infod4:name99999999999999999999:xee").is_err());
        let deep = format!("d4:info{}{}e", "l".repeat(200), "e".repeat(200));
        assert!(torrent_name(deep.as_bytes()).is_err());
    }
}
