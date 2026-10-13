//! Full rescans and single-file indexing (section 5, Watcher).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use super::classify::{classify, media_kind};
use super::parse::{parse_name, season_folder};
use super::{Library, ScanReport, organizer, repo};
use crate::events::{Event, LibraryChange};
use crate::jobs::JobKind;
use crate::models::{Category, Classification, ItemRow, ItemStatus, MediaKind, ThumbState};
use crate::paths::{self, StashLayout, Zone};
use crate::{CoreError, Result, new_id, now};

/// Missing items are forgotten after this long.
pub const MISSING_RETENTION_S: i64 = 7 * 86_400;
/// Inbox and Completed entries must be this old (seconds since last change) before a rescan sorts them.
pub const STABLE_AFTER_S: i64 = 10;
const HASH_CHUNK: u64 = 1024 * 1024;

/// What indexing one file did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    Added,
    Updated,
    /// A missing item was found again under a new path.
    Moved,
    Unchanged,
    /// A changed file in Inbox or Completed was handed back to the sorter.
    QueuedForSort,
}

/// Metadata derived from a path or a classification.
#[derive(Debug, Clone, Default)]
pub(crate) struct Meta {
    pub title: String,
    pub year: Option<i64>,
    pub show_name: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub episode_title: Option<String>,
    pub taken_at: Option<i64>,
}

impl Meta {
    pub fn from_classification(c: &Classification) -> Self {
        Self {
            title: c.title.clone(),
            year: c.year,
            show_name: c.show_name.clone(),
            season: c.season,
            episode: c.episode,
            episode_title: c.episode_title.clone(),
            taken_at: c.taken_at,
        }
    }
}

/// xxh3-64 of size + first 1 MiB + last 1 MiB, hex.
pub fn quick_hash(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    hasher.update(&size.to_le_bytes());
    let mut buf = Vec::with_capacity(HASH_CHUNK as usize);
    (&mut file).take(HASH_CHUNK).read_to_end(&mut buf)?;
    hasher.update(&buf);
    if size > HASH_CHUNK {
        let tail_start = size.saturating_sub(HASH_CHUNK).max(HASH_CHUNK);
        file.seek(SeekFrom::Start(tail_start))?;
        buf.clear();
        file.take(HASH_CHUNK).read_to_end(&mut buf)?;
        hasher.update(&buf);
    }
    Ok(format!("{:016x}", hasher.digest()))
}

pub(crate) async fn quick_hash_async(path: &Path) -> Option<String> {
    let path = path.to_path_buf();
    match tokio::task::spawn_blocking(move || quick_hash(&path)).await {
        Ok(Ok(hash)) => Some(hash),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "could not hash file");
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "hash task failed");
            None
        }
    }
}

/// Modification time in unix seconds.
pub(crate) fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

/// Trim slashes and refuse `..`, `.` and empty paths.
pub(crate) fn normalize_rel(rel: &str) -> Result<String> {
    let rel = rel.trim().trim_matches('/');
    if rel.is_empty()
        || rel
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || rel.contains('\\')
    {
        return Err(CoreError::invalid("That path is not inside the stash."));
    }
    Ok(rel.to_string())
}

/// NAS and OS folders that never hold library media.
pub(crate) fn is_junk_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    paths::is_ignored_name(name)
        || matches!(
            lower.as_str(),
            "lost+found"
                | "#recycle"
                | "$recycle.bin"
                | "system volume information"
                | "#snapshot"
                | "@eadir"
                | "@recycle"
        )
}

pub(crate) const SIDECAR_EXTS: &[&str] = &["srt", "ass", "vtt", "sub", "idx", "nfo", "xmp", "aae"];
pub(crate) const SAMPLE_MAX_BYTES: u64 = 150 * 1024 * 1024;
const EXTRAS_DIRS: &[&str] = &[
    "extras",
    "extra",
    "featurettes",
    "behind the scenes",
    "deleted scenes",
    "interviews",
    "trailers",
    "sample",
    "samples",
    "bonus",
    "shorts",
];

pub(crate) fn is_sidecar(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| SIDECAR_EXTS.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
}

pub(crate) fn is_extras_dir(name: &str) -> bool {
    EXTRAS_DIRS.contains(&name.to_ascii_lowercase().as_str())
}

pub(crate) fn is_sample(name: &str, size: u64) -> bool {
    size < SAMPLE_MAX_BYTES && name.to_ascii_lowercase().contains("sample")
}

