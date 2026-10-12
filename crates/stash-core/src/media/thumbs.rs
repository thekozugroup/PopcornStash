//! Probe/thumbnail job handlers and photo renditions.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{exif, filters, probe, tools, MediaService, ThumbSize};
use crate::events::{Event, LibraryChange};
use crate::models::{Category, ItemRow, ItemStatus, MediaKind, MediaSummary, ThumbState};
use crate::{now, CoreError, Result};

/// One thumbnail or rendition may take this long (4K HEVC seeks on a NAS are slow).
const IMAGE_TIMEOUT: Duration = Duration::from_secs(120);
/// WebP quality for thumbnails.
const THUMB_QUALITY: u32 = 80;
/// Long edge and quality of browser-friendly photo renditions.
const RENDITION_EDGE: u32 = 2560;
const RENDITION_QUALITY: u32 = 85;
/// Photos browsers show as-is.
const BROWSER_IMAGE_EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "webp", "gif", "avif"];

async fn load_item(media: &MediaService, item_id: &str) -> Result<Option<ItemRow>> {
    Ok(sqlx::query_as::<_, ItemRow>("SELECT * FROM items WHERE id = ?")
        .bind(item_id)
        .fetch_optional(&media.db)
        .await?)
}

/// The item's file, or None (with a log line) when the job has nothing to work on.
async fn existing_file(media: &MediaService, item: &ItemRow, job: &str) -> Result<Option<PathBuf>> {
    if matches!(item.status, ItemStatus::Missing | ItemStatus::Trashed) {
        tracing::debug!(item_id = %item.id, job, status = %item.status, "skipped: file is not in the library");
        return Ok(None);
    }
    let abs = media.layout.abs(&item.rel_path)?;
    match tokio::fs::metadata(&abs).await {
        Ok(meta) if meta.is_file() => Ok(Some(abs)),
        _ => {
            tracing::debug!(item_id = %item.id, job, "skipped: file has moved or disappeared");
            Ok(None)
        }
    }
}

