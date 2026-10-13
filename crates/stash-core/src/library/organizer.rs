//! Moving files: automatic sorting, explicit organize, review, trash.

use std::path::{Path, PathBuf};
use std::time::Duration;

use walkdir::WalkDir;

use super::classify::{camera_from_tags, classify, media_kind};
use super::parse::parse_name;
use super::scanner::{
    Meta, enqueue_media_jobs, find_moved, is_extras_dir, is_junk_name, is_sample, mtime_secs,
    new_row, normalize_rel, overwrite_row, publish, quick_hash_async,
};
use super::{Library, OrganizeRequest, ReviewAction, repo};
use crate::events::{Event, LibraryChange};
use crate::jobs::JobKind;
use crate::media::ThumbSize;
use crate::media::exif::ExifInfo;
use crate::models::{
    Category, Classification, Confidence, DownloadRow, ItemRow, ItemStatus, MediaKind, MediaSummary,
};
use crate::paths::{self, StashLayout, Zone};
use crate::{CoreError, Result, now};

/// Longest we wait for ffprobe or EXIF while sorting before classifying by name.
const PROBE_TIMEOUT: Duration = Duration::from_secs(90);
const UNKNOWN_SHOW: &str = "Unknown Show";

/// Relative destination path for a classification and original file name.
pub fn destination_for(classification: &Classification, file_name: &str) -> String {
    let c = classification;
    let name = paths::sanitize_filename(file_name);
    let path = Path::new(&name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.clone());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
        .unwrap_or_default();
    match c.category {
        Category::Movie => {
            let title = if c.title.trim().is_empty() {
                stem.as_str()
            } else {
                c.title.as_str()
            };
            let label = match c.year {
                Some(year) => format!("{title} ({year})"),
                None => title.to_string(),
            };
            let folder = component(&label);
            format!("{}/{folder}/{folder}{ext}", paths::MOVIES)
        }
        Category::Episode => {
            let show = component(
                c.show_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(UNKNOWN_SHOW),
            );
            match (c.season, c.episode) {
                (season, Some(episode)) => {
                    let season = season.unwrap_or(1);
                    let parsed = parse_name(&stem);
                    let end = parsed
                        .episode_end
                        .filter(|_| {
                            parsed.season == Some(season) && parsed.episode == Some(episode)
                        })
                        .filter(|&end| end > episode);
                    let marker = match end {
                        Some(end) => format!("S{season:02}E{episode:02}-E{end:02}"),
                        None => format!("S{season:02}E{episode:02}"),
                    };
                    format!(
                        "{}/{show}/Season {season:02}/{show} - {marker}{ext}",
                        paths::SHOWS
                    )
                }
                (Some(season), None) => {
                    format!("{}/{show}/Season {season:02}/{name}", paths::SHOWS)
                }
                (None, None) => format!("{}/{show}/{name}", paths::SHOWS),
            }
        }
        Category::HomeVideo | Category::Photo => {
            let root = paths::category_folder(c.category);
            match c
                .taken_at
                .and_then(|t| time::OffsetDateTime::from_unix_timestamp(t).ok())
            {
                Some(dt) => format!("{root}/{:04}/{:02}/{name}", dt.year(), u8::from(dt.month())),
                None => format!("{root}/Undated/{name}"),
            }
        }
        Category::Other => format!("{}/{name}", paths::OTHER),
    }
}

/// One safe folder or file name: colons and slashes become dashes, then the usual
/// sanitizing; trailing dots and spaces (which SMB shares reject) are removed.
fn component(s: &str) -> String {
    let s = s.replace(": ", " - ").replace([':', '/', '\\'], "-");
    let cleaned = paths::sanitize_filename(&s);
    let cleaned = cleaned.trim_end_matches(['.', ' ']).to_string();
    if cleaned.is_empty() {
        "untitled".to_string()
    } else {
        cleaned
    }
}

// ---------------------------------------------------------------------------
// Sort queue

pub async fn enqueue_sort(lib: &Library, rel_path: &str, download_id: Option<&str>) -> Result<()> {
    enqueue_sort_inner(lib, rel_path, download_id)
        .await
        .map(|_| ())
}

/// Returns true when a new job was queued.
pub(crate) async fn enqueue_sort_inner(
    lib: &Library,
    rel_path: &str,
    download_id: Option<&str>,
) -> Result<bool> {
    let rel = normalize_rel(rel_path)?;
    if !StashLayout::zone(&rel).auto_sorts() {
        return Err(CoreError::invalid(
            "Only files in Inbox or Downloads/Completed are sorted automatically.",
        ));
    }
    let existing: Option<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT id, state, json_extract(payload, '$.download_id') FROM jobs
         WHERE kind = 'sort' AND state IN ('queued', 'running') AND json_extract(payload, '$.rel_path') = ?
         ORDER BY id LIMIT 1",
    )
    .bind(&rel)
    .fetch_optional(&lib.db)
    .await?;
    if let Some((job_id, state, existing_download)) = existing {
        // The watcher may have queued this path before the download manager linked it.
        if let (Some(download_id), None, "queued") =
            (download_id, existing_download, state.as_str())
        {
            sqlx::query(
                "UPDATE jobs SET payload = ?, updated_at = ? WHERE id = ? AND state = 'queued'",
            )
            .bind(serde_json::json!({"rel_path": rel, "download_id": download_id}).to_string())
            .bind(now())
            .bind(job_id)
            .execute(&lib.db)
            .await?;
        }
        return Ok(false);
    }
    lib.jobs
        .enqueue(
            JobKind::Sort,
            None,
            serde_json::json!({"rel_path": rel, "download_id": download_id}),
        )
        .await?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// Sorting

/// What happened to one file during a sort.
enum Sorted {
    /// Created or changed this item.
    Changed(String),
    /// Already an item and unchanged since it was last sorted.
    Unchanged(String),
    Skipped,
}