/// Whether a file in `zone` becomes an item at all.
fn indexable(zone: Zone, rel: &str, kind: MediaKind, size: u64) -> bool {
    let path = Path::new(rel);
    if is_sidecar(path) {
        return false;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dirs: Vec<&str> = rel.split('/').collect();
    // Folders inside a movie or show folder (not the category folder, and not the title
    // folder itself: "Shows/Extras/" is a TV show).
    let in_extras = dirs
        .iter()
        .take(dirs.len().saturating_sub(1))
        .skip(2)
        .any(|d| is_extras_dir(d));
    match zone {
        // Artwork, theme songs, samples and extras next to movies and shows are not episodes or movies.
        Zone::Library(Category::Movie | Category::Episode) => {
            kind == MediaKind::Video && !in_extras && !is_sample(&name, size)
        }
        Zone::Library(Category::HomeVideo | Category::Photo) => {
            matches!(kind, MediaKind::Video | MediaKind::Photo)
        }
        Zone::Library(Category::Other) => true,
        Zone::Custom | Zone::Inbox | Zone::Completed => kind != MediaKind::Other,
        Zone::InProgress | Zone::Internal => false,
    }
}

/// Metadata for a file inside one of the library folders, from its path.
pub(crate) fn library_metadata(category: Category, rel: &str, mtime: i64) -> Meta {
    let path = Path::new(rel);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parts: Vec<&str> = rel.split('/').collect();
    // Folders between the category folder and the file.
    let dirs: &[&str] = if parts.len() > 2 {
        &parts[1..parts.len() - 1]
    } else {
        &[]
    };
    let file = parse_name(&stem);
    match category {
        Category::Movie => {
            let folder = dirs.last().map(|d| parse_name(d));
            let (title, year) = match folder {
                Some(f) if f.year.is_some() && !f.title.is_empty() => (f.title, f.year),
                _ if file.year.is_some() && !file.title.is_empty() => {
                    (file.title.clone(), file.year)
                }
                Some(f) if !f.title.is_empty() => (f.title, None),
                _ if !file.title.is_empty() => (file.title.clone(), None),
                _ => (stem.clone(), None),
            };
            Meta {
                title,
                year,
                ..Meta::default()
            }
        }
        Category::Episode => {
            let show = dirs
                .first()
                .map(|d| {
                    let parsed = parse_name(d).title;
                    if parsed.is_empty() {
                        d.to_string()
                    } else {
                        parsed
                    }
                })
                .filter(|s| !s.is_empty())
                .or_else(|| (!file.title.is_empty()).then(|| file.title.clone()));
            let season = dirs
                .iter()
                .rev()
                .find_map(|d| season_folder(d))
                .or(file.season);
            let episode = file.episode;
            let title = match (&show, season, episode) {
                (Some(show), Some(s), Some(e)) => format!("{show} S{s:02}E{e:02}"),
                _ => stem.clone(),
            };
            Meta {
                title,
                year: file.year,
                show_name: show,
                season,
                episode,
                episode_title: file.episode_title.clone(),
                taken_at: None,
            }
        }
        Category::HomeVideo | Category::Photo => Meta {
            title: stem.clone(),
            taken_at: Some(file.date.unwrap_or(mtime)),
            ..Meta::default()
        },
        Category::Other => Meta {
            title: stem.clone(),
            ..Meta::default()
        },
    }
}

/// A fresh row for a file. Media fields stay empty until the probe job fills them.
pub(crate) fn new_row(
    rel: &str,
    category: Category,
    status: ItemStatus,
    meta: Meta,
    size: i64,
    mtime: i64,
    quick_hash: Option<String>,
) -> ItemRow {
    let t = now();
    let path = Path::new(rel);
    ItemRow {
        id: new_id(),
        rel_path: rel.to_string(),
        kind: media_kind(path),
        category,
        status,
        sort_title: repo::sort_title(&meta.title),
        title: meta.title,
        year: meta.year,
        show_name: meta.show_name,
        season: meta.season,
        episode: meta.episode,
        episode_title: meta.episode_title,
        size,
        mtime,
        quick_hash,
        mime: mime_guess::from_path(path).first_raw().map(str::to_string),
        duration_s: None,
        width: None,
        height: None,
        media_json: None,
        taken_at: meta.taken_at,
        camera: None,
        thumb_state: ThumbState::Pending,
        suggestion_json: None,
        favorite: false,
        source_download_id: None,
        missing_since: None,
        added_at: t,
        updated_at: t,
    }
}

/// Overwrite an existing row's content with `row` (keeps id, favorite, added_at and,
/// when `row` has none, the source download).
pub(crate) async fn overwrite_row(lib: &Library, id: &str, row: &ItemRow) -> Result<()> {
    sqlx::query(
        "UPDATE items SET rel_path = ?, kind = ?, category = ?, status = ?, title = ?, sort_title = ?, year = ?,
            show_name = ?, season = ?, episode = ?, episode_title = ?, size = ?, mtime = ?, quick_hash = ?, mime = ?,
            duration_s = ?, width = ?, height = ?, media_json = ?, taken_at = ?, camera = ?, thumb_state = ?,
            suggestion_json = ?, source_download_id = COALESCE(?, source_download_id), missing_since = NULL,
            updated_at = ?
         WHERE id = ?",
    )
    .bind(&row.rel_path)
    .bind(row.kind.as_str())
    .bind(row.category.as_str())
    .bind(row.status.as_str())
    .bind(&row.title)
    .bind(&row.sort_title)
    .bind(row.year)
    .bind(&row.show_name)
    .bind(row.season)
    .bind(row.episode)
    .bind(&row.episode_title)
    .bind(row.size)
    .bind(row.mtime)
    .bind(&row.quick_hash)
    .bind(&row.mime)
    .bind(row.duration_s)
    .bind(row.width)
    .bind(row.height)
    .bind(&row.media_json)
    .bind(row.taken_at)
    .bind(&row.camera)
    .bind(row.thumb_state.as_str())
    .bind(&row.suggestion_json)
    .bind(&row.source_download_id)
    .bind(now())
    .bind(id)
    .execute(&lib.db)
    .await?;
    Ok(())
}

/// A missing item with the same size and quick hash: the same file under a new name.
pub(crate) async fn find_moved(
    lib: &Library,
    size: i64,
    hash: Option<&str>,
) -> Result<Option<ItemRow>> {
    let Some(hash) = hash else { return Ok(None) };
    Ok(sqlx::query_as::<_, ItemRow>(
        "SELECT * FROM items WHERE status = 'missing' AND size = ? AND quick_hash = ?
         ORDER BY missing_since DESC, id LIMIT 1",
    )
    .bind(size)
    .bind(hash)
    .fetch_optional(&lib.db)
    .await?)
}

/// Queue probe and thumbnail work for an item.
pub(crate) async fn enqueue_media_jobs(lib: &Library, item_id: &str) -> Result<()> {
    lib.jobs
        .enqueue(JobKind::Probe, Some(item_id), serde_json::json!({}))
        .await?;
    lib.jobs
        .enqueue(JobKind::Thumb, Some(item_id), serde_json::json!({}))
        .await?;
    Ok(())
}

pub(crate) fn publish(lib: &Library, change: LibraryChange, item_id: &str) {
    lib.events.publish(Event::Library {
        r#type: change,
        item_id: item_id.to_string(),
    });
}

/// Mark the item at `rel` (and any items below it, for a folder) missing. Items whose
/// file is (again) on disk are left alone.
pub async fn mark_missing(lib: &Library, rel: &str) -> Result<u64> {
    let rel = normalize_rel(rel)?;
    let candidates: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, rel_path FROM items
         WHERE status IN ('ready', 'needs_review') AND (rel_path = ? OR substr(rel_path, 1, ?) = ?)",
    )
    .bind(&rel)
    .bind(i64::try_from(rel.chars().count() + 1).unwrap_or(i64::MAX))
    .bind(format!("{rel}/"))
    .fetch_all(&lib.db)
    .await?;
    if candidates.is_empty() {
        return Ok(0);
    }
    let review_before = repo::needs_review_count(&lib.db).await?;
    let ids = mark_vanished(lib, &candidates).await?;
    for id in &ids {
        publish(lib, LibraryChange::Updated, id);
    }
    organizer::publish_inbox_if_changed(lib, review_before).await?;
    Ok(ids.len() as u64)
}