fn publish_updated(media: &MediaService, item_id: &str) {
    media.events.publish(Event::Library { r#type: LibraryChange::Updated, item_id: item_id.to_string() });
}

/// What the probe job learned about a file.
#[derive(Debug, Default, Clone, PartialEq)]
struct ProbeResult {
    mime: Option<String>,
    duration_s: Option<f64>,
    width: Option<i64>,
    height: Option<i64>,
    media_json: Option<String>,
    camera: Option<String>,
    taken_at: Option<i64>,
}

pub async fn process_item(media: &MediaService, item_id: &str) -> Result<()> {
    let Some(item) = load_item(media, item_id).await? else {
        tracing::debug!(item_id, "probe skipped: item no longer exists");
        return Ok(());
    };
    let Some(abs) = existing_file(media, &item, "probe").await? else {
        return Ok(());
    };

    let mut result = ProbeResult {
        mime: Some(guess_mime(&abs)),
        duration_s: item.duration_s,
        width: item.width,
        height: item.height,
        media_json: item.media_json.clone(),
        ..ProbeResult::default()
    };

    match item.kind {
        MediaKind::Video | MediaKind::Audio => {
            let summary = probe::probe(&abs).await?;
            let (width, height) = display_size(&summary);
            result.duration_s = summary.duration_s;
            result.width = width;
            result.height = height;
            result.camera = camera_from_tags(&summary.tags);
            // Creation time of a movie or episode is the encode date, not when it was filmed.
            if !matches!(item.category, Category::Movie | Category::Episode) {
                result.taken_at = taken_from_tags(&summary.tags);
            }
            result.media_json = Some(serde_json::to_string(&summary)?);
        }
        MediaKind::Photo => {
            let info = match media.read_exif(&abs).await {
                Ok(info) => info,
                Err(err) => {
                    tracing::warn!(item_id, error = %err, "could not read EXIF");
                    None
                }
            };
            let info = info.unwrap_or_default();
            let (mut width, mut height) = info.display_size();
            if width.is_none() || height.is_none() {
                if let Some((w, h)) = image_size(&abs).await {
                    // HEIF decoders apply the rotation themselves; other formats report stored pixels.
                    let decoder_rotates = matches!(extension(&abs).as_str(), "heic" | "heif" | "avif");
                    (width, height) =
                        if info.swaps_dimensions() && !decoder_rotates { (Some(h), Some(w)) } else { (Some(w), Some(h)) };
                }
            }
            result.width = width;
            result.height = height;
            result.camera = info.camera;
            result.taken_at = info.taken_at;
        }
        MediaKind::Other => {}
    }

    sqlx::query(
        "UPDATE items SET mime = ?, duration_s = ?, width = ?, height = ?, media_json = ?,
                camera = COALESCE(?, camera), taken_at = COALESCE(taken_at, ?), updated_at = ?
         WHERE id = ?",
    )
    .bind(&result.mime)
    .bind(result.duration_s)
    .bind(result.width)
    .bind(result.height)
    .bind(&result.media_json)
    .bind(&result.camera)
    .bind(result.taken_at)
    .bind(now())
    .bind(item_id)
    .execute(&media.db)
    .await?;
    publish_updated(media, item_id);
    Ok(())
}

pub async fn thumbnail_item(media: &MediaService, item_id: &str) -> Result<()> {
    let Some(item) = load_item(media, item_id).await? else {
        tracing::debug!(item_id, "thumbnail skipped: item no longer exists");
        return Ok(());
    };
    let Some(abs) = existing_file(media, &item, "thumb").await? else {
        return Ok(());
    };
    let lg = media.thumb_path(item_id, ThumbSize::Lg);
    let sm = media.thumb_path(item_id, ThumbSize::Sm);
    if let Some(dir) = lg.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }

    let outcome = match item.kind {
        MediaKind::Video => video_thumbs(&abs, &item, &lg, &sm).await.map(|()| ThumbState::Ready),
        MediaKind::Photo => photo_thumbs(&abs, &lg, &sm).await.map(|()| ThumbState::Ready),
        MediaKind::Audio => audio_thumbs(&abs, &lg, &sm).await,
        MediaKind::Other => Ok(ThumbState::None),
    };
    let (state, error) = match outcome {
        Ok(state) => (state, None),
        Err(err) => {
            tracing::warn!(item_id, error = %err, "thumbnail failed");
            (ThumbState::Failed, Some(err))
        }
    };
    if state != ThumbState::Ready {
        // Never leave an old image behind for a file that now has none.
        let _ = tokio::fs::remove_file(&lg).await;
        let _ = tokio::fs::remove_file(&sm).await;
    }

    sqlx::query("UPDATE items SET thumb_state = ?, updated_at = ? WHERE id = ?")
        .bind(state.as_str())
        .bind(now())
        .bind(item_id)
        .execute(&media.db)
        .await?;
    publish_updated(media, item_id);
    match error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

pub async fn photo_view(media: &MediaService, item: &ItemRow) -> Result<PathBuf> {
    if item.kind != MediaKind::Photo {
        return Err(CoreError::NotFound);
    }
    let abs = media.layout.abs(&item.rel_path)?;
    let source = tokio::fs::metadata(&abs).await.map_err(|_| CoreError::NotFound)?;
    if BROWSER_IMAGE_EXTENSIONS.contains(&extension(&abs).as_str()) {
        return Ok(abs);
    }

    let rendition = media.rendition_path(&item.id);
    if let Ok(existing) = tokio::fs::metadata(&rendition).await {
        let fresh = match (existing.modified(), source.modified()) {
            (Ok(made), Ok(changed)) => made >= changed,
            _ => true,
        };
        if fresh && existing.len() > 0 {
            return Ok(rendition);
        }
    }
    if let Some(dir) = rendition.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let made = if tools::has_vips().await {
        vips_resize(&abs, &rendition, RENDITION_EDGE, RENDITION_QUALITY).await
    } else {
        Err(CoreError::external("vipsthumbnail is not installed on this server."))
    };
    if let Err(err) = made {
        tracing::debug!(item_id = %item.id, error = %err, "vips could not make a rendition; trying ffmpeg");
        ffmpeg_still(&abs, None, None, &filters::fit_box(RENDITION_EDGE), RENDITION_QUALITY, &rendition)
            .await
            .map_err(|_| CoreError::external("This photo could not be converted for the browser."))?;
    }
    Ok(rendition)
}

// ---------------------------------------------------------------------------
// Video, photo and audio thumbnails

/// Where to grab the poster frame: 10% in, at least 3 s, but never past the middle of a short clip.
pub fn thumb_seek(duration_s: Option<f64>) -> f64 {
    match duration_s {
        Some(d) if d.is_finite() && d > 0.0 => (d * 0.1).max(3.0).min(d * 0.5),
        _ => 3.0,
    }
}

async fn video_thumbs(abs: &Path, item: &ItemRow, lg: &Path, sm: &Path) -> Result<()> {
    let summary = item.media().unwrap_or_default();
    let stream = summary.video.as_ref().map(|v| v.index);
    let hdr = summary.video.as_ref().and_then(|v| v.hdr.clone());
    let seek = thumb_seek(item.duration_s.or(summary.duration_s));
    let fit = filters::fit_box(ThumbSize::Lg.long_edge());
    let first = match &hdr {
        Some(kind) => format!("{},{fit}", filters::tonemap_chain(kind)),
        None => fit.clone(),
    };
    if let Err(err) = ffmpeg_still(abs, Some(seek), stream, &first, THUMB_QUALITY, lg).await {
        // The clip may be shorter than its metadata claims, or tone mapping may be unavailable.
        tracing::debug!(item_id = %item.id, error = %err, "first frame grab failed; retrying from the start");
        ffmpeg_still(abs, None, stream, &fit, THUMB_QUALITY, lg).await?;
    }
    ffmpeg_still(lg, None, None, &filters::fit_box(ThumbSize::Sm.long_edge()), THUMB_QUALITY, sm).await
}

async fn photo_thumbs(abs: &Path, lg: &Path, sm: &Path) -> Result<()> {
    if tools::has_vips().await {
        match vips_resize(abs, lg, ThumbSize::Lg.long_edge(), THUMB_QUALITY).await {
            Ok(()) => {
                if vips_resize(lg, sm, ThumbSize::Sm.long_edge(), THUMB_QUALITY).await.is_ok() {
                    return Ok(());
                }
                return ffmpeg_still(lg, None, None, &filters::fit_box(ThumbSize::Sm.long_edge()), THUMB_QUALITY, sm)
                    .await;
            }
            Err(err) => tracing::debug!(error = %err, "vips could not read the photo; trying ffmpeg"),
        }
    }
    ffmpeg_still(abs, None, None, &filters::fit_box(ThumbSize::Lg.long_edge()), THUMB_QUALITY, lg).await?;
    ffmpeg_still(lg, None, None, &filters::fit_box(ThumbSize::Sm.long_edge()), THUMB_QUALITY, sm).await
}

/// Embedded cover art becomes the thumbnail; no cover means no thumbnail.
async fn audio_thumbs(abs: &Path, lg: &Path, sm: &Path) -> Result<ThumbState> {
    let fit = filters::fit_box(ThumbSize::Lg.long_edge());
    if ffmpeg_still(abs, None, None, &fit, THUMB_QUALITY, lg).await.is_err() {
        return Ok(ThumbState::None);
    }
    ffmpeg_still(lg, None, None, &filters::fit_box(ThumbSize::Sm.long_edge()), THUMB_QUALITY, sm).await?;
    Ok(ThumbState::Ready)
}

/// A temporary sibling of `dest` with a `.webp` extension, renamed into place when complete.
fn temp_sibling(dest: &Path) -> PathBuf {
    let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    dest.with_file_name(format!(".{name}.{:016x}.tmp.webp", rand::random::<u64>()))
}

/// Move a finished temp file into place, or remove it when it is empty or missing.
async fn finish_temp(tmp: &Path, dest: &Path) -> Result<()> {
    match tokio::fs::metadata(tmp).await {
        Ok(meta) if meta.len() > 0 => {
            tokio::fs::rename(tmp, dest).await?;
            Ok(())
        }
        _ => {
            let _ = tokio::fs::remove_file(tmp).await;
            Err(CoreError::external("No image was produced."))
        }
    }
}

/// One frame of `src` through `filter`, written as WebP to `dest`.
async fn ffmpeg_still(
    src: &Path,
    seek: Option<f64>,
    stream: Option<i64>,
    filter: &str,
    quality: u32,
    dest: &Path,
) -> Result<()> {
    let tmp = temp_sibling(dest);
    let mut cmd = tools::command(tools::FFMPEG);
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]);
    if let Some(seek) = seek.filter(|s| *s > 0.0) {
        cmd.arg("-ss").arg(format!("{seek:.3}"));
    }
    cmd.arg("-i").arg(tools::ffmpeg_input(src));
    match stream {
        Some(index) => cmd.arg("-map").arg(format!("0:{index}")),
        None => cmd.args(["-map", "0:v:0"]),
    };
    cmd.args(["-frames:v", "1", "-an", "-sn", "-dn", "-map_metadata", "-1", "-vf", filter])
        .args(["-c:v", "libwebp", "-quality", &quality.to_string(), "-f", "webp"])
        .arg(&tmp);
    let outcome = tools::run_ok(cmd, tools::FFMPEG, IMAGE_TIMEOUT).await;
    if let Err(err) = outcome {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(err);
    }
    finish_temp(&tmp, dest).await
}