pub async fn sort_path(
    lib: &Library,
    rel_path: &str,
    download_id: Option<&str>,
) -> Result<Vec<String>> {
    let rel = normalize_rel(rel_path)?;
    if !StashLayout::zone(&rel).auto_sorts() {
        return Err(CoreError::invalid(
            "Only files in Inbox or Downloads/Completed are sorted automatically.",
        ));
    }
    let abs = lib.layout.abs(&rel)?;
    let download_id = match download_id {
        Some(id) => Some(id.to_string()),
        None => download_for_path(lib, &rel).await?,
    };
    let meta = match tokio::fs::symlink_metadata(&abs).await {
        Ok(meta) => meta,
        Err(e) if super::scanner::is_not_found(&e) => {
            tracing::info!(path = %rel, "nothing to sort: the path is gone");
            if let Some(id) = &download_id {
                download_files_gone(lib, id).await?;
            }
            return Ok(Vec::new());
        }
        Err(e) => return Err(e.into()),
    };
    if meta.file_type().is_symlink() {
        tracing::warn!(path = %rel, "not sorting a symbolic link");
        return Ok(Vec::new());
    }

    let review_before = repo::needs_review_count(&lib.db).await?;
    let completed = StashLayout::zone(&rel) == Zone::Completed;
    let files = {
        let (abs, layout) = (abs.clone(), lib.layout.clone());
        tokio::task::spawn_blocking(move || collect_sortable(&layout, &abs, completed))
            .await
            .map_err(|e| CoreError::Other(e.into()))?
    };
    let auto_sort = lib.settings.get().await.auto_sort;
    let mut changed = Vec::new();
    let mut linked = Vec::new();
    for file in files {
        match sort_file(lib, &file, download_id.as_deref(), auto_sort).await {
            Ok(Sorted::Changed(id)) => {
                linked.push(id.clone());
                changed.push(id);
            }
            Ok(Sorted::Unchanged(id)) => linked.push(id),
            Ok(Sorted::Skipped) => {}
            Err(CoreError::Db(e)) => return Err(CoreError::Db(e)),
            Err(e) => tracing::warn!(error = %e, file = %file.display(), "could not sort a file"),
        }
    }
    {
        let _moves = lib.lock_moves().await;
        remove_empty_dirs(&lib.layout, &abs).await;
    }
    if let Some(id) = &download_id {
        finish_download(lib, id, &linked).await?;
    }
    publish_inbox_if_changed(lib, review_before).await?;
    Ok(changed)
}

/// Media files under a sort root, skipping samples, extras folders (in Completed) and junk.
fn collect_sortable(layout: &StashLayout, abs: &Path, completed: bool) -> Vec<PathBuf> {
    let in_progress = layout.in_progress();
    WalkDir::new(abs)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !(is_junk_name(&name)
                || e.path().starts_with(&in_progress)
                || (completed && e.file_type().is_dir() && is_extras_dir(&name)))
        })
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let name = e.file_name().to_string_lossy();
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            // Samples are only skipped inside downloads; "Sample day.mp4" in the Inbox is a video.
            media_kind(e.path()) != MediaKind::Other
                && !(completed && is_sample(&name, size))
                && !is_junk_name(&name)
        })
        .map(|e| e.into_path())
        .collect()
}

/// ffprobe for videos and EXIF for photos. Failures (including a crash in the probe) are
/// logged and the file is classified by name only.
async fn analyze(
    lib: &Library,
    abs: &Path,
    kind: MediaKind,
) -> (Option<MediaSummary>, Option<ExifInfo>) {
    let media = lib.media.clone();
    let path = abs.to_path_buf();
    match kind {
        MediaKind::Video => {
            let task = tokio::spawn(async move { media.probe_file(&path).await });
            match tokio::time::timeout(PROBE_TIMEOUT, task).await {
                Ok(Ok(Ok(summary))) => (Some(summary), None),
                Ok(Ok(Err(e))) => {
                    tracing::warn!(error = %e, file = %abs.display(), "could not probe the file; sorting by name only");
                    (None, None)
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, file = %abs.display(), "the probe stopped unexpectedly; sorting by name only");
                    (None, None)
                }
                Err(_) => {
                    tracing::warn!(file = %abs.display(), "the probe took too long; sorting by name only");
                    (None, None)
                }
            }
        }
        MediaKind::Photo => {
            let task = tokio::spawn(async move { media.read_exif(&path).await });
            match tokio::time::timeout(PROBE_TIMEOUT, task).await {
                Ok(Ok(Ok(exif))) => (None, exif),
                Ok(Ok(Err(e))) => {
                    tracing::warn!(error = %e, file = %abs.display(), "could not read EXIF; sorting by name only");
                    (None, None)
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, file = %abs.display(), "EXIF reading stopped unexpectedly");
                    (None, None)
                }
                Err(_) => {
                    tracing::warn!(file = %abs.display(), "EXIF reading took too long; sorting by name only");
                    (None, None)
                }
            }
        }
        MediaKind::Audio | MediaKind::Other => (None, None),
    }
}