/// Whether a stored path is gone right now: not found, or no longer a regular file.
/// Unreadable paths (permissions, I/O errors on a share) count as present.
pub(crate) async fn is_gone(lib: &Library, rel: &str) -> bool {
    let Ok(abs) = lib.layout.abs(rel) else {
        // A symbolic link now leads out of the stash: the file is no longer reachable.
        return true;
    };
    match tokio::fs::symlink_metadata(&abs).await {
        Ok(meta) => !meta.is_file(),
        Err(e) => is_not_found(&e),
    }
}

/// Not found, or a parent turned into a file (ENOTDIR).
pub(crate) fn is_not_found(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::NotFound || e.raw_os_error() == Some(libc::ENOTDIR)
}

/// Mark `(id, rel_path)` items missing, re-checking each under the move lock: a file that
/// is back on disk, or a row that points somewhere else since the caller looked (the
/// sorter or an API action moved it), is left alone. Returns the ids marked missing.
pub(crate) async fn mark_vanished(
    lib: &Library,
    items: &[(String, String)],
) -> Result<Vec<String>> {
    let mut marked = Vec::new();
    for (id, rel) in items {
        let _moves = lib.lock_moves().await;
        if !is_gone(lib, rel).await {
            continue;
        }
        let t = now();
        let done = sqlx::query(
            "UPDATE items SET status = 'missing', missing_since = ?, updated_at = ?
             WHERE id = ? AND rel_path = ? AND status IN ('ready', 'needs_review')",
        )
        .bind(t)
        .bind(t)
        .bind(id)
        .bind(rel)
        .execute(&lib.db)
        .await?;
        if done.rows_affected() == 1 {
            marked.push(id.clone());
        }
    }
    Ok(marked)
}