/// `vipsthumbnail` with EXIF auto-rotation, shrinking (never enlarging) into a `edge`×`edge` box.
async fn vips_resize(src: &Path, dest: &Path, edge: u32, quality: u32) -> Result<()> {
    let tmp = temp_sibling(dest);
    let mut out = tmp.as_os_str().to_owned();
    out.push(format!("[Q={quality},strip]"));
    let mut cmd = tools::command(tools::VIPSTHUMBNAIL);
    cmd.arg(src).arg("--size").arg(format!("{edge}x{edge}>")).arg("-o").arg(out);
    let outcome = tools::run_ok(cmd, tools::VIPSTHUMBNAIL, IMAGE_TIMEOUT).await;
    if let Err(err) = outcome {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(err);
    }
    finish_temp(&tmp, dest).await
}

/// Stored pixel size of an image, via vipsheader, then ffprobe.
async fn image_size(abs: &Path) -> Option<(i64, i64)> {
    if tools::has_vips().await {
        let mut cmd = tools::command(tools::VIPSHEADER);
        cmd.arg(abs);
        if let Ok(output) = tools::run_ok(cmd, tools::VIPSHEADER, Duration::from_secs(30)).await {
            if let Some(size) = parse_vipsheader(&String::from_utf8_lossy(&output.stdout)) {
                return Some(size);
            }
        }
    }
    let summary = probe::probe(abs).await.ok()?;
    let video = summary.video?;
    (video.width > 0 && video.height > 0).then_some((video.width, video.height))
}

