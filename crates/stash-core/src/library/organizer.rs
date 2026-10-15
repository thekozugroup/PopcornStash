//! Moving files: automatic sorting, explicit organize, review, trash.

use std::path::{Path, PathBuf};
use std::time::Duration;

use walkdir::WalkDir;

use std::collections::HashMap;

use super::access::Access;
use super::classify::{camera_from_tags, classify, media_kind};
use super::defs::{self, LibraryDef, Shape};
use super::parse::parse_name;
use super::scanner::{
    Meta, enqueue_media_jobs, find_moved, in_place_category, is_extras_dir, is_junk_name,
    is_sample, kept_capture_time, library_metadata, mtime_secs, new_row, normalize_rel,
    overwrite_row, publish, publish_moved, quick_hash_async,
};
use super::{Library, OrganizeRequest, ReviewAction, repo};
use crate::capture::{self, CaptureTime, Recorded};
use crate::events::{Event, LibraryChange};
use crate::jobs::JobKind;
use crate::media::ThumbSize;
use crate::media::exif::ExifInfo;
use crate::models::{
    Category, Classification, Confidence, DownloadRow, FolderState, ItemRow, ItemStatus, MediaKind,
    MediaSummary, Place, Suggestion, Waiting,
};
use crate::paths::{self, StashLayout, Zone};
use crate::{CoreError, Result, now};

/// Longest we wait for ffprobe or EXIF while sorting before classifying by name.
const PROBE_TIMEOUT: Duration = Duration::from_secs(90);
const UNKNOWN_SHOW: &str = "Unknown Show";

/// Where a file of this classification sits inside a library folder (section 4, claims):
/// `Title (Year)/Title (Year).ext`, `Show/Season 01/Show - S01E02.ext` or `YYYY/MM/name`;
/// for `other`, the file's own (safe) name.
pub fn layout_for(classification: &Classification, file_name: &str) -> String {
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
            format!("{folder}/{folder}{ext}")
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
                    format!("{show}/Season {season:02}/{show} - {marker}{ext}")
                }
                (Some(season), None) => format!("{show}/Season {season:02}/{name}"),
                (None, None) => format!("{show}/{name}"),
            }
        }
        // The month on the camera's clock (section 4, Capture time).
        Category::HomeVideo | Category::Photo => match c.taken() {
            Some(taken) => {
                let (year, month) = taken.year_month();
                format!("{year:04}/{month:02}/{name}")
            }
            None => format!("Undated/{name}"),
        },
        Category::Other => name,
    }
}

/// The relative path a file gets: `<library folder>/<layout>`, or `Other/<name>` when
/// `folder` is None (Other files).
pub fn destination_in(
    folder: Option<&str>,
    classification: &Classification,
    file_name: &str,
) -> String {
    let layout = layout_for(classification, file_name);
    match folder {
        Some(folder) => format!("{folder}/{layout}"),
        None => format!("{}/{}", paths::OTHER, paths::sanitize_filename(file_name)),
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
    sort_path_as_of(lib, rel_path, download_id, lib.claims_generation()).await
}

/// [`sort_path`] for a job that started when the claims generation was `generation`.
pub(crate) async fn sort_path_as_of(
    lib: &Library,
    rel_path: &str,
    download_id: Option<&str>,
    generation: u64,
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
        // A file inside a download is judged with the rest of that download (is there a
        // video next to this picture?), so the plan covers the whole top-level entry.
        let top = super::watcher::top_level_entry(&rel).unwrap_or_else(|| rel.clone());
        let root = lib.layout.abs(&top)?;
        let taken = if completed {
            taken_from_downloads(lib, Some(&top))
                .await?
                .remove(&top)
                .unwrap_or_default()
        } else {
            Taken::default()
        };
        let (abs, layout) = (abs.clone(), lib.layout.clone());
        tokio::task::spawn_blocking(move || {
            plan_sort(&layout, &root, completed, taken)
                .files
                .into_iter()
                .filter(|(path, _)| path.starts_with(&abs))
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|e| CoreError::Other(e.into()))?
    };
    let auto_sort = lib.settings.get().await.auto_sort;
    // One sort job, one set of claims (section 4): when a library write changes where
    // files go while this job runs, the rest of its files wait in Inbox.
    let download = match &download_id {
        Some(id) => download_row(lib, id).await?,
        None => None,
    };
    let job = SortJob {
        library_id: download.as_ref().and_then(|d| d.library_id.clone()),
        download_id: download_id.clone(),
        added_by: download.and_then(|d| d.created_by),
        auto_sort,
        generation,
    };
    let mut changed = Vec::new();
    let mut linked = Vec::new();
    for (file, role) in files {
        match sort_file(lib, &file, role, &job).await {
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

// ---------------------------------------------------------------------------
// What a sort looks at

/// How the sorter treats one file under a sort root (a top-level entry of `Inbox/` or
/// `Downloads/Completed/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// A video, photo or song: classified as usual.
    Media,
    /// Not media, and not part of a download that holds media: an uploaded PDF, a zip,
    /// a folder of documents. It waits in the Inbox, suggested for `Other/`.
    Standalone,
    /// A picture that may be a release's artwork rather than a family photo: one in a
    /// download that also holds a video or a song, or one named like artwork in a
    /// folder. It waits for review, suggested for `Other/`, instead of going straight
    /// into Photos.
    MaybeArtwork,
}

/// The files a sort of one root looks at, and the newest modification time under the
/// root (the rescan sorts a root only once it has settled).
pub(crate) struct SortPlan {
    pub files: Vec<(PathBuf, Role)>,
    pub newest_mtime: i64,
}

/// What earlier sorts already took out of a download folder. Once a release's video
/// has moved into the library, the folder is sorted again (it changed), and its
/// leftovers (poster.jpg, RARBG.txt) must not then pass for a download of pictures or
/// documents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Taken {
    /// A video or a song.
    pub av: bool,
    /// Any video, photo or song.
    pub media: bool,
}

/// [`Taken`] for each download folder in Completed that PopcornStash sorted before
/// (from the items linked to its download), or for just `rel` when given.
pub(crate) async fn taken_from_downloads(
    lib: &Library,
    rel: Option<&str>,
) -> Result<std::collections::HashMap<String, Taken>> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT d.rel_path, MAX(i.kind IN ('video', 'audio')), MAX(i.kind != 'other')
         FROM downloads d
         JOIN download_items di ON di.download_id = d.id
         JOIN items i ON i.id = di.item_id
         WHERE d.rel_path IS NOT NULL AND (? IS NULL OR d.rel_path = ?)
         GROUP BY d.rel_path",
    )
    .bind(rel)
    .bind(rel)
    .fetch_all(&lib.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(path, av, media)| {
            (
                path,
                Taken {
                    av: av != 0,
                    media: media != 0,
                },
            )
        })
        .collect())
}