pub async fn index_path(lib: &Library, rel_path: &str) -> Result<Option<ItemRow>> {
    Ok(index_inner(lib, rel_path).await?.map(|(row, _)| row))
}

/// Index one file in place and say what changed.
pub(crate) async fn index_inner(
    lib: &Library,
    rel_path: &str,
) -> Result<Option<(ItemRow, Change)>> {
    let _moves = lib.lock_moves().await;
    index_locked(lib, rel_path).await
}

/// Mark items at `rel` (or below it) missing without re-checking the disk. The caller holds
/// the move lock and has just seen the path vanish.
async fn mark_missing_now(lib: &Library, rel: &str) -> Result<()> {
    let t = now();
    let ids: Vec<String> = sqlx::query_scalar(
        "UPDATE items SET status = 'missing', missing_since = ?, updated_at = ?
         WHERE status IN ('ready', 'needs_review') AND (rel_path = ? OR substr(rel_path, 1, ?) = ?)
         RETURNING id",
    )
    .bind(t)
    .bind(t)
    .bind(rel)
    .bind(i64::try_from(rel.chars().count() + 1).unwrap_or(i64::MAX))
    .bind(format!("{rel}/"))
    .fetch_all(&lib.db)
    .await?;
    for id in &ids {
        publish(lib, LibraryChange::Updated, id);
    }
    Ok(())
}