/// `photo.jpg: 640x480 uchar, 3 bands, srgb, jpegload` → (640, 480).
fn parse_vipsheader(out: &str) -> Option<(i64, i64)> {
    let line = out.lines().next()?;
    line.split_whitespace().rev().find_map(|token| {
        let (w, h) = token.split_once('x')?;
        let (w, h) = (w.parse::<i64>().ok()?, h.parse::<i64>().ok()?);
        (w > 0 && h > 0).then_some((w, h))
    })
}

// ---------------------------------------------------------------------------
// Metadata helpers

/// Width and height as shown on screen (swapped for 90°/270° rotation).
pub fn display_size(summary: &MediaSummary) -> (Option<i64>, Option<i64>) {
    match &summary.video {
        Some(v) if v.width > 0 && v.height > 0 => {
            if v.rotation == 90 || v.rotation == 270 {
                (Some(v.height), Some(v.width))
            } else {
                (Some(v.width), Some(v.height))
            }
        }
        _ => (None, None),
    }
}

/// "Apple iPhone 15 Pro" from the probe tags.
pub fn camera_from_tags(tags: &BTreeMap<String, String>) -> Option<String> {
    let get = |keys: &[&str]| keys.iter().find_map(|k| tags.get(*k)).map(String::as_str);
    exif::camera_name(
        get(&["make", "com.apple.quicktime.make", "com.android.manufacturer"]),
        get(&["model", "com.apple.quicktime.model", "com.android.model"]),
    )
}

/// When the clip was recorded, from QuickTime/Matroska tags.
pub fn taken_from_tags(tags: &BTreeMap<String, String>) -> Option<i64> {
    ["com.apple.quicktime.creationdate", "creation_time", "date"]
        .iter()
        .filter_map(|k| tags.get(*k))
        .find_map(|v| parse_tag_time(v))
}

/// `2024-06-13T10:21:00.000000Z`, `2024-06-13T12:21:00+0200`, `2024-06-13 10:21:00`,
/// `2024:06:13 10:21:00` or `2024-06-13` → unix seconds (no offset means UTC).
pub fn parse_tag_time(value: &str) -> Option<i64> {
    let s = value.trim();
    let num = |from: usize, to: usize| s.get(from..to).and_then(|t| t.parse::<u32>().ok());
    let sep = s.as_bytes().get(4).copied()?;
    if !matches!(sep, b'-' | b':') || s.as_bytes().get(7) != Some(&sep) {
        return None;
    }
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (mut hour, mut minute, mut second) = (0, 0, 0);
    let mut rest = s.get(10..)?;
    if let Some(clock) = rest.strip_prefix(['T', ' ']) {
        let b = clock.as_bytes();
        if b.len() < 5 || b[2] != b':' {
            return None;
        }
        hour = clock.get(0..2)?.parse().ok()?;
        minute = clock.get(3..5)?.parse().ok()?;
        rest = clock.get(5..)?;
        if let Some(sec) = rest.strip_prefix(':') {
            second = sec.get(0..2)?.parse().ok()?;
            rest = sec.get(2..)?;
        }
        if let Some(frac) = rest.strip_prefix(['.', ',']) {
            rest = frac.trim_start_matches(|c: char| c.is_ascii_digit());
        }
    }
    let offset = parse_offset(rest.trim())?;
    exif::to_unix(
        i32::try_from(year).ok()?,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
        u8::try_from(hour).ok()?,
        u8::try_from(minute).ok()?,
        u8::try_from(second).ok()?,
        offset,
    )
}