/// Release files that only mean something next to the release's video: never sorted
/// on their own when they sit in a folder.
const CLUTTER_EXTS: &[&str] = &[
    "nfo", "sfv", "url", "md5", "sha1", "sha256", "sha512", "par2", "webloc", "lnk",
];

/// Folders inside a download that hold the release's pictures.
const ARTWORK_DIRS: &[&str] = &[
    "screens",
    "screenshots",
    "screenshot",
    "screencaps",
    "artwork",
    "art",
    "covers",
    "scans",
    "posters",
    "fanart",
    "extrafanart",
    "extrathumbs",
    "proof",
    "booklet",
];

/// Picture names that media players and releases use for artwork, after trailing
/// numbers are dropped ("poster.jpg", "fanart2.jpg", "screen01.png", "cd1.jpg").
const ARTWORK_NAMES: &[&str] = &[
    "poster",
    "cover",
    "folder",
    "fanart",
    "banner",
    "thumb",
    "backdrop",
    "background",
    "clearart",
    "clearlogo",
    "logo",
    "disc",
    "discart",
    "cd",
    "landscape",
    "keyart",
    "front",
    "back",
    "inlay",
    "albumart",
    "albumartsmall",
    "screen",
    "screens",
    "screenshot",
    "screencap",
    "shot",
    "snapshot",
    "proof",
];

/// Kodi-style artwork next to a video, named after it: "Heat (1995)-poster.jpg".
const ARTWORK_SUFFIXES: &[&str] = &[
    "poster",
    "fanart",
    "banner",
    "thumb",
    "landscape",
    "clearart",
    "clearlogo",
    "logo",
    "disc",
    "discart",
    "keyart",
    "backdrop",
    "cover",
];