/// [`index_inner`] with the move lock already held.
async fn index_locked(lib: &Library, rel_path: &str) -> Result<Option<(ItemRow, Change)>> {
    let rel = normalize_rel(rel_path)?;
    if rel.split('/').any(is_junk_name) {
        return Ok(None);
    }
    let zone = StashLayout::zone(&rel);
    if !zone.indexed() {
        return Ok(None);
    }
    let abs = lib.layout.abs(&rel)?;
    let meta = match tokio::fs::symlink_metadata(&abs).await {
        Ok(meta) => meta,
        Err(e) if is_not_found(&e) => {
            mark_missing_now(lib, &rel).await?;
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() {
        return Ok(None);
    }
    let kind = media_kind(&abs);
    if !indexable(zone, &rel, kind, meta.len()) {
        return Ok(None);
    }
    let size = i64::try_from(meta.len()).unwrap_or(i64::MAX);
    let mtime = mtime_secs(&meta);
    let existing = repo::get_by_path(&lib.db, &rel).await?;

    // Inbox and Completed belong to the sorter; only refresh items it already left there.
    if zone.auto_sorts() && existing.is_none() {
        return Ok(None);
    }

    if let Some(item) = existing {
        if item.status == ItemStatus::Trashed {
            return Ok(None);
        }
        let restored_status = match item.status {
            ItemStatus::Missing if zone.auto_sorts() && item.suggestion_json.is_some() => {
                ItemStatus::NeedsReview
            }
            ItemStatus::Missing => ItemStatus::Ready,
            other => other,
        };
        if item.size == size && item.mtime == mtime {
            if item.status == ItemStatus::Missing {
                sqlx::query("UPDATE items SET status = ?, missing_since = NULL, updated_at = ? WHERE id = ?")
                    .bind(restored_status.as_str())
                    .bind(now())
                    .bind(&item.id)
                    .execute(&lib.db)
                    .await?;
                publish(lib, LibraryChange::Updated, &item.id);
                if restored_status == ItemStatus::NeedsReview {
                    lib.events.publish(Event::Inbox {
                        count: repo::needs_review_count(&lib.db).await?,
                    });
                }
                let row = repo::get(&lib.db, &item.id)
                    .await?
                    .ok_or(CoreError::NotFound)?;
                return Ok(Some((row, Change::Updated)));
            }
            return Ok(Some((item, Change::Unchanged)));
        }
        // A changed file waiting for review in Inbox or Completed is the sorter's to look at
        // again. One the owner reviewed (ignored) stays where it is; only its facts refresh.
        if zone.auto_sorts() && restored_status == ItemStatus::NeedsReview {
            let queued = organizer::enqueue_sort_inner(lib, &rel, None).await?;
            let change = if queued {
                Change::QueuedForSort
            } else {
                Change::Unchanged
            };
            return Ok(Some((item, change)));
        }
        // The file changed in place: keep the user's metadata, refresh the file facts.
        let hash = quick_hash_async(&abs).await;
        let suggestion = if zone == Zone::Custom {
            Some(serde_json::to_string(&classify(&rel, None, None, mtime))?)
        } else {
            item.suggestion_json.clone()
        };
        sqlx::query(
            "UPDATE items SET size = ?, mtime = ?, quick_hash = ?, status = ?, missing_since = NULL,
                thumb_state = 'pending', suggestion_json = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(size)
        .bind(mtime)
        .bind(&hash)
        .bind(restored_status.as_str())
        .bind(&suggestion)
        .bind(now())
        .bind(&item.id)
        .execute(&lib.db)
        .await?;
        enqueue_media_jobs(lib, &item.id).await?;
        publish(lib, LibraryChange::Updated, &item.id);
        let row = repo::get(&lib.db, &item.id)
            .await?
            .ok_or(CoreError::NotFound)?;
        return Ok(Some((row, Change::Updated)));
    }

    let hash = quick_hash_async(&abs).await;
    let (category, meta, suggestion) = match zone {
        Zone::Library(category) => (category, library_metadata(category, &rel, mtime), None),
        _ => {
            let c = classify(&rel, None, None, mtime);
            (c.category, Meta::from_classification(&c), Some(c))
        }
    };

    // Same bytes as a missing item: it was moved or renamed outside PopcornStash.
    if let Some(old) = find_moved(lib, size, hash.as_deref()).await? {
        let category_changed = matches!(zone, Zone::Library(c) if c != old.category);
        let mut row = old.clone();
        row.rel_path = rel.clone();
        row.mtime = mtime;
        row.status = ItemStatus::Ready;
        row.kind = kind;
        if category_changed {
            row.category = category;
            row.title = meta.title.clone();
            row.sort_title = repo::sort_title(&meta.title);
            row.year = meta.year;
            row.show_name = meta.show_name.clone();
            row.season = meta.season;
            row.episode = meta.episode;
            row.episode_title = meta.episode_title.clone();
            row.taken_at = meta.taken_at.or(old.taken_at);
        }
        row.source_download_id = None;
        overwrite_row(lib, &old.id, &row).await?;
        publish(lib, LibraryChange::Updated, &old.id);
        tracing::info!(item = %old.id, from = %old.rel_path, to = %rel, "found a moved file");
        let row = repo::get(&lib.db, &old.id)
            .await?
            .ok_or(CoreError::NotFound)?;
        return Ok(Some((row, Change::Moved)));
    }

    let mut row = new_row(&rel, category, ItemStatus::Ready, meta, size, mtime, hash);
    row.suggestion_json = suggestion.map(|c| serde_json::to_string(&c)).transpose()?;
    if !repo::insert_item(&lib.db, &row).await? {
        // Someone else indexed it a moment ago.
        let current = repo::get_by_path(&lib.db, &rel)
            .await?
            .ok_or(CoreError::NotFound)?;
        return Ok(Some((current, Change::Unchanged)));
    }
    enqueue_media_jobs(lib, &row.id).await?;
    publish(lib, LibraryChange::Added, &row.id);
    Ok(Some((row, Change::Added)))
}

// ---------------------------------------------------------------------------
// Full rescans

struct WalkedFile {
    size: i64,
    mtime: i64,
}

struct SortCandidate {
    rel: String,
    newest_mtime: i64,
    /// Media files inside (or the file itself) that the sorter would look at.
    media: Vec<String>,
}

#[derive(Default)]
struct Walk {
    files: HashMap<String, WalkedFile>,
    candidates: Vec<SortCandidate>,
}

fn walk_stash(layout: &StashLayout) -> Walk {
    let root = layout.root();
    let mut walk = Walk::default();
    let entries = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            if is_junk_name(&e.file_name().to_string_lossy()) {
                return false;
            }
            match layout.rel(e.path()) {
                Some(rel) => StashLayout::zone(&rel).indexed(),
                None => false,
            }
        });
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                tracing::warn!(error = %e, "could not read part of the stash during a rescan");
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(rel) = layout.rel(entry.path()) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else { continue };
        walk.files.insert(
            rel,
            WalkedFile {
                size: i64::try_from(meta.len()).unwrap_or(i64::MAX),
                mtime: mtime_secs(&meta),
            },
        );
    }
    walk.candidates = sort_candidates(layout);
    walk
}