/// `Z`, `` (UTC), `+02:00`, `+0200`, `-05` → minutes east of UTC.
fn parse_offset(s: &str) -> Option<i32> {
    if s.is_empty() || s.eq_ignore_ascii_case("z") || s.eq_ignore_ascii_case("utc") {
        return Some(0);
    }
    let sign = match s.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits: String = s[1..].chars().filter(|c| *c != ':').collect();
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (h, m) = match digits.len() {
        2 => (digits.parse::<i32>().ok()?, 0),
        4 => (digits[..2].parse::<i32>().ok()?, digits[2..].parse::<i32>().ok()?),
        _ => return None,
    };
    (h <= 14 && m < 60).then_some(sign * (h * 60 + m))
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// MIME type from the extension, with fixed answers for media types guessers get wrong.
pub fn guess_mime(path: &Path) -> String {
    let fixed = match extension(path).as_str() {
        "ts" | "m2ts" | "mts" => Some("video/mp2t"),
        "mkv" => Some("video/x-matroska"),
        "mov" => Some("video/quicktime"),
        "m4v" => Some("video/x-m4v"),
        "webm" => Some("video/webm"),
        "heic" => Some("image/heic"),
        "heif" => Some("image/heif"),
        "avif" => Some("image/avif"),
        "dng" => Some("image/x-adobe-dng"),
        "cr2" => Some("image/x-canon-cr2"),
        "cr3" => Some("image/x-canon-cr3"),
        "nef" => Some("image/x-nikon-nef"),
        "arw" => Some("image/x-sony-arw"),
        "orf" => Some("image/x-olympus-orf"),
        "rw2" => Some("image/x-panasonic-rw2"),
        "raf" => Some("image/x-fuji-raf"),
        "m4a" => Some("audio/mp4"),
        "opus" => Some("audio/opus"),
        "flac" => Some("audio/flac"),
        _ => None,
    };
    fixed
        .map(str::to_string)
        .or_else(|| mime_guess::from_path(path).first().map(|m| m.essence_str().to_string()))
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::Arc;

    use super::*;
    use crate::config::{Config, HwAccel};
    use crate::events::EventBus;
    use crate::paths::StashLayout;
    use crate::settings::SettingsStore;

    struct Harness {
        _root: tempfile::TempDir,
        _config_dir: tempfile::TempDir,
        media: MediaService,
    }

    async fn harness() -> Harness {
        let root = tempfile::tempdir().unwrap();
        let config_dir = tempfile::tempdir().unwrap();
        let config = Arc::new(Config::for_tests(root.path(), config_dir.path()));
        let db = crate::db::open_memory().await.unwrap();
        let settings = SettingsStore::load(db.clone(), HwAccel::None).await.unwrap();
        let layout = Arc::new(StashLayout::new(root.path()));
        layout.ensure().await.unwrap();
        let media = MediaService::new(config, db, settings, layout, EventBus::new());
        Harness { _root: root, _config_dir: config_dir, media }
    }

    async fn insert_item(media: &MediaService, rel: &str, kind: MediaKind, category: Category) -> String {
        let id = crate::new_id();
        let t = now();
        sqlx::query(
            "INSERT INTO items (id, rel_path, kind, category, status, title, sort_title, size, mtime, thumb_state, added_at, updated_at)
             VALUES (?, ?, ?, ?, 'ready', 't', 't', 1, ?, 'pending', ?, ?)",
        )
        .bind(&id)
        .bind(rel)
        .bind(kind.as_str())
        .bind(category.as_str())
        .bind(t)
        .bind(t)
        .bind(t)
        .execute(&media.db)
        .await
        .unwrap();
        id
    }

    async fn row(media: &MediaService, id: &str) -> ItemRow {
        load_item(media, id).await.unwrap().unwrap()
    }

    fn ffmpeg(args: &[&str], out: &Path) {
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        let status = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .arg(out)
            .status()
            .unwrap();
        assert!(status.success(), "ffmpeg failed for {}", out.display());
    }

    fn webp_size(path: &Path) -> (i64, i64) {
        let out = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "stream=width,height,codec_name", "-of", "csv=p=0"])
            .arg(path)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let parts: Vec<&str> = text.trim().split(',').collect();
        assert_eq!(parts[0], "webp", "{text}");
        (parts[1].parse().unwrap(), parts[2].parse().unwrap())
    }

    /// iPhone-style portrait clip: 640x360 coded, rotated, Apple tags and a location to drop.
    fn make_phone_clip(path: &Path) {
        let tmp = path.with_extension("tmp.mov");
        ffmpeg(
            &[
                "-f", "lavfi", "-i", "testsrc=s=640x360:r=25:d=6", "-f", "lavfi", "-i", "sine=d=6",
                "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-shortest",
                "-movflags", "use_metadata_tags",
                "-metadata", "com.apple.quicktime.make=Apple",
                "-metadata", "com.apple.quicktime.model=iPhone 15 Pro",
                "-metadata", "com.apple.quicktime.creationdate=2024-06-13T12:21:00+0200",
                "-metadata", "com.apple.quicktime.location.ISO6709=+37.7749-122.4194+010.000/",
                "-f", "mov",
            ],
            &tmp,
        );
        ffmpeg(
            &["-display_rotation", "-90", "-i", tmp.to_str().unwrap(), "-c", "copy", "-movflags", "use_metadata_tags", "-f", "mov"],
            path,
        );
        std::fs::remove_file(tmp).unwrap();
    }

    #[tokio::test]
    async fn video_probe_and_thumbnails() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Home Videos/2024/06/IMG_0001.MOV";
        make_phone_clip(&media.layout.abs(rel).unwrap());
        let id = insert_item(media, rel, MediaKind::Video, Category::HomeVideo).await;
        let mut events = media.events.subscribe();

        media.process_item(&id).await.unwrap();
        let item = row(media, &id).await;
        assert!((item.duration_s.unwrap() - 6.0).abs() < 0.2, "{:?}", item.duration_s);
        assert_eq!((item.width, item.height), (Some(360), Some(640)), "rotation swaps the size");
        assert_eq!(item.mime.as_deref(), Some("video/quicktime"));
        assert_eq!(item.camera.as_deref(), Some("Apple iPhone 15 Pro"));
        assert_eq!(item.taken_at, Some(1_718_274_060));
        let summary = item.media().unwrap();
        assert_eq!(summary.video.as_ref().unwrap().rotation, 90);
        assert!(!item.media_json.as_deref().unwrap().contains("37.7749"), "location must not be stored");
        match events.try_recv().unwrap() {
            Event::Library { r#type, item_id } => {
                assert_eq!(r#type, LibraryChange::Updated);
                assert_eq!(item_id, id);
            }
            other => panic!("unexpected event {other:?}"),
        }

        media.thumbnail_item(&id).await.unwrap();
        let item = row(media, &id).await;
        assert_eq!(item.thumb_state, ThumbState::Ready);
        // Rotated portrait frame, never upscaled past the source.
        assert_eq!(webp_size(&media.thumb_path(&id, ThumbSize::Lg)), (360, 640));
        let (w, h2) = webp_size(&media.thumb_path(&id, ThumbSize::Sm));
        assert_eq!(h2, 400);
        assert!((224..=226).contains(&w), "{w}");
        // No temp files left behind.
        let dir = media.thumb_path(&id, ThumbSize::Sm).parent().unwrap().to_path_buf();
        assert!(std::fs::read_dir(dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains(".tmp")));
    }

    #[tokio::test]
    async fn movie_keeps_taken_at_empty_and_short_clips_work() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Movies/Clip (2020)/Clip (2020).mkv";
        ffmpeg(
            &["-f", "lavfi", "-i", "testsrc=s=1920x1080:r=25:d=1.5", "-c:v", "libx264", "-preset", "ultrafast",
              "-metadata", "creation_time=2020-01-01T00:00:00Z"],
            &media.layout.abs(rel).unwrap(),
        );
        let id = insert_item(media, rel, MediaKind::Video, Category::Movie).await;
        media.process_item(&id).await.unwrap();
        let item = row(media, &id).await;
        assert_eq!(item.taken_at, None);
        assert_eq!(item.mime.as_deref(), Some("video/x-matroska"));
        assert_eq!((item.width, item.height), (Some(1920), Some(1080)));
        media.thumbnail_item(&id).await.unwrap();
        assert_eq!(webp_size(&media.thumb_path(&id, ThumbSize::Lg)), (1280, 720));
        assert_eq!(webp_size(&media.thumb_path(&id, ThumbSize::Sm)), (400, 225));
    }

    #[tokio::test]
    async fn hdr_video_thumbnail_is_tone_mapped() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Home Videos/hdr.mp4";
        let encoders = Command::new("ffmpeg").args(["-hide_banner", "-encoders"]).output().unwrap();
        if !String::from_utf8_lossy(&encoders.stdout).contains("libx265") {
            eprintln!("libx265 missing; skipping");
            return;
        }
        ffmpeg(
            &["-f", "lavfi", "-i", "testsrc=s=640x360:r=25:d=2", "-pix_fmt", "yuv420p10le", "-c:v", "libx265",
              "-x265-params", "log-level=error:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc",
              "-tag:v", "hvc1"],
            &media.layout.abs(rel).unwrap(),
        );
        let id = insert_item(media, rel, MediaKind::Video, Category::HomeVideo).await;
        media.process_item(&id).await.unwrap();
        let summary = row(media, &id).await.media().unwrap();
        assert_eq!(summary.video.as_ref().unwrap().hdr.as_deref(), Some("hdr10"));
        media.thumbnail_item(&id).await.unwrap();
        assert_eq!(row(media, &id).await.thumb_state, ThumbState::Ready);
        assert_eq!(webp_size(&media.thumb_path(&id, ThumbSize::Lg)), (640, 360));
    }

    #[tokio::test]
    async fn photo_probe_thumbnails_and_view() {
        let h = harness().await;
        let media = &h.media;
        // A 2000x1000 JPEG whose EXIF says "rotate 90° clockwise".
        let rel = "Photos/2024/06/IMG_0002.jpg";
        let abs = media.layout.abs(rel).unwrap();
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        exif::tests::make_jpeg(&abs, "2000x1000");
        let jpeg = std::fs::read(&abs).unwrap();
        let spec = exif::tests::TestExif { width: Some(2000), height: Some(1000), ..Default::default() };
        std::fs::write(&abs, exif::tests::inject_into_jpeg(&jpeg, &exif::tests::tiff_block(&spec))).unwrap();
        let id = insert_item(media, rel, MediaKind::Photo, Category::Photo).await;

        media.process_item(&id).await.unwrap();
        let item = row(media, &id).await;
        assert_eq!((item.width, item.height), (Some(1000), Some(2000)));
        assert_eq!(item.taken_at, Some(1_718_274_060));
        assert_eq!(item.camera.as_deref(), Some("Apple iPhone 15 Pro"));
        assert_eq!(item.mime.as_deref(), Some("image/jpeg"));
        assert_eq!(item.media_json, None);

        media.thumbnail_item(&id).await.unwrap();
        let item = row(media, &id).await;
        assert_eq!(item.thumb_state, ThumbState::Ready);
        assert_eq!(webp_size(&media.thumb_path(&id, ThumbSize::Lg)), (640, 1280));
        assert_eq!(webp_size(&media.thumb_path(&id, ThumbSize::Sm)), (200, 400));

        // Browsers show JPEG directly.
        assert_eq!(media.photo_view(&item).await.unwrap(), abs);
    }

    #[tokio::test]
    async fn photo_without_exif_gets_size_from_the_image() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Photos/screenshot.png";
        ffmpeg(&["-f", "lavfi", "-i", "testsrc=s=300x200:d=1", "-frames:v", "1"], &media.layout.abs(rel).unwrap());
        let id = insert_item(media, rel, MediaKind::Photo, Category::Photo).await;
        sqlx::query("UPDATE items SET taken_at = 123 WHERE id = ?").bind(&id).execute(&media.db).await.unwrap();
        media.process_item(&id).await.unwrap();
        let item = row(media, &id).await;
        assert_eq!((item.width, item.height), (Some(300), Some(200)));
        assert_eq!(item.mime.as_deref(), Some("image/png"));
        assert_eq!(item.taken_at, Some(123), "an existing date is kept");
        media.thumbnail_item(&id).await.unwrap();
        assert_eq!(webp_size(&media.thumb_path(&id, ThumbSize::Lg)), (300, 200), "small images are not enlarged");
    }

    #[tokio::test]
    async fn tiff_gets_a_cached_rendition() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Photos/scan.tif";
        ffmpeg(&["-f", "lavfi", "-i", "testsrc=s=3000x2000:d=1", "-frames:v", "1"], &media.layout.abs(rel).unwrap());
        let id = insert_item(media, rel, MediaKind::Photo, Category::Photo).await;
        let item = row(media, &id).await;
        let view = media.photo_view(&item).await.unwrap();
        assert_eq!(view, media.rendition_path(&id));
        assert_eq!(webp_size(&view), (2560, 1707));
        let made = std::fs::metadata(&view).unwrap().modified().unwrap();
        // Second call reuses the cached file.
        assert_eq!(media.photo_view(&item).await.unwrap(), view);
        assert_eq!(std::fs::metadata(&view).unwrap().modified().unwrap(), made);
        // Videos have no photo view.
        let vid = insert_item(media, "Other/a.mkv", MediaKind::Video, Category::Other).await;
        assert!(matches!(media.photo_view(&row(media, &vid).await).await, Err(CoreError::NotFound)));
    }

    #[tokio::test]
    async fn audio_cover_art_or_none() {
        let h = harness().await;
        let media = &h.media;
        let cover = media.layout.abs("Other/cover.png").unwrap();
        ffmpeg(&["-f", "lavfi", "-i", "testsrc=s=500x500:d=1", "-frames:v", "1"], &cover);
        let with_cover = "Other/song.mp3";
        ffmpeg(
            &["-f", "lavfi", "-i", "sine=d=2", "-i", cover.to_str().unwrap(), "-map", "0", "-map", "1",
              "-c:a", "libmp3lame", "-c:v", "png", "-disposition:v", "attached_pic", "-id3v2_version", "3"],
            &media.layout.abs(with_cover).unwrap(),
        );
        let plain = "Other/plain.mp3";
        ffmpeg(&["-f", "lavfi", "-i", "sine=d=2", "-c:a", "libmp3lame"], &media.layout.abs(plain).unwrap());

        let a = insert_item(media, with_cover, MediaKind::Audio, Category::Other).await;
        media.process_item(&a).await.unwrap();
        let item = row(media, &a).await;
        assert_eq!(item.mime.as_deref(), Some("audio/mpeg"));
        assert!(item.media().unwrap().video.is_none());
        assert_eq!((item.width, item.height), (None, None));
        media.thumbnail_item(&a).await.unwrap();
        assert_eq!(row(media, &a).await.thumb_state, ThumbState::Ready);
        assert_eq!(webp_size(&media.thumb_path(&a, ThumbSize::Sm)), (400, 400));

        let b = insert_item(media, plain, MediaKind::Audio, Category::Other).await;
        media.thumbnail_item(&b).await.unwrap();
        assert_eq!(row(media, &b).await.thumb_state, ThumbState::None);
        assert!(!media.thumb_path(&b, ThumbSize::Lg).exists());
    }

    #[tokio::test]
    async fn other_files_and_failures() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Other/notes.pdf";
        std::fs::write(media.layout.abs(rel).unwrap(), b"%PDF-1.4").unwrap();
        let id = insert_item(media, rel, MediaKind::Other, Category::Other).await;
        media.process_item(&id).await.unwrap();
        assert_eq!(row(media, &id).await.mime.as_deref(), Some("application/pdf"));
        media.thumbnail_item(&id).await.unwrap();
        assert_eq!(row(media, &id).await.thumb_state, ThumbState::None);

        // A "video" that is not one: probe fails, thumbnail is marked failed.
        let bad = "Other/broken.mp4";
        std::fs::write(media.layout.abs(bad).unwrap(), b"not really a video").unwrap();
        let id = insert_item(media, bad, MediaKind::Video, Category::Other).await;
        assert!(media.process_item(&id).await.is_err());
        assert!(media.thumbnail_item(&id).await.is_err());
        assert_eq!(row(media, &id).await.thumb_state, ThumbState::Failed);

        // Gone items and vanished files are skipped quietly.
        media.process_item("no-such-item").await.unwrap();
        media.thumbnail_item("no-such-item").await.unwrap();
        let gone = insert_item(media, "Other/gone.mp4", MediaKind::Video, Category::Other).await;
        media.process_item(&gone).await.unwrap();
        media.thumbnail_item(&gone).await.unwrap();
        assert_eq!(row(media, &gone).await.thumb_state, ThumbState::Pending);
    }

    #[test]
    fn seek_points() {
        assert_eq!(thumb_seek(None), 3.0);
        assert_eq!(thumb_seek(Some(600.0)), 60.0);
        assert_eq!(thumb_seek(Some(20.0)), 3.0);
        assert_eq!(thumb_seek(Some(4.0)), 2.0);
        assert_eq!(thumb_seek(Some(0.0)), 3.0);
    }

    #[test]
    fn tag_times() {
        assert_eq!(parse_tag_time("2024-06-13T10:21:00.000000Z"), Some(1_718_274_060));
        assert_eq!(parse_tag_time("2024-06-13T12:21:00+0200"), Some(1_718_274_060));
        assert_eq!(parse_tag_time("2024-06-13T12:21:00+02:00"), Some(1_718_274_060));
        assert_eq!(parse_tag_time("2024-06-13T05:21:00-05"), Some(1_718_274_060));
        assert_eq!(parse_tag_time("2024-06-13 10:21:00"), Some(1_718_274_060));
        assert_eq!(parse_tag_time("2024:06:13 10:21:00"), Some(1_718_274_060));
        assert_eq!(parse_tag_time("2024-06-13"), Some(1_718_236_800));
        assert_eq!(parse_tag_time("1970-01-01T00:00:00.000000Z"), None);
        assert_eq!(parse_tag_time("1904-01-01T00:00:00Z"), None);
        assert_eq!(parse_tag_time("2001"), None);
        assert_eq!(parse_tag_time("June 13, 2024"), None);
        assert_eq!(parse_tag_time("2024-06-13T10:21:00 nonsense"), None);
        assert_eq!(parse_tag_time("2024-06-13T1"), None);
        assert_eq!(parse_tag_time("２０２４-06-13"), None);
    }

    #[test]
    fn tags_to_camera_and_date() {
        let mut tags = BTreeMap::new();
        assert_eq!(camera_from_tags(&tags), None);
        assert_eq!(taken_from_tags(&tags), None);
        tags.insert("com.android.manufacturer".to_string(), "Google".to_string());
        tags.insert("com.android.model".to_string(), "Pixel 8".to_string());
        tags.insert("creation_time".to_string(), "2024-06-13T10:21:00.000000Z".to_string());
        assert_eq!(camera_from_tags(&tags).as_deref(), Some("Google Pixel 8"));
        assert_eq!(taken_from_tags(&tags), Some(1_718_274_060));
        // The Apple local date wins over the UTC creation_time when both exist.
        tags.insert("com.apple.quicktime.creationdate".to_string(), "2024-06-13T12:00:00+0200".to_string());
        assert_eq!(taken_from_tags(&tags), Some(1_718_272_800));
    }

    #[test]
    fn mime_types() {
        assert_eq!(guess_mime(Path::new("a.MKV")), "video/x-matroska");
        assert_eq!(guess_mime(Path::new("a.ts")), "video/mp2t");
        assert_eq!(guess_mime(Path::new("a.mp4")), "video/mp4");
        assert_eq!(guess_mime(Path::new("a.HEIC")), "image/heic");
        assert_eq!(guess_mime(Path::new("a.jpg")), "image/jpeg");
        assert_eq!(guess_mime(Path::new("a.unknownext")), "application/octet-stream");
    }

    #[test]
    fn vipsheader_output() {
        assert_eq!(parse_vipsheader("a.jpg: 640x480 uchar, 3 bands, srgb, jpegload\n"), Some((640, 480)));
        assert_eq!(parse_vipsheader("my 2x2: file.png: 30x20 uchar, 4 bands, srgb, pngload"), Some((30, 20)));
        assert_eq!(parse_vipsheader(""), None);
    }

    #[test]
    fn display_sizes() {
        let mut summary = MediaSummary {
            video: Some(crate::models::VideoStream { width: 1920, height: 1080, rotation: 270, ..Default::default() }),
            ..Default::default()
        };
        assert_eq!(display_size(&summary), (Some(1080), Some(1920)));
        summary.video.as_mut().unwrap().rotation = 180;
        assert_eq!(display_size(&summary), (Some(1920), Some(1080)));
        assert_eq!(display_size(&MediaSummary::default()), (None, None));
    }
}