/// Whether a picture at `path` inside the download `root` is the release's artwork.
pub(crate) fn is_artwork(root: &Path, path: &Path) -> bool {
    let in_artwork_dir = path
        .strip_prefix(root)
        .ok()
        .and_then(Path::parent)
        .is_some_and(|dirs| {
            dirs.components().any(|c| {
                ARTWORK_DIRS.contains(&c.as_os_str().to_string_lossy().to_lowercase().as_str())
            })
        });
    if in_artwork_dir {
        return true;
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let base = stem.trim_end_matches(|c: char| c.is_ascii_digit() || matches!(c, ' ' | '-' | '_'));
    ARTWORK_NAMES.contains(&base)
        || base.starts_with("albumart")
        // Site adverts ("www.YTS.MX.jpg").
        || base.starts_with("www.")
        || ARTWORK_SUFFIXES.iter().any(|name| {
            base.strip_suffix(name)
                .is_some_and(|rest| rest.ends_with(['-', '_', '.']))
        })
}

fn is_clutter(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| CLUTTER_EXTS.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
}

/// A subtitle or other sidecar next to the media file it belongs to: it travels with
/// that file (see [`move_sidecars`]) and is not sorted on its own.
fn claimed_by_media(path: &Path) -> bool {
    if !super::scanner::is_sidecar(path) {
        return false;
    }
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    let name = name.to_string_lossy();
    let Ok(read) = std::fs::read_dir(dir) else {
        return false;
    };
    read.flatten().any(|entry| {
        let media = entry.file_name().to_string_lossy().into_owned();
        entry.file_type().is_ok_and(|t| t.is_file())
            && !is_junk_name(&media)
            && media_kind(&entry.path()) != MediaKind::Other
            && sidecar_claim(&name, &media).is_some()
    })
}

/// What a sort of `root` (a file or folder) looks at. `taken` says what earlier sorts
/// of this folder already moved out.
/// - Videos, photos and songs, except samples and extras folders inside downloads.
/// - In a folder with a video or a song (or named like a release), pictures named or
///   kept like artwork are left alone, and in a download with a video or song the other
///   pictures wait for review ([`Role::MaybeArtwork`]). In other folders artwork-like
///   pictures wait for review too, and the rest are photos.
/// - In a folder with any media, the other files are its companions and stay
///   (subtitles travel with their video when it moves).
/// - Anything else is sorted on its own ([`Role::Standalone`]), except release clutter
///   (`.nfo`, `.sfv`, ...) in a folder, or a sidecar whose media file sits next to it.
/// - A single file is taken as it is: a sent image named "poster.jpg" is a photo.
pub(crate) fn plan_sort(
    layout: &StashLayout,
    root: &Path,
    completed: bool,
    taken: Taken,
) -> SortPlan {
    let mut plan = SortPlan {
        files: Vec::new(),
        newest_mtime: 0,
    };
    let Ok(meta) = std::fs::symlink_metadata(root) else {
        return plan;
    };
    plan.newest_mtime = mtime_secs(&meta);
    if meta.is_file() {
        let name = file_name(root);
        if is_junk_name(&name) {
            return plan;
        }
        match media_kind(root) {
            MediaKind::Other if claimed_by_media(root) => {}
            MediaKind::Other => plan.files.push((root.to_path_buf(), Role::Standalone)),
            // Samples are only skipped inside downloads; "Sample day.mp4" in the Inbox is a video.
            _ if completed && is_sample(&name, meta.len()) => {}
            _ => plan.files.push((root.to_path_buf(), Role::Media)),
        }
        return plan;
    }
    if !meta.is_dir() {
        return plan;
    }
    let in_progress = layout.in_progress();
    let mut found = Vec::new();
    let mut has_av = taken.av;
    let entries = WalkDir::new(root)
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
        });
    for entry in entries.flatten() {
        let Ok(m) = entry.metadata() else { continue };
        plan.newest_mtime = plan.newest_mtime.max(mtime_secs(&m));
        if !entry.file_type().is_file() {
            continue;
        }
        let kind = media_kind(entry.path());
        has_av |= matches!(kind, MediaKind::Video | MediaKind::Audio);
        if kind != MediaKind::Other
            && completed
            && is_sample(&entry.file_name().to_string_lossy(), m.len())
        {
            continue;
        }
        found.push((entry.into_path(), kind));
    }
    let has_media =
        has_av || taken.media || found.iter().any(|(_, kind)| *kind != MediaKind::Other);
    // A folder named like a release ("Some.Movie.2019.1080p.WEB.x264-GRP") is one, even
    // when it was copied in by hand and its video has moved on: its artwork stays put.
    let release = has_av || {
        let parsed = parse_name(&file_name(root));
        parsed.has_release_tags || parsed.episode.is_some()
    };
    for (path, kind) in found {
        let role = match kind {
            MediaKind::Video | MediaKind::Audio => Some(Role::Media),
            MediaKind::Photo if is_artwork(root, &path) => (!release).then_some(Role::MaybeArtwork),
            MediaKind::Photo if has_av && completed => Some(Role::MaybeArtwork),
            MediaKind::Photo => Some(Role::Media),
            MediaKind::Other if has_media || is_clutter(&path) => None,
            MediaKind::Other => Some(Role::Standalone),
        };
        if let Some(role) = role {
            plan.files.push((path, role));
        }
    }
    plan
}

/// A picture that may be a release's artwork: suggested for `Other/` and left for
/// review, so neither artwork nor a family photo is filed blindly. Its date is kept in
/// case the reviewer files it under Photos.
fn possible_artwork(mut c: Classification, file_name: &str) -> Classification {
    c.category = Category::Other;
    c.confidence = Confidence::Low;
    c.reason = "This picture may be artwork from a movie, show or album".into();
    c.layout = layout_for(&c, file_name);
    c
}