/// Direct children of `Inbox/`, `Downloads/Completed/` and loose entries in `Downloads/`.
fn sort_candidates(layout: &StashLayout) -> Vec<SortCandidate> {
    let mut out = Vec::new();
    let downloads = layout.root().join(paths::DOWNLOADS);
    for dir in [layout.inbox(), layout.completed(), downloads.clone()] {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_junk_name(&name) || path == layout.in_progress() || path == layout.completed() {
                continue;
            }
            let Some(rel) = layout.rel(&path) else {
                continue;
            };
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let completed = StashLayout::zone(&rel) == Zone::Completed;
            if meta.is_file() {
                let mut media = Vec::new();
                if media_kind(&path) != MediaKind::Other
                    && !(completed && is_sample(&name, meta.len()))
                {
                    media.push(rel.clone());
                }
                out.push(SortCandidate {
                    rel,
                    newest_mtime: mtime_secs(&meta),
                    media,
                });
            } else if meta.is_dir() {
                let mut newest = mtime_secs(&meta);
                let mut media = Vec::new();
                let inner = WalkDir::new(&path)
                    .follow_links(false)
                    .into_iter()
                    .filter_entry(|e| {
                        let n = e.file_name().to_string_lossy();
                        e.depth() == 0
                            || !(is_junk_name(&n)
                                || (completed && e.file_type().is_dir() && is_extras_dir(&n)))
                    });
                for e in inner.flatten() {
                    let Ok(m) = e.metadata() else { continue };
                    newest = newest.max(mtime_secs(&m));
                    if e.file_type().is_file()
                        && media_kind(e.path()) != MediaKind::Other
                        && !(completed && is_sample(&e.file_name().to_string_lossy(), m.len()))
                        && let Some(r) = layout.rel(e.path())
                    {
                        media.push(r);
                    }
                }
                out.push(SortCandidate {
                    rel,
                    newest_mtime: newest,
                    media,
                });
            }
        }
    }
    out
}

struct KnownItem {
    id: String,
    rel_path: String,
    size: i64,
    mtime: i64,
    status: ItemStatus,
    missing_since: Option<i64>,
}