async fn sort_file(
    lib: &Library,
    abs: &Path,
    download_id: Option<&str>,
    auto_sort: bool,
) -> Result<Sorted> {
    let Some(rel) = lib.layout.rel(abs) else {
        return Ok(Sorted::Skipped);
    };
    if !StashLayout::zone(&rel).auto_sorts() {
        return Ok(Sorted::Skipped);
    }
    let meta = tokio::fs::metadata(abs).await?;
    let size = i64::try_from(meta.len()).unwrap_or(i64::MAX);
    let mtime = mtime_secs(&meta);
    let existing = repo::get_by_path(&lib.db, &rel).await?;
    if let Some(item) = &existing {
        // Unchanged files are not looked at again.
        if item.size == size && item.mtime == mtime && item.status != ItemStatus::Missing {
            return Ok(Sorted::Unchanged(item.id.clone()));
        }
        // The owner reviewed this one and chose to leave it here: refresh it in place.
        if item.status == ItemStatus::Ready {
            super::scanner::index_inner(lib, &rel).await?;
            return Ok(Sorted::Unchanged(item.id.clone()));
        }
    }

    let kind = media_kind(abs);
    let (probe, exif) = analyze(lib, abs, kind).await;
    let mut c = classify(&rel, probe.as_ref(), exif.as_ref(), mtime);
    if c.category == Category::Episode {
        canonicalize_show(lib, &mut c, &file_name(abs)).await?;
    }
    let camera = exif
        .as_ref()
        .and_then(|e| e.camera.clone())
        .or_else(|| probe.as_ref().and_then(camera_from_tags));
    let hash = quick_hash_async(abs).await;

    let mut row = new_row(
        &rel,
        c.category,
        ItemStatus::NeedsReview,
        Meta::from_classification(&c),
        size,
        mtime,
        hash,
    );
    row.source_download_id = download_id.map(str::to_string);
    row.camera = camera;
    fill_media(&mut row, probe.as_ref(), exif.as_ref());

    // Probing took a while without the lock. Only act if neither the file nor its row
    // changed meanwhile (an organize, review, trash or a new copy of the file).
    let _moves = lib.lock_moves().await;
    let current = repo::get_by_path(&lib.db, &rel).await?;
    let same_row = match (&existing, &current) {
        (None, None) => true,
        (Some(a), Some(b)) => a.id == b.id && a.status == b.status && a.updated_at == b.updated_at,
        _ => false,
    };
    let same_file = tokio::fs::symlink_metadata(abs).await.is_ok_and(|m| {
        m.is_file() && i64::try_from(m.len()).unwrap_or(i64::MAX) == size && mtime_secs(&m) == mtime
    });
    if !same_row || !same_file {
        tracing::info!(path = %rel, "the file changed while it was being sorted; leaving it for the next pass");
        return Ok(Sorted::Skipped);
    }
    // The same bytes as a missing item: the file was moved here, so that item (and the
    // path it held) is reused and its favorites and progress survive.
    let target = match existing {
        Some(item) => Some(item),
        None => find_moved(lib, size, row.quick_hash.as_deref()).await?,
    };
    let own_id = target.as_ref().map(|t| t.id.clone());

    if c.confidence == Confidence::High && auto_sort {
        match move_into_library(lib, abs, &c.destination, own_id.as_deref()).await {
            Ok(new_rel) => {
                row.rel_path = new_rel;
                row.status = ItemStatus::Ready;
                row.suggestion_json = None;
                let id = record(lib, target, row).await?;
                tracing::info!(item = %id, from = %rel, reason = %c.reason, "sorted a file");
                return Ok(Sorted::Changed(id));
            }
            Err(e) => {
                tracing::warn!(error = %e, file = %rel, "could not move the file into the library; leaving it for review");
            }
        }
    }
    row.suggestion_json = Some(serde_json::to_string(&c)?);
    let id = record(lib, target, row).await?;
    Ok(Sorted::Changed(id))
}

/// Duration, size and the stored probe summary from what the sorter already read.
fn fill_media(row: &mut ItemRow, probe: Option<&MediaSummary>, exif: Option<&ExifInfo>) {
    if let Some(p) = probe {
        row.duration_s = p.duration_s;
        if let Some(v) = &p.video {
            let rotated = matches!(v.rotation.rem_euclid(360), 90 | 270);
            (row.width, row.height) = if rotated {
                (Some(v.height), Some(v.width))
            } else {
                (Some(v.width), Some(v.height))
            };
        }
        row.media_json = serde_json::to_string(p).ok();
    }
    if let Some(e) = exif
        && e.width.is_some()
    {
        // As displayed: a portrait phone photo is stored landscape with an orientation tag.
        (row.width, row.height) = e.display_size();
    }
}

/// Write a sorted file's row: update `target` (the item at the old path, or a missing item
/// with the same bytes), or insert a new one. Queues probe and thumbnail work. The caller
/// holds the move lock.
async fn record(lib: &Library, target: Option<ItemRow>, row: ItemRow) -> Result<String> {
    let (id, change) = match target {
        Some(item) => {
            overwrite_row(lib, &item.id, &row).await?;
            (item.id, LibraryChange::Updated)
        }
        None => {
            if repo::insert_item(&lib.db, &row).await? {
                (row.id.clone(), LibraryChange::Added)
            } else {
                // A row for this path appeared meanwhile (watcher or rescan): take it over.
                let current = repo::get_by_path(&lib.db, &row.rel_path)
                    .await?
                    .ok_or(CoreError::NotFound)?;
                overwrite_row(lib, &current.id, &row).await?;
                (current.id, LibraryChange::Updated)
            }
        }
    };
    enqueue_media_jobs(lib, &id).await?;
    publish(lib, change, &id);
    Ok(id)
}

/// Reuse the spelling and folder of a show that is already in the library.
async fn canonicalize_show(lib: &Library, c: &mut Classification, file_name: &str) -> Result<()> {
    let Some(show) = c.show_name.clone().filter(|s| !s.trim().is_empty()) else {
        return Ok(());
    };
    let existing: Option<(String, String)> = sqlx::query_as(
        "SELECT show_name, rel_path FROM items
         WHERE category = 'episode' AND status IN ('ready', 'missing') AND show_name = ? COLLATE NOCASE
         ORDER BY added_at, id LIMIT 1",
    )
    .bind(&show)
    .fetch_optional(&lib.db)
    .await?;
    let Some((name, rel_path)) = existing else {
        return Ok(());
    };
    c.show_name = Some(name.clone());
    if let Some((s, e)) = c.season.zip(c.episode) {
        c.title = format!("{name} S{s:02}E{e:02}");
    }
    c.destination = destination_for(c, file_name);
    // Keep using the show's existing folder under Shows/ (it may be named differently).
    let mut parts = rel_path.split('/');
    if let (Some(paths::SHOWS), Some(folder), Some(_)) = (parts.next(), parts.next(), parts.next())
    {
        let ours = component(&name);
        let prefix = format!("{}/{ours}/", paths::SHOWS);
        if folder != ours
            && let Some(rest) = c.destination.strip_prefix(&prefix)
        {
            c.destination = format!("{}/{folder}/{rest}", paths::SHOWS);
        }
    }
    Ok(())
}