/// What one sort job keeps for all of its files.
struct SortJob {
    library_id: Option<String>,
    download_id: Option<String>,
    /// Who brought the files in (the download's creator).
    added_by: Option<String>,
    auto_sort: bool,
    /// The claims generation when the job started.
    generation: u64,
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

async fn sort_file(lib: &Library, abs: &Path, role: Role, job: &SortJob) -> Result<Sorted> {
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
    if role == Role::MaybeArtwork {
        c = possible_artwork(c, &file_name(abs));
    }
    let camera = exif
        .as_ref()
        .and_then(|e| e.camera.clone())
        .or_else(|| probe.as_ref().and_then(camera_from_tags));
    let hash = quick_hash_async(abs).await;

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
        None => find_moved(lib, size, hash.as_deref()).await?,
    };
    let own_id = target.as_ref().map(|t| t.id.clone());

    // Where it goes, from the claims as they are now (section 4, claims).
    let library_defs = defs::load_all(&lib.db).await?;
    let preferred = match (job.added_by.as_deref(), job.library_id.as_deref()) {
        (Some(user), Some(id)) => super::destinations::visible(&lib.db, user, id).await?,
        _ => None,
    };
    // An unidentified video still fits an explicitly chosen one-off library.
    // Do not invent episode numbers, or treat documents/artwork as videos.
    if let Some(def) = preferred.as_ref()
        && c.category == Category::Other
        && kind == MediaKind::Video
        && def.shape() == Shape::OneOffs
    {
        c.category = in_place_category(
            def,
            &format!("{}/{}", def.folder(), file_name(abs)),
            kind,
            meta.len(),
        )
        .unwrap_or(Category::Movie);
        if c.category == Category::HomeVideo && c.taken().is_none() {
            c.set_taken(Recorded::instant(mtime).and_then(|t| t.resolve(&capture::server_zone())));
        }
        c.layout = layout_for(&c, &file_name(abs));
    }
    let preferred = preferred.filter(|def| def.shape().fits(c.category));
    let mut row = new_row(
        &rel,
        c.category,
        ItemStatus::NeedsReview,
        Meta::from_classification(&c),
        size,
        mtime,
        hash,
    );
    row.source_download_id = job.download_id.clone();
    row.added_by = job.added_by.clone();
    row.camera = camera;
    fill_media(&mut row, probe.as_ref(), exif.as_ref());
    let claimer = preferred.as_ref().or_else(|| {
        (c.category != Category::Other)
            .then(|| library_defs.iter().find(|d| d.claims(c.category)))
            .flatten()
    });
    if let Some(def) = claimer
        && c.category == Category::Episode
    {
        canonicalize_show(lib, &mut c, def).await?;
        row.show_name.clone_from(&c.show_name);
        row.title.clone_from(&c.title);
        row.sort_title = repo::sort_title(&row.title);
    }
    let claims_changed = lib.claims_generation() != job.generation;
    // Choosing a compatible library is an explicit placement decision. Confidence
    // and auto-sort gate only automatic destinations; folder and generation checks
    // still protect the move when the library changes or its storage disappears.
    let automatic_waits = !job.auto_sort || c.confidence != Confidence::High;
    let destination = if claims_changed || (preferred.is_none() && automatic_waits) {
        None
    } else if c.category == Category::Other {
        Some(destination_in(None, &c, &file_name(abs)))
    } else {
        match claimer {
            // A kind no library claims waits for a person to pick a library.
            None => None,
            Some(def) => match defs::ready_folder(lib, def).await {
                Ok(()) => Some(show_destination(lib, def, &c, &file_name(abs)).await?),
                // A missing or read-only folder: the file waits until it is back.
                Err(_) => None,
            },
        }
    };
    if claims_changed {
        tracing::info!(path = %rel, "the libraries changed while this download was sorted; leaving the rest in Inbox");
    }
    if let Some(dest) = destination {
        match move_into_library(lib, abs, &dest, own_id.as_deref()).await {
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
    let mut suggestion = serde_json::to_value(&c)?;
    if let Some(def) = preferred {
        suggestion["preferred_library_id"] = serde_json::Value::String(def.id().into());
    }
    row.suggestion_json = Some(serde_json::to_string(&suggestion)?);
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
            enqueue_media_jobs(lib, &item.id).await?;
            publish_moved(lib, LibraryChange::Updated, &item.id, item.facts());
            return Ok(item.id);
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
                enqueue_media_jobs(lib, &current.id).await?;
                publish_moved(lib, LibraryChange::Updated, &current.id, current.facts());
                return Ok(current.id);
            }
        }
    };
    enqueue_media_jobs(lib, &id).await?;
    publish(lib, change, &id);
    Ok(id)
}