pub async fn rescan(lib: &Library) -> Result<ScanReport> {
    let started = now();
    let review_before = repo::needs_review_count(&lib.db).await?;
    let layout = lib.layout.clone();
    let walk = tokio::task::spawn_blocking(move || walk_stash(&layout))
        .await
        .map_err(|e| CoreError::Other(e.into()))?;
    let mut report = ScanReport::default();

    let known: Vec<KnownItem> = sqlx::query_as::<_, (String, String, i64, i64, ItemStatus, Option<i64>)>(
        "SELECT id, rel_path, size, mtime, status, missing_since FROM items WHERE status != 'trashed'",
    )
    .fetch_all(&lib.db)
    .await?
    .into_iter()
    .map(|(id, rel_path, size, mtime, status, missing_since)| KnownItem { id, rel_path, size, mtime, status, missing_since })
    .collect();

    // An unmounted share or a wrong mount looks like a stash where every file vanished.
    // Never conclude that from an empty walk: nothing is marked missing or forgotten.
    if walk.files.is_empty() && !known.is_empty() {
        tracing::warn!(
            items = known.len(),
            root = %lib.layout.root().display(),
            "the stash folder has no files; skipping the rescan"
        );
        return Err(CoreError::external(
            "The stash folder has no files, so nothing was marked missing. Check that the folder or share is mounted.",
        ));
    }

    // 1. Files that vanished. Mark them missing first so moved files can be matched to them.
    //    Each is re-checked on disk: the walk is a snapshot and the sorter may have moved
    //    a file into place since.
    let mut vanished = Vec::new();
    let mut stale = Vec::new();
    for item in &known {
        if walk.files.contains_key(&item.rel_path) {
            continue;
        }
        if item.status == ItemStatus::Missing {
            if item
                .missing_since
                .is_some_and(|t| t < started - MISSING_RETENTION_S)
            {
                stale.push(item);
            }
        } else {
            vanished.push((item.id.clone(), item.rel_path.clone()));
        }
    }
    let newly_missing = mark_vanished(lib, &vanished).await?;

    // 2. New and changed files.
    let by_path: HashMap<&str, &KnownItem> =
        known.iter().map(|k| (k.rel_path.as_str(), k)).collect();
    let mut paths: Vec<&String> = walk.files.keys().collect();
    paths.sort();
    for rel in paths {
        let file = &walk.files[rel];
        let zone = StashLayout::zone(rel);
        match by_path.get(rel.as_str()) {
            Some(k)
                if k.size == file.size
                    && k.mtime == file.mtime
                    && k.status != ItemStatus::Missing =>
            {
                continue;
            }
            None if zone.auto_sorts() => continue,
            None if !indexable(
                zone,
                rel,
                media_kind(Path::new(rel)),
                u64::try_from(file.size).unwrap_or(0),
            ) =>
            {
                continue;
            }
            _ => {}
        }
        match index_inner(lib, rel).await {
            Ok(Some((_, Change::Added))) => report.added += 1,
            Ok(Some((_, Change::Updated))) => report.updated += 1,
            Ok(Some((_, Change::Moved))) => report.moved += 1,
            Ok(Some((_, Change::QueuedForSort))) => report.queued_for_sort += 1,
            Ok(_) => {}
            Err(CoreError::Db(e)) => return Err(CoreError::Db(e)),
            Err(e) => tracing::warn!(error = %e, path = %rel, "could not index file"),
        }
    }

    // 3. Report what is still missing, and forget long-missing items.
    for id in &newly_missing {
        let status: Option<ItemStatus> =
            sqlx::query_scalar("SELECT status FROM items WHERE id = ?")
                .bind(id)
                .fetch_optional(&lib.db)
                .await?;
        if status == Some(ItemStatus::Missing) {
            report.missing += 1;
            publish(lib, LibraryChange::Updated, id);
        }
    }
    for item in &stale {
        let deleted = {
            let _moves = lib.lock_moves().await;
            if !is_gone(lib, &item.rel_path).await {
                continue;
            }
            sqlx::query("DELETE FROM items WHERE id = ? AND rel_path = ? AND status = 'missing'")
                .bind(&item.id)
                .bind(&item.rel_path)
                .execute(&lib.db)
                .await?
        };
        if deleted.rows_affected() == 1 {
            report.removed += 1;
            organizer::remove_cached_media(lib, &item.id).await;
            publish(lib, LibraryChange::Removed, &item.id);
        }
    }

    // 4. Queue Inbox and Completed entries that are stable and not yet items.
    for candidate in &walk.candidates {
        if candidate.newest_mtime > started - STABLE_AFTER_S {
            continue;
        }
        let unsorted = candidate
            .media
            .iter()
            .any(|m| !by_path.contains_key(m.as_str()));
        if !unsorted {
            continue;
        }
        match organizer::enqueue_sort_inner(lib, &candidate.rel, None).await {
            Ok(true) => report.queued_for_sort += 1,
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, path = %candidate.rel, "could not queue a sort"),
        }
    }

    // 5. Trash retention (setting `trash_retention_days`, 30 by default).
    let retention = lib.settings.get().await.trash_retention_days;
    match organizer::empty_trash(lib, Some(retention)).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(
            count = n,
            days = retention,
            "deleted items past the trash retention"
        ),
        Err(e) => tracing::warn!(error = %e, "could not apply the trash retention"),
    }
    organizer::publish_inbox_if_changed(lib, review_before).await?;
    Ok(report)
}