/// `Movies/X/X (2).mkv` is already where `Movies/X/X.mkv` would land after a name collision.
fn is_numbered_variant(current: &str, dest: &str) -> bool {
    let (Some((cur_dir, cur_name)), Some((dest_dir, dest_name))) =
        (current.rsplit_once('/'), dest.rsplit_once('/'))
    else {
        return false;
    };
    let split = |name: &str| match name.rsplit_once('.') {
        Some((stem, ext)) => (stem.to_string(), format!(".{ext}")),
        None => (name.to_string(), String::new()),
    };
    let ((cur_stem, cur_ext), (dest_stem, dest_ext)) = (split(cur_name), split(dest_name));
    cur_dir == dest_dir
        && cur_ext == dest_ext
        && cur_stem
            .strip_prefix(&dest_stem)
            .and_then(|rest| rest.strip_prefix(" ("))
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

fn file_name(abs: &Path) -> String {
    abs.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `dest`, or `dest (2).ext`, `dest (3).ext`…: the first name that is free on disk and not
/// claimed by another item row (a missing item keeps its path until it is forgotten). A
/// path held by `own_id` (the item being moved) counts as free.
async fn free_destination(lib: &Library, dest_abs: &Path, own_id: Option<&str>) -> Result<PathBuf> {
    let parent = dest_abs.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = dest_abs
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = dest_abs
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 1..10_000 {
        let candidate = if n == 1 {
            dest_abs.to_path_buf()
        } else {
            parent.join(format!("{stem} ({n}){ext}"))
        };
        if tokio::fs::symlink_metadata(&candidate).await.is_ok() {
            continue;
        }
        let rel = lib
            .layout
            .rel(&candidate)
            .ok_or_else(|| CoreError::invalid("That destination is outside the stash."))?;
        match repo::get_by_path(&lib.db, &rel).await? {
            None => return Ok(candidate),
            Some(holder) if Some(holder.id.as_str()) == own_id => return Ok(candidate),
            Some(_) => {}
        }
    }
    Err(CoreError::conflict(
        "Could not find a free file name at the destination.",
    ))
}

/// Move a file (and its sidecars) to `dest_rel` or a free name next to it. Returns the new
/// rel path. `own_id` is the item being moved (its own old path counts as free).
async fn move_into_library(
    lib: &Library,
    src: &Path,
    dest_rel: &str,
    own_id: Option<&str>,
) -> Result<String> {
    let dest_rel = normalize_rel(dest_rel)?;
    if !matches!(StashLayout::zone(&dest_rel), Zone::Library(_)) {
        return Err(CoreError::invalid(
            "Files can only be organized into the library folders.",
        ));
    }
    let dest_abs = lib.layout.abs(&dest_rel)?;
    let mut last_err = None;
    for _ in 0..3 {
        let target = free_destination(lib, &dest_abs, own_id).await?;
        match paths::move_path(src, &target).await {
            Ok(()) => {
                move_sidecars(src, &target).await;
                return lib
                    .layout
                    .rel(&target)
                    .ok_or_else(|| CoreError::invalid("That destination is outside the stash."));
            }
            // Lost a race for the name: try the next one.
            Err(CoreError::Conflict(msg)) => last_err = Some(CoreError::Conflict(msg)),
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| CoreError::conflict("Could not move the file.")))
}

/// How strongly the media file `media_name` claims the sidecar `file`: the length of the
/// shared prefix, where `name.ext.` (e.g. `IMG_1.HEIC.xmp`) beats `stem.` (`IMG_1.xmp`).
fn sidecar_claim(file: &str, media_name: &str) -> Option<usize> {
    if file.len() > media_name.len()
        && file.starts_with(media_name)
        && file[media_name.len()..].starts_with('.')
    {
        return Some(media_name.len() + 1);
    }
    let stem = Path::new(media_name).file_stem()?.to_string_lossy();
    (file.len() > stem.len() + 1
        && file.starts_with(stem.as_ref())
        && file[stem.len()..].starts_with('.'))
    .then_some(stem.len() + 1)
}

/// Sidecar files next to `src` that belong to it: a sidecar extension, a name starting
/// with `src`'s stem (or full name) and a dot, and no other media file next to it with a
/// longer claim ("Heat.1995.1080p.srt" belongs to "Heat.1995.1080p.mkv", not to
/// "Heat.1995.mkv"). Returns (path, claimed prefix length in bytes).
async fn sidecars_of(src: &Path) -> Vec<(PathBuf, usize)> {
    let (Some(dir), Some(name)) = (src.parent(), src.file_name()) else {
        return Vec::new();
    };
    let name = name.to_string_lossy().into_owned();
    let Ok(mut read) = tokio::fs::read_dir(dir).await else {
        return Vec::new();
    };
    let (mut sidecars, mut media) = (Vec::new(), Vec::new());
    while let Ok(Some(entry)) = read.next_entry().await {
        let entry_name = entry.file_name().to_string_lossy().into_owned();
        if entry_name == name || is_junk_name(&entry_name) {
            continue;
        }
        let is_file = entry
            .file_type()
            .await
            .map(|t| t.is_file())
            .unwrap_or(false);
        if !is_file {
            continue;
        }
        if super::scanner::is_sidecar(&entry.path()) {
            sidecars.push((entry.path(), entry_name));
        } else if media_kind(&entry.path()) != MediaKind::Other {
            media.push(entry_name);
        }
    }
    sidecars
        .into_iter()
        .filter_map(|(path, file)| {
            let ours = sidecar_claim(&file, &name)?;
            let theirs = media
                .iter()
                .filter_map(|m| sidecar_claim(&file, m))
                .max()
                .unwrap_or(0);
            (ours >= theirs).then_some((path, ours))
        })
        .collect()
}

/// Move `src`'s sidecars to sit next to `dest`, renamed to match it
/// ("Heat.1995.en.srt" → "Heat (1995).en.srt", "IMG_1.HEIC.xmp" → "IMG_1 (2).HEIC.xmp").
async fn move_sidecars(src: &Path, dest: &Path) {
    let (Some(dest_dir), Some(src_name), Some(dest_name), Some(dest_stem)) = (
        dest.parent(),
        src.file_name(),
        dest.file_name(),
        dest.file_stem(),
    ) else {
        return;
    };
    let src_name = src_name.to_string_lossy().into_owned();
    let dest_name = dest_name.to_string_lossy().into_owned();
    let dest_stem = dest_stem.to_string_lossy().into_owned();
    for (path, claimed) in sidecars_of(src).await {
        let Some(file) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        // `claimed` covers either the full source name plus a dot, or its stem plus a dot.
        let base = if claimed == src_name.len() + 1 {
            &dest_name
        } else {
            &dest_stem
        };
        let rest = file.get(claimed - 1..).unwrap_or_default();
        let target = paths::unique_path(&dest_dir.join(format!("{base}{rest}")));
        if let Err(e) = paths::move_path(&path, &target).await {
            tracing::warn!(error = %e, file = %path.display(), "could not move a sidecar file");
        }
    }
}

/// Remove folders left empty under `start` and its parents, inside Inbox/ or the
/// Downloads area only, never the zone roots themselves.
async fn remove_empty_dirs(layout: &StashLayout, start: &Path) {
    let layout = layout.clone();
    let start = start.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        let removable = |p: &Path| -> bool {
            let Some(rel) = layout.rel(p) else {
                return false;
            };
            let protected = [
                paths::INBOX,
                paths::DOWNLOADS,
                paths::COMPLETED,
                paths::IN_PROGRESS,
            ];
            StashLayout::zone(&rel).auto_sorts() && !protected.contains(&rel.as_str())
        };
        if start.is_dir() {
            for entry in WalkDir::new(&start)
                .follow_links(false)
                .contents_first(true)
                .into_iter()
                .flatten()
            {
                if entry.file_type().is_dir() && removable(entry.path()) {
                    let _ = std::fs::remove_dir(entry.path());
                }
            }
        }
        let mut current = start.parent().map(Path::to_path_buf);
        while let Some(dir) = current {
            if !removable(&dir) || std::fs::remove_dir(&dir).is_err() {
                break;
            }
            current = dir.parent().map(Path::to_path_buf);
        }
    })
    .await;
}