/// Reuse the spelling of a show that is already in the library `def`.
async fn canonicalize_show(lib: &Library, c: &mut Classification, def: &LibraryDef) -> Result<()> {
    let Some(show) = c.show_name.clone().filter(|s| !s.trim().is_empty()) else {
        return Ok(());
    };
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT show_name FROM items
         WHERE library_id = ? AND category = 'episode' AND status IN ('ready', 'missing')
           AND show_name = ? COLLATE NOCASE
         ORDER BY added_at, id LIMIT 1",
    )
    .bind(def.id())
    .bind(&show)
    .fetch_optional(&lib.db)
    .await?;
    if let Some(name) = existing {
        if let Some((s, e)) = c.season.zip(c.episode) {
            c.title = format!("{name} S{s:02}E{e:02}");
        }
        c.show_name = Some(name);
    }
    Ok(())
}

/// The destination in `def` for `c`, keeping an episode in its show's existing folder
/// (which may be named differently from the show).
async fn show_destination(
    lib: &Library,
    def: &LibraryDef,
    c: &Classification,
    file_name: &str,
) -> Result<String> {
    let mut dest = destination_in(Some(def.folder()), c, file_name);
    if c.category != Category::Episode {
        return Ok(dest);
    }
    let Some(show) = c.show_name.clone().filter(|s| !s.trim().is_empty()) else {
        return Ok(dest);
    };
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT rel_path FROM items
         WHERE library_id = ? AND category = 'episode' AND status IN ('ready', 'missing') AND show_name = ?
         ORDER BY added_at, id LIMIT 1",
    )
    .bind(def.id())
    .bind(&show)
    .fetch_optional(&lib.db)
    .await?;
    if let Some(rel_path) = existing {
        let mut parts = rel_path.split('/');
        if let (Some(_), Some(folder), Some(_)) = (parts.next(), parts.next(), parts.next()) {
            let ours = component(&show);
            let prefix = format!("{}/{ours}/", def.folder());
            if folder != ours
                && let Some(rest) = dest.strip_prefix(&prefix)
            {
                dest = format!("{}/{folder}/{rest}", def.folder());
            }
        }
    }
    Ok(dest)
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
    let library_defs = defs::load_all(&lib.db).await?;
    if StashLayout::zone(&dest_rel) != Zone::Other
        && defs::library_for_path(&library_defs, &dest_rel).is_none()
    {
        return Err(CoreError::invalid(
            "Files can only be organized into a library or Other files.",
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
    let mut tx = crate::db::begin_write(&lib.db).await?;
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

/// Where an organize or accept sends a file.
enum Target {
    Library(LibraryDef),
    OtherFiles,
}

/// The target named by `request` (section 8, Inbox accept and Organize): `place` and
/// `library_id` when sent; otherwise the suggestion as it reads now (Other files for
/// `other`, else the library that claims the kind), else the item's own library.
fn resolve_target(
    item: &ItemRow,
    request: &OrganizeRequest,
    defs: &[LibraryDef],
) -> Result<Target> {
    let find = |id: &str| {
        defs.iter()
            .find(|d| d.id() == id)
            .cloned()
            .map(Target::Library)
            .ok_or_else(|| CoreError::library_not_found(Some("library_id")))
    };
    let claimer = |category: Category| defs.iter().find(|d| d.claims(category));
    let suggested = item.suggestion().map(|s| s.category);
    match request.place {
        Some(Place::OtherFiles) => Ok(Target::OtherFiles),
        Some(Place::Library) => match request.library_id.as_deref() {
            Some(id) => find(id),
            None => request
                .category
                .or(suggested)
                .and_then(claimer)
                .map(|d| Target::Library(d.clone()))
                .ok_or_else(choose_library),
        },
        Some(_) => Err(CoreError::bad_field(
            "place",
            "Choose a library or Other files.",
        )),
        None => {
            if let Some(id) = request.library_id.as_deref() {
                return find(id);
            }
            match request.category.or(suggested) {
                Some(Category::Other) => return Ok(Target::OtherFiles),
                Some(category) => {
                    if let Some(def) = claimer(category) {
                        return Ok(Target::Library(def.clone()));
                    }
                }
                None => {}
            }
            match (item.place, item.library_id.as_deref()) {
                (Place::Library, Some(id)) => find(id),
                (Place::OtherFiles, _) => Ok(Target::OtherFiles),
                _ => Err(choose_library()),
            }
        }
    }
}

fn choose_library() -> CoreError {
    CoreError::bad_field("library_id", "Choose a library for this file.")
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
    let mut target = Classification {
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
        taken_at: None,
        taken_local: None,
        taken_offset: None,
        confidence: Confidence::High,
        reason: "Organized by hand".to_string(),
        layout: String::new(),
    };
    target.set_taken(
        item.taken()
            .or_else(|| s.and_then(Classification::taken))
            .or_else(|| Recorded::instant(item.mtime)?.resolve(&capture::server_zone())),
    );
    target
}

/// Longest title or show name accepted by organize and review (as for metadata edits).
const MAX_NAME_CHARS: usize = 500;

/// What the file becomes in `def` (section 8, Inbox accept and Organize: the file must
/// fit the library).
fn fit_category(
    def: &LibraryDef,
    item: &ItemRow,
    request: &OrganizeRequest,
    file_name: &str,
) -> Result<Category> {
    defs::accepts(def.shape(), item.kind)?;
    Ok(match def.shape() {
        Shape::Photos => Category::Photo,
        Shape::Episodes => Category::Episode,
        Shape::OneOffs => {
            let one_off = |c: Category| matches!(c, Category::Movie | Category::HomeVideo);
            request
                .category
                .filter(|c| one_off(*c))
                .or_else(|| {
                    item.suggestion()
                        .map(|s| s.category)
                        .filter(|c| one_off(*c))
                })
                .or_else(|| {
                    in_place_category(
                        def,
                        &format!("{}/{file_name}", def.folder()),
                        item.kind,
                        u64::try_from(item.size).unwrap_or(0),
                    )
                })
                .unwrap_or(Category::Movie)
        }
    })
}

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
    let library_defs = defs::load_all(&lib.db).await?;
    let target = resolve_target(&item, &request, &library_defs)?;
    let abs = lib.layout.abs(&item.rel_path)?;
    let name = file_name(&abs);
    let mut target_class = organize_target(&item, &request, prefer_suggestion);
    match &target {
        Target::OtherFiles => target_class.category = Category::Other,
        Target::Library(def) => {
            target_class.category = fit_category(def, &item, &request, &name)?;
        }
    }
    if target_class
        .year
        .is_some_and(|year| !(1800..=2200).contains(&year))
    {
        return Err(CoreError::invalid(
            "The year must be between 1800 and 2200.",
        ));
    }
    if target_class.season.is_some_and(|s| s < 0) || target_class.episode.is_some_and(|e| e < 0) {
        return Err(CoreError::invalid(
            "Season and episode numbers cannot be negative.",
        ));
    }
    match target_class.category {
        Category::Movie if target_class.title.trim().is_empty() => {
            return Err(CoreError::bad_field(
                "title",
                "Add a title to file this as a movie.",
            ));
        }
        Category::Episode
            if target_class
                .show_name
                .as_deref()
                .is_none_or(|s| s.trim().is_empty()) =>
        {
            return Err(CoreError::bad_field(
                "show_name",
                "Add the show name to file this as an episode.",
            ));
        }
        Category::Episode if target_class.episode.is_none() => {
            return Err(CoreError::bad_field(
                "episode",
                "Add the episode number to file this as an episode.",
            ));
        }
        _ => {}
    }
    if target_class.category == Category::Episode {
        target_class.season = Some(target_class.season.unwrap_or(1));
        if let (Some(show), Some(s), Some(e)) = (
            &target_class.show_name,
            target_class.season,
            target_class.episode,
        ) {
            target_class.title = format!("{show} S{s:02}E{e:02}");
        }
    }
    if tokio::fs::symlink_metadata(&abs).await.is_err() {
        return Err(CoreError::invalid(
            "The file is no longer where PopcornStash expected it. Try a rescan.",
        ));
    }
    let destination = match &target {
        Target::OtherFiles => destination_in(None, &target_class, &name),
        Target::Library(def) => {
            defs::ready_folder(lib, def).await?;
            if target_class.category == Category::Episode {
                canonicalize_show(lib, &mut target_class, def).await?;
            }
            show_destination(lib, def, &target_class, &name).await?
        }
    };

    let review_before = repo::needs_review_count(&lib.db).await?;
    let new_rel =
        if destination == item.rel_path || is_numbered_variant(&item.rel_path, &destination) {
            item.rel_path.clone()
        } else {
            move_into_library(lib, &abs, &destination, Some(&item.id)).await?
        };
    let target = target_class;
    let taken = match target.category {
        Category::HomeVideo | Category::Photo => target.taken(),
        _ => item.taken(),
    };
    let (taken_at, taken_local, taken_offset) = CaptureTime::columns(taken.as_ref());
    sqlx::query(
        "UPDATE items SET rel_path = ?, category = ?, title = ?, sort_title = ?, year = ?, show_name = ?, season = ?,
            episode = ?, episode_title = ?, taken_at = ?, taken_local = ?, taken_offset = ?, status = 'ready',
            suggestion_json = NULL, missing_since = NULL, updated_at = ?
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
    .bind(taken_at)
    .bind(&taken_local)
    .bind(taken_offset)
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
    publish_moved(lib, LibraryChange::Updated, &item.id, item.facts());
    publish_inbox_if_changed(lib, review_before).await?;
    repo::get(&lib.db, &item.id)
        .await?
        .ok_or(CoreError::NotFound)
}

/// The sentence for an Inbox decision on an item that is no longer waiting.
pub const ALREADY_SORTED: &str = "That file was already sorted.";

pub async fn review(lib: &Library, item_id: &str, action: ReviewAction) -> Result<ItemRow> {
    let _moves = lib.lock_moves().await;
    let item = repo::get(&lib.db, item_id)
        .await?
        .ok_or(CoreError::NotFound)?;
    match action {
        ReviewAction::Accept(request) => {
            // The target is checked before the item's state (section 8).
            let library_defs = defs::load_all(&lib.db).await?;
            resolve_target(&item, &request, &library_defs)?;
            let ignored = item.status == ItemStatus::Ready && item.place == Place::Incoming;
            if item.status != ItemStatus::NeedsReview && !ignored {
                return Err(CoreError::conflict(ALREADY_SORTED));
            }
            organize_item(lib, item, request, true).await
        }
        ReviewAction::Ignore => {
            if item.status != ItemStatus::NeedsReview {
                return Err(CoreError::conflict(ALREADY_SORTED));
            }
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
            publish_moved(lib, LibraryChange::Updated, &item.id, item.facts());
            publish_inbox_if_changed(lib, review_before).await?;
            repo::get(&lib.db, &item.id)
                .await?
                .ok_or(CoreError::NotFound)
        }
    }
}

/// A stored classification as `access` reads it now (section 8, `Classification`):
/// where it would go and why it waits, from the claims, the folder states and what the
/// reader sees. Nothing about a library the reader cannot see is shown.
pub fn suggestion_view(
    row: &ItemRow,
    access: &Access,
    library_defs: &[LibraryDef],
    states: &HashMap<String, FolderState>,
) -> Option<Suggestion> {
    let mut c = row.suggestion()?;
    let name = row.rel_path.rsplit('/').next().unwrap_or(&row.rel_path);
    c.layout = layout_for(&c, name);
    let waiting_now = row.status == ItemStatus::NeedsReview;
    let low = c.confidence == Confidence::Low;
    let (place, library_id, destination, waiting) = if c.category == Category::Other {
        let waiting = if low {
            Waiting::LowConfidence
        } else {
            Waiting::Confirm
        };
        (
            Some(Place::OtherFiles),
            None,
            Some(destination_in(None, &c, name)),
            waiting,
        )
    } else {
        match super::destinations::suggested(row, access, library_defs, c.category).or_else(|| {
            access
                .visible_claimer(c.category)
                .and_then(|id| library_defs.iter().find(|d| d.id() == id))
        }) {
            None => (None, None, None, Waiting::ChooseLibrary),
            Some(def) => {
                let waiting = match states.get(def.id()) {
                    Some(FolderState::Missing | FolderState::ReadOnly) => {
                        Waiting::FolderUnavailable
                    }
                    _ if low => Waiting::LowConfidence,
                    _ => Waiting::Confirm,
                };
                (
                    Some(Place::Library),
                    Some(def.id().to_string()),
                    Some(destination_in(Some(def.folder()), &c, name)),
                    waiting,
                )
            }
        }
    };
    Some(Suggestion {
        classification: c,
        place,
        library_id,
        destination,
        waiting: waiting_now.then_some(waiting),
    })
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
    publish_moved(lib, LibraryChange::Removed, &item.id, item.facts());
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
    let mut tx = crate::db::begin_write(&lib.db).await?;
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

/// Whether the library that now holds a trashed item's original path would show it, and
/// what it would be there (section 11, Restore from Trash). `Err` carries the 409.
async fn restore_plan(
    lib: &Library,
    item: &ItemRow,
    original_rel: &str,
    create_folder: bool,
) -> Result<Option<(LibraryDef, Category)>> {
    let library_defs = defs::load_all(&lib.db).await?;
    let Some(def) = defs::library_for_path(&library_defs, original_rel) else {
        return Ok(None);
    };
    let size = u64::try_from(item.size).unwrap_or(0);
    // What the library there now shows (section 4, Files already in a library). A file
    // keeps its category when that still fits the library.
    let shown = in_place_category(def, original_rel, item.kind, size);
    let category = shown.map(|c| {
        if def.shape().fits(item.category) {
            item.category
        } else {
            c
        }
    });
    let Some(category) = category else {
        return Err(CoreError::conflict(format!(
            "{} does not show this kind of file any more, so it stays in the trash.",
            def.row.name
        )));
    };
    if create_folder {
        defs::ready_folder(lib, def).await.map_err(|_| {
            CoreError::conflict(
                "This library's folder is missing or cannot be written, so nothing can move there now.",
            )
        })?;
    } else {
        let live: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM items WHERE library_id = ? AND status != 'missing'",
        )
        .bind(def.id())
        .fetch_one(&lib.db)
        .await?;
        if defs::folder_state(&lib.layout, def.folder(), live > 0).await != FolderState::Ok {
            return Err(CoreError::conflict(
                "This library's folder is missing or cannot be written, so nothing can move there now.",
            ));
        }
    }
    Ok(Some((def.clone(), category)))
}

/// The 409 sentence a restore of this trashed item would get now, if any.
pub async fn restore_refusal(lib: &Library, item_id: &str) -> Result<Option<String>> {
    let original: Option<String> =
        sqlx::query_scalar("SELECT original_rel_path FROM trash WHERE item_id = ?")
            .bind(item_id)
            .fetch_optional(&lib.db)
            .await?;
    let (Some(original), Some(item)) = (original, repo::get(&lib.db, item_id).await?) else {
        return Ok(None);
    };
    match restore_plan(lib, &item, &original, false).await {
        Ok(_) => Ok(None),
        Err(CoreError::Conflict(message)) => Ok(Some(message)),
        Err(e) => Err(e),
    }
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
    // Refused before anything moves when the library there now would not show it.
    let plan = restore_plan(lib, &item, &original_rel, true).await?;
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
    let mut tx = crate::db::begin_write(&lib.db).await?;
    sqlx::query("UPDATE items SET rel_path = ?, status = ?, missing_since = NULL, updated_at = ? WHERE id = ?")
        .bind(&new_rel)
        .bind(status.as_str())
        .bind(now())
        .bind(item_id)
        .execute(&mut *tx)
        .await?;
    // Indexed under the rules of the library its path is in now.
    let mut probe_again = false;
    if let Some((_, category)) = plan
        && category != item.category
    {
        let meta = library_metadata(category, &new_rel, item.mtime);
        let taken = kept_capture_time(category, item.taken(), meta.taken);
        // Dated from its name or file time only: the probe reads its EXIF or tags.
        probe_again = item.taken().is_none() && taken.is_some();
        let (taken_at, taken_local, taken_offset) = CaptureTime::columns(taken.as_ref());
        sqlx::query(
            "UPDATE items SET category = ?, title = ?, sort_title = ?, year = ?, show_name = ?, season = ?,
                episode = ?, episode_title = ?, taken_at = ?, taken_local = ?, taken_offset = ?
             WHERE id = ?",
        )
        .bind(category.as_str())
        .bind(&meta.title)
        .bind(repo::sort_title(&meta.title))
        .bind(meta.year)
        .bind(&meta.show_name)
        .bind(meta.season)
        .bind(meta.episode)
        .bind(&meta.episode_title)
        .bind(taken_at)
        .bind(&taken_local)
        .bind(taken_offset)
        .bind(item_id)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM trash WHERE item_id = ?")
        .bind(item_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if probe_again {
        lib.jobs
            .enqueue(JobKind::Probe, Some(item_id), serde_json::json!({}))
            .await?;
    }
    tracing::info!(item = %item_id, path = %new_rel, "restored an item from the trash");
    publish_moved(lib, LibraryChange::Added, item_id, item.facts());
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
            taken_local: None,
            taken_offset: None,
            confidence: Confidence::High,
            reason: String::new(),
            layout: String::new(),
        }
    }

    /// The destination in the starting library for the kind (or Other files).
    fn destination_for(c: &Classification, file_name: &str) -> String {
        let folder = match c.category {
            Category::Movie => Some("Movies"),
            Category::Episode => Some("Shows"),
            Category::HomeVideo => Some("Home Videos"),
            Category::Photo => Some("Photos"),
            Category::Other => None,
        };
        destination_in(folder, c, file_name)
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
        // 23:30 on June 30 at -04:00 is July 1 in UTC: the folder follows the camera's clock.
        h.set_taken(
            Recorded::clock(2024, 6, 30, 23, 30, 0, Some(-240))
                .and_then(|r| r.resolve(&jiff::tz::TimeZone::UTC)),
        );
        assert_eq!(h.taken_at, Some(1_719_804_600));
        assert_eq!(
            destination_for(&h, "IMG_1234.MOV"),
            "Home Videos/2024/06/IMG_1234.MOV"
        );
        h.set_taken(None);
        assert_eq!(
            destination_for(&h, "IMG_1234.MOV"),
            "Home Videos/Undated/IMG_1234.MOV"
        );
        // A suggestion stored before capture times had clocks: its moment read in UTC.
        let mut p = class(Category::Photo);
        p.taken_at = Some(86_400 * 40);
        assert_eq!(destination_for(&p, "a:b.jpg"), "Photos/1970/02/a_b.jpg");
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