/// Every regular file under `abs` (a file or folder), skipping junk and symlinks.
pub(crate) fn files_under(abs: &Path) -> Vec<PathBuf> {
    WalkDir::new(abs)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_junk_name(&e.file_name().to_string_lossy()))
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_hash_covers_size_head_and_tail() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        std::fs::write(&small, b"hello").unwrap();
        let h1 = quick_hash(&small).unwrap();
        assert_eq!(h1.len(), 16);
        assert_eq!(h1, quick_hash(&small).unwrap());
        std::fs::write(&small, b"hellp").unwrap();
        assert_ne!(h1, quick_hash(&small).unwrap());

        // 3 MiB: a change in the middle is not seen, a change in the tail is.
        let big = dir.path().join("big");
        let mut data = vec![7u8; 3 * 1024 * 1024];
        std::fs::write(&big, &data).unwrap();
        let base = quick_hash(&big).unwrap();
        data[1536 * 1024] = 1;
        std::fs::write(&big, &data).unwrap();
        assert_eq!(base, quick_hash(&big).unwrap());
        data[3 * 1024 * 1024 - 1] = 1;
        std::fs::write(&big, &data).unwrap();
        assert_ne!(base, quick_hash(&big).unwrap());
        // Size is part of the hash.
        let empty = dir.path().join("empty");
        std::fs::write(&empty, b"").unwrap();
        assert_ne!(quick_hash(&empty).unwrap(), quick_hash(&small).unwrap());
    }

    #[test]
    fn rel_normalization() {
        assert_eq!(normalize_rel("/Movies/a.mkv/").unwrap(), "Movies/a.mkv");
        assert!(normalize_rel("../etc").is_err());
        assert!(normalize_rel("Movies//a").is_err());
        assert!(normalize_rel("").is_err());
    }

    #[test]
    fn library_paths() {
        let m = library_metadata(Category::Movie, "Movies/Heat (1995)/Heat (1995).mkv", 0);
        assert_eq!((m.title.as_str(), m.year), ("Heat", Some(1995)));
        let m = library_metadata(Category::Movie, "Movies/heat.1995.1080p.mkv", 0);
        assert_eq!((m.title.as_str(), m.year), ("Heat", Some(1995)));
        let m = library_metadata(Category::Movie, "Movies/Heat (1995)/movie.mkv", 0);
        assert_eq!((m.title.as_str(), m.year), ("Heat", Some(1995)));
        let m = library_metadata(Category::Movie, "Movies/Old Home Movie.mkv", 0);
        assert_eq!((m.title.as_str(), m.year), ("Old Home Movie", None));

        let e = library_metadata(
            Category::Episode,
            "Shows/The Office/Season 02/The Office - S02E03.mkv",
            0,
        );
        assert_eq!(e.show_name.as_deref(), Some("The Office"));
        assert_eq!((e.season, e.episode), (Some(2), Some(3)));
        assert_eq!(e.title, "The Office S02E03");
        let e = library_metadata(
            Category::Episode,
            "Shows/Severance/Specials/S00E01 Making of.mkv",
            0,
        );
        assert_eq!((e.season, e.episode), (Some(0), Some(1)));
        assert_eq!(e.episode_title.as_deref(), Some("Making of"));
        let e = library_metadata(Category::Episode, "Shows/Andor.S01E02.mkv", 0);
        assert_eq!(e.show_name.as_deref(), Some("Andor"));

        let h = library_metadata(
            Category::HomeVideo,
            "Home Videos/2024/06/VID_20240613_101500.mp4",
            5,
        );
        assert_eq!(h.title, "VID_20240613_101500");
        assert_eq!(h.taken_at, Some(1_718_273_700));
        let p = library_metadata(Category::Photo, "Photos/beach.jpg", 5);
        assert_eq!(p.taken_at, Some(5));
    }

    #[test]
    fn indexable_rules() {
        let movie = Zone::Library(Category::Movie);
        assert!(indexable(
            movie,
            "Movies/Heat (1995)/Heat (1995).mkv",
            MediaKind::Video,
            1 << 30
        ));
        assert!(!indexable(
            movie,
            "Movies/Heat (1995)/poster.jpg",
            MediaKind::Photo,
            10
        ));
        assert!(!indexable(
            movie,
            "Movies/Heat (1995)/Heat (1995).srt",
            MediaKind::Other,
            10
        ));
        assert!(!indexable(
            movie,
            "Movies/Heat (1995)/extras/Interview.mkv",
            MediaKind::Video,
            1 << 30
        ));
        assert!(!indexable(
            movie,
            "Movies/Heat (1995)/heat-sample.mkv",
            MediaKind::Video,
            1 << 20
        ));
        // Extras folders count inside a title folder, not as the title itself
        // ("Extras" is also a TV show).
        assert!(indexable(
            Zone::Library(Category::Episode),
            "Shows/Extras/Season 01/Extras - S01E01.mkv",
            MediaKind::Video,
            1 << 30
        ));
        assert!(!indexable(
            Zone::Library(Category::Episode),
            "Shows/Andor/Season 01/Extras/Making of.mkv",
            MediaKind::Video,
            1 << 30
        ));
        assert!(indexable(
            Zone::Library(Category::Other),
            "Other/manual.pdf",
            MediaKind::Other,
            10
        ));
        assert!(!indexable(
            Zone::Custom,
            "Family/notes.txt",
            MediaKind::Other,
            10
        ));
        assert!(indexable(
            Zone::Custom,
            "Family/beach.mp4",
            MediaKind::Video,
            10
        ));
        assert!(!indexable(
            Zone::InProgress,
            "Downloads/In Progress/a.mkv",
            MediaKind::Video,
            10
        ));
    }
}