// ---------------------------------------------------------------------------
// Downloads

/// A finished download waiting for this path, when the caller did not name one.
async fn download_for_path(lib: &Library, rel: &str) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM downloads WHERE rel_path = ? AND state IN ('completed', 'sorting')
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(rel)
    .fetch_optional(&lib.db)
    .await?)
}

async fn download_row(lib: &Library, id: &str) -> Result<Option<DownloadRow>> {
    Ok(
        sqlx::query_as::<_, DownloadRow>("SELECT * FROM downloads WHERE id = ?")
            .bind(id)
            .fetch_optional(&lib.db)
            .await?,
    )
}

async fn publish_download(lib: &Library, id: &str) -> Result<()> {
    if let Some(row) = download_row(lib, id).await? {
        let item_ids: Vec<String> = sqlx::query_scalar(
            "SELECT item_id FROM download_items WHERE download_id = ? ORDER BY rowid",
        )
        .bind(id)
        .fetch_all(&lib.db)
        .await?;
        lib.events
            .publish(Event::Download(row.to_download(item_ids)));
    }
    Ok(())
}

/// Link the download to its items and mark it sorted.
async fn finish_download(lib: &Library, id: &str, item_ids: &[String]) -> Result<()> {
    if download_row(lib, id).await?.is_none() {
        tracing::warn!(download = %id, "sorted files for a download that no longer exists");
        return Ok(());
    }
    let t = now();
    let mut tx = lib.db.begin().await?;
    for item_id in item_ids {
        sqlx::query("INSERT OR IGNORE INTO download_items (download_id, item_id) VALUES (?, ?)")
            .bind(id)
            .bind(item_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE items SET source_download_id = ? WHERE id = ? AND source_download_id IS NULL",
        )
        .bind(id)
        .bind(item_id)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE downloads SET state = 'sorted', error = NULL, sorted_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(t)
    .bind(t)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    publish_download(lib, id).await
}

/// The download's files vanished before the sorter got to them.
async fn download_files_gone(lib: &Library, id: &str) -> Result<()> {
    let Some(row) = download_row(lib, id).await? else {
        return Ok(());
    };
    if !matches!(
        row.state,
        crate::models::DownloadState::Completed | crate::models::DownloadState::Sorting
    ) {
        return Ok(());
    }
    let linked: i64 =
        sqlx::query_scalar("SELECT count(*) FROM download_items WHERE download_id = ?")
            .bind(id)
            .fetch_one(&lib.db)
            .await?;
    let t = now();
    if linked > 0 {
        sqlx::query(
            "UPDATE downloads SET state = 'sorted', sorted_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(t)
        .bind(t)
        .bind(id)
        .execute(&lib.db)
        .await?;
    } else {
        sqlx::query(
            "UPDATE downloads SET state = 'failed', error = ?, updated_at = ? WHERE id = ?",
        )
        .bind("The finished files were not found in Downloads/Completed, so nothing was sorted.")
        .bind(t)
        .bind(id)
        .execute(&lib.db)
        .await?;
    }
    publish_download(lib, id).await
}

pub(crate) async fn publish_inbox_if_changed(lib: &Library, before: i64) -> Result<()> {
    let after = repo::needs_review_count(&lib.db).await?;
    if after != before {
        lib.events.publish(Event::Inbox { count: after });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Explicit actions

pub async fn organize(lib: &Library, item_id: &str, request: OrganizeRequest) -> Result<ItemRow> {
    let _moves = lib.lock_moves().await;
    let item = repo::get(&lib.db, item_id)
        .await?
        .ok_or(CoreError::NotFound)?;
    organize_item(lib, item, request, false).await
}

/// Build the target classification. Explicit request fields win; the rest come from the
/// item's metadata then its suggestion (or the other way round when accepting a review).
fn organize_target(
    item: &ItemRow,
    request: &OrganizeRequest,
    prefer_suggestion: bool,
) -> Classification {
    fn choose<T>(
        prefer_suggestion: bool,
        request: Option<T>,
        item: Option<T>,
        suggestion: Option<T>,
    ) -> Option<T> {
        if prefer_suggestion {
            request.or(suggestion).or(item)
        } else {
            request.or(item).or(suggestion)
        }
    }
    let suggestion = item.suggestion();
    let s = suggestion.as_ref();
    let non_empty = |t: &str| Some(t.trim().to_string()).filter(|t| !t.is_empty());
    let category = request.category.unwrap_or(match (prefer_suggestion, s) {
        (true, Some(s)) => s.category,
        _ => item.category,
    });
    let title = choose(
        prefer_suggestion,
        request.title.as_deref().and_then(non_empty),
        non_empty(&item.title),
        s.and_then(|s| non_empty(&s.title)),
    )
    .unwrap_or_default();
    Classification {
        category,
        title,
        year: choose(
            prefer_suggestion,
            request.year,
            item.year,
            s.and_then(|s| s.year),
        ),
        show_name: choose(
            prefer_suggestion,
            request.show_name.as_deref().and_then(non_empty),
            item.show_name.as_deref().and_then(non_empty),
            s.and_then(|s| s.show_name.as_deref().and_then(non_empty)),
        ),
        season: choose(
            prefer_suggestion,
            request.season,
            item.season,
            s.and_then(|s| s.season),
        ),
        episode: choose(
            prefer_suggestion,
            request.episode,
            item.episode,
            s.and_then(|s| s.episode),
        ),
        episode_title: item
            .episode_title
            .clone()
            .or_else(|| s.and_then(|s| s.episode_title.clone())),
        taken_at: item
            .taken_at
            .or_else(|| s.and_then(|s| s.taken_at))
            .or(Some(item.mtime)),
        confidence: Confidence::High,
        reason: "Organized by hand".to_string(),
        destination: String::new(),
    }
}

/// Longest title or show name accepted by organize and review (as for metadata edits).
const MAX_NAME_CHARS: usize = 500;

/// The caller holds the move lock.
async fn organize_item(
    lib: &Library,
    item: ItemRow,
    request: OrganizeRequest,
    prefer_suggestion: bool,
) -> Result<ItemRow> {
    let too_long = |s: &Option<String>| {
        s.as_deref()
            .is_some_and(|s| s.trim().chars().count() > MAX_NAME_CHARS)
    };
    if too_long(&request.title) || too_long(&request.show_name) {
        return Err(CoreError::invalid(format!(
            "Titles and show names must be {MAX_NAME_CHARS} characters or fewer."
        )));
    }
    match item.status {
        ItemStatus::Trashed => {
            return Err(CoreError::invalid(
                "This item is in the trash. Restore it first.",
            ));
        }
        ItemStatus::Missing => {
            return Err(CoreError::invalid(
                "The file for this item is missing, so it cannot be moved.",
            ));
        }
        ItemStatus::Ready | ItemStatus::NeedsReview => {}
    }
    let zone = StashLayout::zone(&item.rel_path);
    if matches!(zone, Zone::Internal | Zone::InProgress) {
        return Err(CoreError::invalid(
            "Files that are still downloading or in the trash cannot be organized.",
        ));
    }
    let mut target = organize_target(&item, &request, prefer_suggestion);
    if target
        .year
        .is_some_and(|year| !(1800..=2200).contains(&year))
    {
        return Err(CoreError::invalid(
            "The year must be between 1800 and 2200.",
        ));
    }
    if target.season.is_some_and(|s| s < 0) || target.episode.is_some_and(|e| e < 0) {
        return Err(CoreError::invalid(
            "Season and episode numbers cannot be negative.",
        ));
    }
    match target.category {
        Category::Movie if target.title.trim().is_empty() => {
            return Err(CoreError::invalid("Add a title to file this as a movie."));
        }
        Category::Episode
            if target
                .show_name
                .as_deref()
                .is_none_or(|s| s.trim().is_empty()) =>
        {
            return Err(CoreError::invalid(
                "Add the show name to file this as an episode.",
            ));
        }
        _ => {}
    }
    if target.category == Category::Episode
        && let (Some(show), Some(s), Some(e)) = (&target.show_name, target.season, target.episode)
    {
        target.title = format!("{show} S{s:02}E{e:02}");
    }

    let abs = lib.layout.abs(&item.rel_path)?;
    if tokio::fs::symlink_metadata(&abs).await.is_err() {
        return Err(CoreError::invalid(
            "The file is no longer where PopcornStash expected it. Try a rescan.",
        ));
    }
    let name = file_name(&abs);
    target.destination = destination_for(&target, &name);
    if target.category == Category::Episode {
        canonicalize_show(lib, &mut target, &name).await?;
    }

    let review_before = repo::needs_review_count(&lib.db).await?;
    let new_rel = if target.destination == item.rel_path
        || is_numbered_variant(&item.rel_path, &target.destination)
    {
        item.rel_path.clone()
    } else {
        move_into_library(lib, &abs, &target.destination, Some(&item.id)).await?
    };
    sqlx::query(
        "UPDATE items SET rel_path = ?, category = ?, title = ?, sort_title = ?, year = ?, show_name = ?, season = ?,
            episode = ?, episode_title = ?, taken_at = ?, status = 'ready', suggestion_json = NULL, missing_since = NULL,
            updated_at = ?
         WHERE id = ?",
    )
    .bind(&new_rel)
    .bind(target.category.as_str())
    .bind(&target.title)
    .bind(repo::sort_title(&target.title))
    .bind(target.year)
    .bind(&target.show_name)
    .bind(target.season)
    .bind(target.episode)
    .bind(&target.episode_title)
    .bind(match target.category {
        Category::HomeVideo | Category::Photo => target.taken_at,
        _ => item.taken_at,
    })
    .bind(now())
    .bind(&item.id)
    .execute(&lib.db)
    .await?;
    if zone.auto_sorts()
        && let Some(parent) = abs.parent()
    {
        remove_empty_dirs(&lib.layout, parent).await;
    }
    tracing::info!(item = %item.id, from = %item.rel_path, to = %new_rel, "organized an item");
    publish(lib, LibraryChange::Updated, &item.id);
    publish_inbox_if_changed(lib, review_before).await?;
    repo::get(&lib.db, &item.id)
        .await?
        .ok_or(CoreError::NotFound)
}

pub async fn review(lib: &Library, item_id: &str, action: ReviewAction) -> Result<ItemRow> {
    let _moves = lib.lock_moves().await;
    let item = repo::get(&lib.db, item_id)
        .await?
        .ok_or(CoreError::NotFound)?;
    if item.status != ItemStatus::NeedsReview {
        return Err(CoreError::conflict("This item is not waiting for review."));
    }
    match action {
        ReviewAction::Accept(request) => organize_item(lib, item, request, true).await,
        ReviewAction::Ignore => {
            let review_before = repo::needs_review_count(&lib.db).await?;
            let category = item.suggestion().map_or(item.category, |s| s.category);
            sqlx::query(
                "UPDATE items SET status = 'ready', category = ?, updated_at = ? WHERE id = ?",
            )
            .bind(category.as_str())
            .bind(now())
            .bind(&item.id)
            .execute(&lib.db)
            .await?;
            publish(lib, LibraryChange::Updated, &item.id);
            publish_inbox_if_changed(lib, review_before).await?;
            repo::get(&lib.db, &item.id)
                .await?
                .ok_or(CoreError::NotFound)
        }
    }
}

// ---------------------------------------------------------------------------
// Trash

fn trash_dir_for(lib: &Library, item_id: &str) -> PathBuf {
    lib.layout.trash().join(paths::sanitize_filename(item_id))
}

pub async fn trash_item(lib: &Library, item_id: &str, user_id: Option<&str>) -> Result<()> {
    let _moves = lib.lock_moves().await;
    let item = repo::get(&lib.db, item_id)
        .await?
        .ok_or(CoreError::NotFound)?;
    if item.status == ItemStatus::Trashed {
        return Err(CoreError::conflict("This item is already in the trash."));
    }
    let zone = StashLayout::zone(&item.rel_path);
    if zone == Zone::Internal {
        return Err(CoreError::invalid(
            "This file cannot be moved to the trash.",
        ));
    }
    let review_before = repo::needs_review_count(&lib.db).await?;
    let src = lib.layout.abs(&item.rel_path)?;
    let name = Path::new(&item.rel_path)
        .file_name()
        .map(|n| paths::sanitize_filename(&n.to_string_lossy()))
        .unwrap_or_else(|| "file".to_string());
    let dest = trash_dir_for(lib, &item.id).join(name);
    if tokio::fs::symlink_metadata(&src).await.is_ok() {
        let dest_free = paths::unique_path(&dest);
        paths::move_path(&src, &dest_free).await?;
        move_sidecars(&src, &dest_free).await;
        record_trash(lib, &item, &dest_free, user_id).await?;
        if zone.auto_sorts()
            && let Some(parent) = src.parent()
        {
            remove_empty_dirs(&lib.layout, parent).await;
        }
    } else {
        // Nothing on disk to move (the file was already missing).
        record_trash(lib, &item, &dest, user_id).await?;
    }
    tracing::info!(item = %item.id, path = %item.rel_path, "moved an item to the trash");
    publish(lib, LibraryChange::Removed, &item.id);
    publish_inbox_if_changed(lib, review_before).await
}

async fn record_trash(
    lib: &Library,
    item: &ItemRow,
    trashed_abs: &Path,
    user_id: Option<&str>,
) -> Result<()> {
    let trashed_rel = lib
        .layout
        .rel(trashed_abs)
        .ok_or_else(|| CoreError::invalid("The trash folder is outside the stash."))?;
    let t = now();
    let mut tx = lib.db.begin().await?;
    sqlx::query(
        "INSERT INTO trash (item_id, original_rel_path, trashed_rel_path, trashed_at, trashed_by) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(item_id) DO UPDATE SET original_rel_path = excluded.original_rel_path,
           trashed_rel_path = excluded.trashed_rel_path, trashed_at = excluded.trashed_at, trashed_by = excluded.trashed_by",
    )
    .bind(&item.id)
    .bind(&item.rel_path)
    .bind(&trashed_rel)
    .bind(t)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE items SET status = 'trashed', rel_path = ?, updated_at = ? WHERE id = ?")
        .bind(&trashed_rel)
        .bind(t)
        .bind(&item.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn restore_item(lib: &Library, item_id: &str) -> Result<ItemRow> {
    let _moves = lib.lock_moves().await;
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT original_rel_path, trashed_rel_path FROM trash WHERE item_id = ?")
            .bind(item_id)
            .fetch_optional(&lib.db)
            .await?;
    let Some((original_rel, trashed_rel)) = row else {
        return Err(CoreError::NotFound);
    };
    let item = repo::get(&lib.db, item_id)
        .await?
        .ok_or(CoreError::NotFound)?;
    let src = lib.layout.abs(&trashed_rel)?;
    if tokio::fs::symlink_metadata(&src).await.is_err() {
        return Err(CoreError::conflict(
            "The file is no longer in the trash, so it cannot be restored.",
        ));
    }
    let review_before = repo::needs_review_count(&lib.db).await?;
    let original_abs = lib.layout.abs(&original_rel)?;
    let dest = free_destination(lib, &original_abs, Some(item_id)).await?;
    paths::move_path(&src, &dest).await?;
    move_sidecars(&src, &dest).await;
    if let Some(dir) = src.parent()
        && dir.starts_with(lib.layout.trash())
        && dir != lib.layout.trash()
    {
        let _ = tokio::fs::remove_dir(dir).await;
    }
    let new_rel = lib
        .layout
        .rel(&dest)
        .ok_or_else(|| CoreError::invalid("That destination is outside the stash."))?;
    let status = if StashLayout::zone(&new_rel).auto_sorts() && item.suggestion_json.is_some() {
        ItemStatus::NeedsReview
    } else {
        ItemStatus::Ready
    };
    let mut tx = lib.db.begin().await?;
    sqlx::query("UPDATE items SET rel_path = ?, status = ?, missing_since = NULL, updated_at = ? WHERE id = ?")
        .bind(&new_rel)
        .bind(status.as_str())
        .bind(now())
        .bind(item_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM trash WHERE item_id = ?")
        .bind(item_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    tracing::info!(item = %item_id, path = %new_rel, "restored an item from the trash");
    publish(lib, LibraryChange::Added, item_id);
    publish_inbox_if_changed(lib, review_before).await?;
    repo::get(&lib.db, item_id)
        .await?
        .ok_or(CoreError::NotFound)
}

pub async fn empty_trash(lib: &Library, older_than_days: Option<u32>) -> Result<u64> {
    let cutoff = older_than_days.map(|days| now() - i64::from(days) * 86_400);
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT item_id, trashed_rel_path FROM trash WHERE ? IS NULL OR trashed_at <= ? ORDER BY trashed_at")
            .bind(cutoff)
            .bind(cutoff)
            .fetch_all(&lib.db)
            .await?;
    let trash_root = lib.layout.trash();
    let mut deleted = 0;
    for (item_id, trashed_rel) in rows {
        let _moves = lib.lock_moves().await;
        // Restored (or deleted) since the list was read: nothing to do.
        let still_trashed: Option<String> =
            sqlx::query_scalar("SELECT trashed_rel_path FROM trash WHERE item_id = ?")
                .bind(&item_id)
                .fetch_optional(&lib.db)
                .await?;
        if still_trashed.as_deref() != Some(trashed_rel.as_str()) {
            continue;
        }
        let dir = trash_dir_for(lib, &item_id);
        let mut ok = match tokio::fs::remove_dir_all(&dir).await {
            Ok(()) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(e) => {
                tracing::warn!(error = %e, item = %item_id, "could not delete a trashed file");
                false
            }
        };
        // Files trashed by an older layout may sit elsewhere inside the trash folder.
        if ok
            && let Ok(path) = lib.layout.abs(&trashed_rel)
            && path.starts_with(&trash_root)
            && path != trash_root
            && !path.starts_with(&dir)
        {
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    tracing::warn!(error = %e, item = %item_id, "could not delete a trashed file");
                    ok = false;
                }
            }
        }
        if !ok {
            continue;
        }
        sqlx::query("DELETE FROM items WHERE id = ?")
            .bind(&item_id)
            .execute(&lib.db)
            .await?;
        remove_cached_media(lib, &item_id).await;
        deleted += 1;
    }
    if deleted > 0 {
        tracing::info!(count = deleted, "emptied the trash");
    }
    Ok(deleted)
}

/// Delete an item's thumbnails and photo rendition (best effort).
pub(crate) async fn remove_cached_media(lib: &Library, item_id: &str) {
    for path in [
        lib.media.thumb_path(item_id, ThumbSize::Sm),
        lib.media.thumb_path(item_id, ThumbSize::Lg),
        lib.media.rendition_path(item_id),
    ] {
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::debug!(error = %e, file = %path.display(), "could not remove a cached image")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(category: Category) -> Classification {
        Classification {
            category,
            title: String::new(),
            year: None,
            show_name: None,
            season: None,
            episode: None,
            episode_title: None,
            taken_at: None,
            confidence: Confidence::High,
            reason: String::new(),
            destination: String::new(),
        }
    }

    #[test]
    fn destinations() {
        let mut c = class(Category::Movie);
        c.title = "Heat".into();
        c.year = Some(1995);
        assert_eq!(
            destination_for(&c, "heat.1995.1080p.MKV"),
            "Movies/Heat (1995)/Heat (1995).mkv"
        );
        c.title = "Star Wars: Episode IV".into();
        c.year = Some(1977);
        assert_eq!(
            destination_for(&c, "x.mp4"),
            "Movies/Star Wars - Episode IV (1977)/Star Wars - Episode IV (1977).mp4"
        );
        c.title = "AC/DC Live...".into();
        c.year = None;
        assert_eq!(
            destination_for(&c, "x.mp4"),
            "Movies/AC-DC Live/AC-DC Live.mp4"
        );
        c.title = String::new();
        assert_eq!(
            destination_for(&c, "Fallback.mkv"),
            "Movies/Fallback/Fallback.mkv"
        );
        c.title = "../../etc".into();
        let escaped = destination_for(&c, "x.mkv");
        assert_eq!(escaped.split('/').count(), 3, "{escaped}");
        assert!(
            escaped.starts_with("Movies/") && escaped.split('/').all(|p| p != ".." && p != "."),
            "{escaped}"
        );

        let mut e = class(Category::Episode);
        e.show_name = Some("The Office US".into());
        e.season = Some(2);
        e.episode = Some(3);
        assert_eq!(
            destination_for(&e, "the.office.s02e03.mkv"),
            "Shows/The Office US/Season 02/The Office US - S02E03.mkv"
        );
        assert_eq!(
            destination_for(&e, "Show.S02E03E04.mkv"),
            "Shows/The Office US/Season 02/The Office US - S02E03-E04.mkv"
        );
        e.season = None;
        assert_eq!(
            destination_for(&e, "x.mkv"),
            "Shows/The Office US/Season 01/The Office US - S01E03.mkv"
        );
        e.episode = None;
        assert_eq!(destination_for(&e, "x.mkv"), "Shows/The Office US/x.mkv");
        e.season = Some(4);
        assert_eq!(
            destination_for(&e, "x.mkv"),
            "Shows/The Office US/Season 04/x.mkv"
        );
        e.show_name = None;
        assert_eq!(
            destination_for(&e, "x.mkv"),
            "Shows/Unknown Show/Season 04/x.mkv"
        );

        let mut h = class(Category::HomeVideo);
        h.taken_at = Some(1_718_273_700);
        assert_eq!(
            destination_for(&h, "IMG_1234.MOV"),
            "Home Videos/2024/06/IMG_1234.MOV"
        );
        h.taken_at = None;
        assert_eq!(
            destination_for(&h, "IMG_1234.MOV"),
            "Home Videos/Undated/IMG_1234.MOV"
        );
        let mut p = class(Category::Photo);
        p.taken_at = Some(0);
        assert_eq!(destination_for(&p, "a:b.jpg"), "Photos/1970/01/a_b.jpg");
        assert_eq!(
            destination_for(&class(Category::Other), "notes.PDF"),
            "Other/notes.PDF"
        );
    }

    #[test]
    fn numbered_variants() {
        assert!(is_numbered_variant("Movies/X/X (2).mkv", "Movies/X/X.mkv"));
        assert!(is_numbered_variant("Movies/X/X (12).mkv", "Movies/X/X.mkv"));
        assert!(!is_numbered_variant("Movies/X/X.mkv", "Movies/X/X.mkv"));
        assert!(!is_numbered_variant("Movies/X/X (a).mkv", "Movies/X/X.mkv"));
        assert!(!is_numbered_variant("Movies/Y/X (2).mkv", "Movies/X/X.mkv"));
        assert!(!is_numbered_variant("Movies/X/X (2).mp4", "Movies/X/X.mkv"));
        assert!(!is_numbered_variant("Movies/X/X ().mkv", "Movies/X/X.mkv"));
    }

    #[test]
    fn components() {
        assert_eq!(component("Title."), "Title");
        assert_eq!(component("..."), "untitled");
        assert_eq!(component("What If: A Story"), "What If - A Story");
    }
}
