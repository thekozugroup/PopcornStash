//! Probe/thumbnail job handlers and photo renditions.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::tz::TimeZone;

use super::{MediaService, ThumbSize, exif, filters, probe, tools};
use crate::capture::{self, CaptureTime, Recorded};
use crate::events::{Event, LibraryChange};
use crate::library::parse::parse_name;
use crate::models::{Category, ItemRow, ItemStatus, MediaKind, MediaSummary, ThumbState};
use crate::{CoreError, Result, now};

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
    Ok(
        sqlx::query_as::<_, ItemRow>("SELECT * FROM items WHERE id = ?")
            .bind(item_id)
            .fetch_optional(&media.db)
            .await?,
    )
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
    media.events.publish(Event::Library {
        r#type: LibraryChange::Updated,
        item_id: item_id.to_string(),
        before: None,
    });
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
    /// The capture time the file's own metadata gives (EXIF, the clip's tags).
    recorded: Option<Recorded>,
}

/// How the probe job settles the capture time (section 4, Capture time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dating {
    /// The file's metadata, else the time already stored (from the name or the file time
    /// when the item was indexed), else the name, else the file time.
    Keep,
    /// Read everything again, once, for rows dated before migration 0003: the metadata,
    /// else the name, else the stored moment (a file time) read in the server's zone.
    Redate,
}

/// The capture time a probed item gets. Movies and episodes have none (their creation
/// time is the encode date); other kinds keep a stored time unless the metadata has one.
/// A photo or home video's UTC creation time is paired with the clock its camera-style
/// name writes when the two agree (`capture::with_name_clock`, with `item.duration_s`).
pub fn capture_time(
    item: &ItemRow,
    recorded: Option<Recorded>,
    dating: Dating,
    zone: &TimeZone,
) -> Option<CaptureTime> {
    let stored = item.taken();
    match item.category {
        Category::Movie | Category::Episode => stored,
        Category::Other => recorded.and_then(|r| r.resolve(zone)).or(stored),
        Category::Photo | Category::HomeVideo => {
            let stem = std::path::Path::new(&item.rel_path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let parsed = parse_name(&stem);
            let named = parsed.date;
            let file_time = Recorded::instant(item.mtime);
            // A UTC creation time beside the phone's clock in the name gives both (Android).
            let recorded = capture::with_name_clock(recorded, parsed.camera_clock, item.duration_s);
            match dating {
                Dating::Keep => recorded
                    .and_then(|r| r.resolve(zone))
                    .or(stored)
                    .or_else(|| capture::first_of([named, file_time], zone)),
                Dating::Redate => capture::first_of(
                    [
                        recorded,
                        named,
                        stored.and_then(|t| Recorded::instant(t.at)),
                        file_time,
                    ],
                    zone,
                ),
            }
        }
    }
}

pub async fn process_item(media: &MediaService, item_id: &str, dating: Dating) -> Result<()> {
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
                result.recorded = taken_from_tags(&summary.tags);
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
            if (width.is_none() || height.is_none())
                && let Some((w, h)) = image_size(&abs).await
            {
                // HEIF decoders apply the rotation themselves; other formats report stored pixels.
                let decoder_rotates = matches!(extension(&abs).as_str(), "heic" | "heif" | "avif");
                (width, height) = if info.swaps_dimensions() && !decoder_rotates {
                    (Some(h), Some(w))
                } else {
                    (Some(w), Some(h))
                };
            }
            result.width = width;
            result.height = height;
            result.camera = info.camera;
            result.recorded = info.taken;
        }
        MediaKind::Other => {}
    }

    // The capture date from EXIF or the clip's own tags beats whatever the scanner had
    // (a date in the name or the file's modification time), as section 4 orders them. The
    // clip's length is the one just read.
    let probed = ItemRow {
        duration_s: result.duration_s,
        ..item.clone()
    };
    let taken = capture_time(&probed, result.recorded, dating, &capture::server_zone());
    let camera = result.camera.clone().or_else(|| item.camera.clone());
    // A waiting file's suggestion carries the same capture time, so where it would go
    // (its `YYYY/MM/` folder) follows what the probe learned.
    let suggestion = match item.suggestion() {
        Some(mut c)
            if matches!(c.category, Category::Photo | Category::HomeVideo)
                && taken.is_some()
                && c.taken() != taken =>
        {
            c.set_taken(taken);
            Some(serde_json::to_string(&c)?)
        }
        _ => item.suggestion_json.clone(),
    };
    let unchanged = result.mime == item.mime
        && result.duration_s == item.duration_s
        && result.width == item.width
        && result.height == item.height
        && result.media_json == item.media_json
        && camera == item.camera
        && taken == item.taken()
        && item.taken_local.is_some() == item.taken_at.is_some()
        && suggestion == item.suggestion_json;
    if unchanged {
        // Nothing new (a second probe, or a capture time that was already right): no
        // write, no event.
        return Ok(());
    }
    let (taken_at, taken_local, taken_offset) = CaptureTime::columns(taken.as_ref());
    sqlx::query(
        "UPDATE items SET mime = ?, duration_s = ?, width = ?, height = ?, media_json = ?,
                camera = ?, taken_at = ?, taken_local = ?, taken_offset = ?, suggestion_json = ?,
                updated_at = ?
         WHERE id = ?",
    )
    .bind(&result.mime)
    .bind(result.duration_s)
    .bind(result.width)
    .bind(result.height)
    .bind(&result.media_json)
    .bind(&camera)
    .bind(taken_at)
    .bind(&taken_local)
    .bind(taken_offset)
    .bind(&suggestion)
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
        MediaKind::Video => video_thumbs(&abs, &item, &lg, &sm)
            .await
            .map(|()| ThumbState::Ready),
        MediaKind::Photo => {
            let made = photo_thumbs(&abs, &lg, &sm).await;
            if made.is_ok() && needs_rendition(&abs) {
                // Prepare the browser copy now so opening the photo is instant. A failure
                // here is not a thumbnail failure; the view request will try again.
                let prepared = match tokio::fs::metadata(&abs).await {
                    Ok(source) => ensure_rendition(media, item_id, &abs, &source)
                        .await
                        .map(|_| ()),
                    Err(err) => Err(err.into()),
                };
                if let Err(err) = prepared {
                    tracing::warn!(item_id, error = %err, "could not prepare the photo for browsers");
                }
            }
            made.map(|()| ThumbState::Ready)
        }
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
    let source = tokio::fs::metadata(&abs)
        .await
        .map_err(|_| CoreError::NotFound)?;
    if !needs_rendition(&abs) {
        return Ok(abs);
    }
    ensure_rendition(media, &item.id, &abs, &source).await
}

/// HEIC, RAW and TIFF photos need a browser-friendly copy; the rest are shown as they are.
fn needs_rendition(abs: &Path) -> bool {
    !BROWSER_IMAGE_EXTENSIONS.contains(&extension(abs).as_str())
}

/// Decoding a 50-megapixel HEIC or RAW takes real memory and CPU, and a viewer can ask
/// for many photos at once: only this many renditions are made at the same time.
static RENDITION_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// The cached rendition when it is newer than the photo.
async fn fresh_rendition(rendition: &Path, source: &std::fs::Metadata) -> bool {
    match tokio::fs::metadata(rendition).await {
        Ok(existing) if existing.len() > 0 => match (existing.modified(), source.modified()) {
            (Ok(made), Ok(changed)) => made >= changed,
            _ => true,
        },
        _ => false,
    }
}

/// The photo's rendition, made now unless a fresh one is cached.
async fn ensure_rendition(
    media: &MediaService,
    item_id: &str,
    abs: &Path,
    source: &std::fs::Metadata,
) -> Result<PathBuf> {
    let rendition = media.rendition_path(item_id);
    if fresh_rendition(&rendition, source).await {
        return Ok(rendition);
    }
    let _slot = RENDITION_SLOTS
        .acquire()
        .await
        .map_err(|_| CoreError::external("Photo conversion is shutting down."))?;
    // Another request may have made it while this one waited for a slot.
    if fresh_rendition(&rendition, source).await {
        return Ok(rendition);
    }
    if let Some(dir) = rendition.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let made = if tools::has_vips().await {
        vips_resize(abs, &rendition, RENDITION_EDGE, RENDITION_QUALITY).await
    } else {
        Err(CoreError::external(
            "vipsthumbnail is not installed on this server.",
        ))
    };
    if let Err(err) = made {
        tracing::debug!(item_id, error = %err, "vips could not make a rendition; trying ffmpeg");
        ffmpeg_still(
            abs,
            None,
            None,
            &filters::fit_box(RENDITION_EDGE),
            RENDITION_QUALITY,
            &rendition,
        )
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
    // Probe and thumbnail jobs run in separate pools, so the probe may not have filled
    // `media_json` yet. Without it an HDR clip would get a washed-out, untone-mapped poster.
    let summary = match item.media() {
        Some(summary) => summary,
        None => probe::probe(abs).await.unwrap_or_else(|err| {
            tracing::debug!(item_id = %item.id, error = %err, "probe for the thumbnail failed");
            MediaSummary::default()
        }),
    };
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
    ffmpeg_still(
        lg,
        None,
        None,
        &filters::fit_box(ThumbSize::Sm.long_edge()),
        THUMB_QUALITY,
        sm,
    )
    .await
}

async fn photo_thumbs(abs: &Path, lg: &Path, sm: &Path) -> Result<()> {
    if tools::has_vips().await {
        match vips_resize(abs, lg, ThumbSize::Lg.long_edge(), THUMB_QUALITY).await {
            Ok(()) => {
                if vips_resize(lg, sm, ThumbSize::Sm.long_edge(), THUMB_QUALITY)
                    .await
                    .is_ok()
                {
                    return Ok(());
                }
                return ffmpeg_still(
                    lg,
                    None,
                    None,
                    &filters::fit_box(ThumbSize::Sm.long_edge()),
                    THUMB_QUALITY,
                    sm,
                )
                .await;
            }
            Err(err) => {
                tracing::debug!(error = %err, "vips could not read the photo; trying ffmpeg")
            }
        }
    }
    ffmpeg_still(
        abs,
        None,
        None,
        &filters::fit_box(ThumbSize::Lg.long_edge()),
        THUMB_QUALITY,
        lg,
    )
    .await?;
    ffmpeg_still(
        lg,
        None,
        None,
        &filters::fit_box(ThumbSize::Sm.long_edge()),
        THUMB_QUALITY,
        sm,
    )
    .await
}

/// Embedded cover art becomes the thumbnail; no cover means no thumbnail.
async fn audio_thumbs(abs: &Path, lg: &Path, sm: &Path) -> Result<ThumbState> {
    let fit = filters::fit_box(ThumbSize::Lg.long_edge());
    if ffmpeg_still(abs, None, None, &fit, THUMB_QUALITY, lg)
        .await
        .is_err()
    {
        return Ok(ThumbState::None);
    }
    ffmpeg_still(
        lg,
        None,
        None,
        &filters::fit_box(ThumbSize::Sm.long_edge()),
        THUMB_QUALITY,
        sm,
    )
    .await?;
    Ok(ThumbState::Ready)
}

/// A temporary sibling of `dest` with a `.webp` extension, renamed into place when complete.
fn temp_sibling(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dest.with_file_name(format!(".{name}.{:016x}.tmp.webp", rand::random::<u64>()))
}

/// Whether a file name is one of [`temp_sibling`]'s: `.<name>.<16 hex>.tmp.webp`.
fn is_temp_name(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix('.')
        .and_then(|n| n.strip_suffix(".tmp.webp"))
    else {
        return false;
    };
    rest.rsplit_once('.').is_some_and(|(stem, hex)| {
        !stem.is_empty() && hex.len() == 16 && hex.bytes().all(|b| b.is_ascii_hexdigit())
    })
}

/// The file a tool writes before it is moved into place (see [`temp_sibling`]). It is
/// removed when dropped unless it was moved into place: a photo view the browser
/// cancels drops the conversion midway (the tool is killed with it, see
/// [`tools::command`]), and nothing else would remove the partial file.
struct TempOutput {
    path: PathBuf,
    placed: bool,
}

impl TempOutput {
    fn next_to(dest: &Path) -> Self {
        Self {
            path: temp_sibling(dest),
            placed: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempOutput {
    fn drop(&mut self) {
        if !self.placed {
            // Best effort and quick: one unlink, also when the file was never made.
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Remove unfinished thumbnails and renditions under `dirs` (and their subfolders),
/// left by a crash or power cut mid-conversion. Only call it while nothing converts,
/// that is on start. Returns how many were removed.
pub async fn remove_temp_files(dirs: &[PathBuf]) -> u64 {
    let dirs = dirs.to_vec();
    tokio::task::spawn_blocking(move || {
        let mut removed = 0;
        for dir in dirs {
            for entry in walkdir::WalkDir::new(&dir)
                .follow_links(false)
                .into_iter()
                .flatten()
            {
                if entry.file_type().is_file()
                    && is_temp_name(&entry.file_name().to_string_lossy())
                    && std::fs::remove_file(entry.path()).is_ok()
                {
                    removed += 1;
                }
            }
        }
        removed
    })
    .await
    .unwrap_or(0)
}

/// Run a tool that writes `tmp`, then move its output into `dest`. On failure, or when
/// this future is dropped, `tmp` is removed.
async fn run_into(
    tmp: TempOutput,
    cmd: tokio::process::Command,
    program: &str,
    dest: &Path,
) -> Result<()> {
    tools::run_ok(cmd, program, IMAGE_TIMEOUT).await?;
    finish_temp(tmp, dest).await
}

/// Move a finished temp file into place, or remove it when it is not a WebP image.
///
/// ffmpeg asked for a frame past the end of a clip exits successfully and leaves a few
/// bytes that are no image at all; accepting those would skip the retry from the start
/// and leave a broken thumbnail behind.
async fn finish_temp(mut tmp: TempOutput, dest: &Path) -> Result<()> {
    if is_webp(tmp.path()).await {
        tokio::fs::rename(tmp.path(), dest).await?;
        tmp.placed = true;
        Ok(())
    } else {
        Err(CoreError::external("No image was produced."))
    }
}

/// `RIFF....WEBP` followed by at least one chunk header.
async fn is_webp(path: &Path) -> bool {
    use tokio::io::AsyncReadExt;
    let Ok(file) = tokio::fs::File::open(path).await else {
        return false;
    };
    let mut head = Vec::with_capacity(20);
    if file.take(20).read_to_end(&mut head).await.is_err() {
        return false;
    }
    head.len() == 20 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP"
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
    let tmp = TempOutput::next_to(dest);
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
    cmd.args([
        "-frames:v",
        "1",
        "-an",
        "-sn",
        "-dn",
        "-map_metadata",
        "-1",
        "-vf",
        filter,
    ])
    .args([
        "-c:v",
        "libwebp",
        "-quality",
        &quality.to_string(),
        "-f",
        "webp",
    ])
    .arg(tmp.path());
    run_into(tmp, cmd, tools::FFMPEG, dest).await
}

/// Camera RAW photos. The TIFF-based ones (DNG, CR2, NEF, ARW) are read by libvips' TIFF
/// loader like any TIFF; the others need ImageMagick (libraw), one of libvips'
/// untrusted loaders, and the image's ImageMagick policy allows only the RAW coders.
const RAW_EXTENSIONS: [&str; 8] = ["dng", "cr2", "cr3", "nef", "arw", "orf", "rw2", "raf"];

/// Whether a file starts like a camera RAW format that only ImageMagick reads: Olympus
/// ORF, Panasonic RW2, Fujifilm RAF or Canon CR3. None of libvips' other loaders claim
/// these starts, so such a file can only reach ImageMagick.
fn starts_like_magick_raw(header: &[u8]) -> bool {
    const STARTS: [&[u8]; 5] = [b"IIRO", b"IIRS", b"MMOR", b"IIU\0", b"FUJIFILMCCD-RAW"];
    STARTS.iter().any(|start| header.starts_with(start))
        || (header.len() >= 12 && &header[4..8] == b"ftyp" && &header[8..12] == b"crx ")
}

/// The first bytes of `path` (fewer if it is shorter or cannot be read).
async fn file_header(path: &Path) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut header = Vec::with_capacity(16);
    if let Ok(file) = tokio::fs::File::open(path).await {
        let _ = file.take(16).read_to_end(&mut header).await;
    }
    header
}

/// A libvips command that reads `src`. libvips picks a loader by content, not by name,
/// and library files can come from strangers (torrents), so only the loaders libvips
/// trusts (JPEG, PNG, WebP, GIF, TIFF, HEIF) may read them. Its untrusted ones (PDF,
/// SVG, ImageMagick, OpenSlide, MATLAB, FITS, ...) stay off, except for a file that has
/// a RAW name and starts like a RAW format only ImageMagick reads: a hostile file merely
/// named like a RAW photo still gets the trusted loaders only.
async fn vips_command(program: &str, src: &Path) -> tokio::process::Command {
    let mut cmd = tools::command(program);
    let raw = RAW_EXTENSIONS.contains(&extension(src).as_str())
        && starts_like_magick_raw(&file_header(src).await);
    if !raw {
        cmd.env("VIPS_BLOCK_UNTRUSTED", "1");
    }
    cmd
}

/// `vipsthumbnail` with EXIF auto-rotation, shrinking (never enlarging) into a `edge`×`edge` box.
async fn vips_resize(src: &Path, dest: &Path, edge: u32, quality: u32) -> Result<()> {
    let tmp = TempOutput::next_to(dest);
    let mut out = tmp.path().as_os_str().to_owned();
    out.push(format!("[Q={quality},strip]"));
    let mut cmd = vips_command(tools::VIPSTHUMBNAIL, src).await;
    cmd.arg(src)
        .arg("--size")
        .arg(format!("{edge}x{edge}>"))
        .arg("-o")
        .arg(out);
    run_into(tmp, cmd, tools::VIPSTHUMBNAIL, dest).await
}

/// Stored pixel size of an image, via vipsheader, then ffprobe.
async fn image_size(abs: &Path) -> Option<(i64, i64)> {
    if tools::has_vips().await {
        let mut cmd = vips_command(tools::VIPSHEADER, abs).await;
        cmd.arg(abs);
        if let Ok(output) = tools::run_ok(cmd, tools::VIPSHEADER, Duration::from_secs(30)).await
            && let Some(size) = parse_vipsheader(&String::from_utf8_lossy(&output.stdout))
        {
            return Some(size);
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
        get(&[
            "make",
            "com.apple.quicktime.make",
            "com.android.manufacturer",
        ]),
        get(&["model", "com.apple.quicktime.model", "com.android.model"]),
    )
}

/// When the clip was recorded, from QuickTime/Matroska tags: the phone's clock with its
/// offset (`com.apple.quicktime.creationdate`), else the container's creation time (a
/// moment in UTC), else a `date` tag (section 4, Capture time).
pub fn taken_from_tags(tags: &BTreeMap<String, String>) -> Option<Recorded> {
    ["com.apple.quicktime.creationdate", "creation_time", "date"]
        .iter()
        .filter_map(|k| tags.get(*k))
        .find_map(|v| capture::parse_tag_time(v))
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
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
        .or_else(|| {
            mime_guess::from_path(path)
                .first()
                .map(|m| m.essence_str().to_string())
        })
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
        let settings = SettingsStore::load(db.clone(), HwAccel::None)
            .await
            .unwrap();
        let layout = Arc::new(StashLayout::new(root.path()));
        layout.ensure().await.unwrap();
        let media = MediaService::new(config, db, settings, layout, EventBus::new());
        Harness {
            _root: root,
            _config_dir: config_dir,
            media,
        }
    }

    async fn insert_item(
        media: &MediaService,
        rel: &str,
        kind: MediaKind,
        category: Category,
    ) -> String {
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
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=width,height,codec_name",
                "-of",
                "csv=p=0",
            ])
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
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=640x360:r=25:d=6",
                "-f",
                "lavfi",
                "-i",
                "sine=d=6",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-c:a",
                "aac",
                "-shortest",
                "-movflags",
                "use_metadata_tags",
                "-metadata",
                "com.apple.quicktime.make=Apple",
                "-metadata",
                "com.apple.quicktime.model=iPhone 15 Pro",
                "-metadata",
                "com.apple.quicktime.creationdate=2024-06-13T12:21:00+0200",
                "-metadata",
                "com.apple.quicktime.location.ISO6709=+37.7749-122.4194+010.000/",
                "-f",
                "mov",
            ],
            &tmp,
        );
        ffmpeg(
            &[
                "-display_rotation",
                "-90",
                "-i",
                tmp.to_str().unwrap(),
                "-c",
                "copy",
                "-movflags",
                "use_metadata_tags",
                "-f",
                "mov",
            ],
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
        assert!(
            (item.duration_s.unwrap() - 6.0).abs() < 0.2,
            "{:?}",
            item.duration_s
        );
        assert_eq!(
            (item.width, item.height),
            (Some(360), Some(640)),
            "rotation swaps the size"
        );
        assert_eq!(item.mime.as_deref(), Some("video/quicktime"));
        assert_eq!(item.camera.as_deref(), Some("Apple iPhone 15 Pro"));
        assert_eq!(item.taken_at, Some(1_718_274_060));
        let summary = item.media().unwrap();
        assert_eq!(summary.video.as_ref().unwrap().rotation, 90);
        assert!(
            !item.media_json.as_deref().unwrap().contains("37.7749"),
            "location must not be stored"
        );
        match events.try_recv().unwrap() {
            Event::Library {
                r#type, item_id, ..
            } => {
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
        let dir = media
            .thumb_path(&id, ThumbSize::Sm)
            .parent()
            .unwrap()
            .to_path_buf();
        assert!(
            std::fs::read_dir(dir).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp"))
        );
    }

    #[tokio::test]
    async fn movie_keeps_taken_at_empty_and_short_clips_work() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Movies/Clip (2020)/Clip (2020).mkv";
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=1920x1080:r=25:d=1.5",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-metadata",
                "creation_time=2020-01-01T00:00:00Z",
            ],
            &media.layout.abs(rel).unwrap(),
        );
        let id = insert_item(media, rel, MediaKind::Video, Category::Movie).await;
        media.process_item(&id).await.unwrap();
        let item = row(media, &id).await;
        assert_eq!(item.taken_at, None);
        assert_eq!(item.mime.as_deref(), Some("video/x-matroska"));
        assert_eq!((item.width, item.height), (Some(1920), Some(1080)));
        media.thumbnail_item(&id).await.unwrap();
        assert_eq!(
            webp_size(&media.thumb_path(&id, ThumbSize::Lg)),
            (1280, 720)
        );
        assert_eq!(webp_size(&media.thumb_path(&id, ThumbSize::Sm)), (400, 225));
    }

    /// An Android clip as the phone writes it: the name on the phone's clock, the
    /// container's creation time in UTC when the recording stopped.
    #[tokio::test]
    async fn an_android_clip_is_dated_by_its_name_and_creation_time() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Home Videos/2024/07/VID_20240630_223000.mp4";
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=320x240:r=25:d=1.5",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-metadata",
                "creation_time=2024-07-01T02:30:01Z",
            ],
            &media.layout.abs(rel).unwrap(),
        );
        let id = insert_item(media, rel, MediaKind::Video, Category::HomeVideo).await;
        media.process_item(&id).await.unwrap();
        let item = row(media, &id).await;
        assert_eq!(
            (
                item.taken_at,
                item.taken_local.as_deref(),
                item.taken_offset
            ),
            (Some(MONTH_EDGE), Some("2024-06-30T22:30:00"), Some(-240))
        );
    }

    #[tokio::test]
    async fn hdr_video_thumbnail_is_tone_mapped() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Home Videos/hdr.mp4";
        let encoders = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
            .unwrap();
        if !String::from_utf8_lossy(&encoders.stdout).contains("libx265") {
            eprintln!("libx265 missing; skipping");
            return;
        }
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=640x360:r=25:d=2",
                "-pix_fmt",
                "yuv420p10le",
                "-c:v",
                "libx265",
                "-x265-params",
                "log-level=error:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc",
                "-tag:v",
                "hvc1",
            ],
            &media.layout.abs(rel).unwrap(),
        );
        let id = insert_item(media, rel, MediaKind::Video, Category::HomeVideo).await;
        media.process_item(&id).await.unwrap();
        let summary = row(media, &id).await.media().unwrap();
        assert_eq!(
            summary.video.as_ref().unwrap().hdr.as_deref(),
            Some("hdr10")
        );
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
        let spec = exif::tests::TestExif {
            width: Some(2000),
            height: Some(1000),
            ..Default::default()
        };
        std::fs::write(
            &abs,
            exif::tests::inject_into_jpeg(&jpeg, &exif::tests::tiff_block(&spec)),
        )
        .unwrap();
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
        assert_eq!(
            webp_size(&media.thumb_path(&id, ThumbSize::Lg)),
            (640, 1280)
        );
        assert_eq!(webp_size(&media.thumb_path(&id, ThumbSize::Sm)), (200, 400));

        // Browsers show JPEG directly.
        assert_eq!(media.photo_view(&item).await.unwrap(), abs);
    }

    #[tokio::test]
    async fn photo_without_exif_gets_size_from_the_image() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Photos/screenshot.png";
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=300x200:d=1",
                "-frames:v",
                "1",
            ],
            &media.layout.abs(rel).unwrap(),
        );
        let id = insert_item(media, rel, MediaKind::Photo, Category::Photo).await;
        sqlx::query("UPDATE items SET taken_at = 123 WHERE id = ?")
            .bind(&id)
            .execute(&media.db)
            .await
            .unwrap();
        media.process_item(&id).await.unwrap();
        let item = row(media, &id).await;
        assert_eq!((item.width, item.height), (Some(300), Some(200)));
        assert_eq!(item.mime.as_deref(), Some("image/png"));
        assert_eq!(item.taken_at, Some(123), "an existing date is kept");
        media.thumbnail_item(&id).await.unwrap();
        assert_eq!(
            webp_size(&media.thumb_path(&id, ThumbSize::Lg)),
            (300, 200),
            "small images are not enlarged"
        );
    }

    /// A real iPhone-style HEIC (tests/fixtures/portrait.heic, a generated test pattern):
    /// 1200x800 stored pixels turned upright by the HEIF container, EXIF from an iPhone.
    #[tokio::test]
    async fn heic_photo_is_read_upright() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/portrait.heic");
        let vips = |args: &[&str]| {
            Command::new(args[0])
                .args(&args[1..])
                .arg(&fixture)
                .output()
                .is_ok_and(|out| out.status.success())
        };
        if !tools::has_vips().await || !vips(&["vipsheader"]) {
            eprintln!("skipping: libvips cannot read HEIC on this machine");
            return;
        }
        // libheif may read the container without an HEVC decoder (the image has one).
        let decodes = vips(&["vips", "avg"]);
        let h = harness().await;
        let media = &h.media;
        let rel = "Photos/2023/07/portrait.heic";
        let abs = media.layout.abs(rel).unwrap();
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::copy(&fixture, &abs).unwrap();
        let id = insert_item(media, rel, MediaKind::Photo, Category::Photo).await;

        media.process_item(&id).await.unwrap();
        let item = row(media, &id).await;
        assert_eq!((item.width, item.height), (Some(800), Some(1200)));
        assert_eq!(item.camera.as_deref(), Some("Apple iPhone 15 Pro"));
        // 2023-07-04 18:30:00 at UTC-07:00.
        assert_eq!(item.taken_at, Some(1_688_520_600));
        assert_eq!(item.mime.as_deref(), Some("image/heic"));

        // Without PixelXDimension and PixelYDimension in its EXIF, the size comes from
        // vipsheader, which reports it upright already: orientation 6 must not swap it again.
        let mut bytes = std::fs::read(&fixture).unwrap();
        for (tag, unknown) in [
            (b"\xa0\x02\x00\x03", b"\xa0\xf2\x00\x03"),
            (b"\xa0\x03\x00\x03", b"\xa0\xf3\x00\x03"),
        ] {
            let at = bytes
                .windows(4)
                .position(|w| w == tag)
                .expect("the fixture's EXIF has its pixel size");
            bytes[at..at + 4].copy_from_slice(unknown);
        }
        let sizeless = "Photos/2023/07/portrait-sizeless.heic";
        std::fs::write(media.layout.abs(sizeless).unwrap(), bytes).unwrap();
        let sizeless = insert_item(media, sizeless, MediaKind::Photo, Category::Photo).await;
        media.process_item(&sizeless).await.unwrap();
        let sizeless = row(media, &sizeless).await;
        assert_eq!((sizeless.width, sizeless.height), (Some(800), Some(1200)));
        assert_eq!(sizeless.camera.as_deref(), Some("Apple iPhone 15 Pro"));

        if !decodes {
            eprintln!("skipping the thumbnail: libheif has no HEVC decoder on this machine");
            return;
        }
        media.thumbnail_item(&id).await.unwrap();
        let item = row(media, &id).await;
        assert_eq!(item.thumb_state, ThumbState::Ready);
        let (w, h) = webp_size(&media.thumb_path(&id, ThumbSize::Lg));
        assert!(h > w, "the thumbnail is upright, not {w}x{h}");
        let view = media.photo_view(&item).await.unwrap();
        assert_eq!(view, media.rendition_path(&id));
        assert_eq!(webp_size(&view), (800, 1200));
    }

    #[tokio::test]
    async fn tiff_gets_a_cached_rendition() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Photos/scan.tif";
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=3000x2000:d=1",
                "-frames:v",
                "1",
            ],
            &media.layout.abs(rel).unwrap(),
        );
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
        assert!(matches!(
            media.photo_view(&row(media, &vid).await).await,
            Err(CoreError::NotFound)
        ));
    }

    #[tokio::test]
    async fn audio_cover_art_or_none() {
        let h = harness().await;
        let media = &h.media;
        let cover = media.layout.abs("Other/cover.png").unwrap();
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=500x500:d=1",
                "-frames:v",
                "1",
            ],
            &cover,
        );
        let with_cover = "Other/song.mp3";
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=d=2",
                "-i",
                cover.to_str().unwrap(),
                "-map",
                "0",
                "-map",
                "1",
                "-c:a",
                "libmp3lame",
                "-c:v",
                "png",
                "-disposition:v",
                "attached_pic",
                "-id3v2_version",
                "3",
            ],
            &media.layout.abs(with_cover).unwrap(),
        );
        let plain = "Other/plain.mp3";
        ffmpeg(
            &["-f", "lavfi", "-i", "sine=d=2", "-c:a", "libmp3lame"],
            &media.layout.abs(plain).unwrap(),
        );

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
        assert_eq!(
            row(media, &id).await.mime.as_deref(),
            Some("application/pdf")
        );
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

    #[tokio::test]
    async fn capture_dates_replace_file_dates() {
        let h = harness().await;
        let media = &h.media;
        // Files indexed in place get a file-system date from the scanner; the date the
        // photo or clip was taken must replace it once the probe has read it.
        let file_date = 1_800_000_000;
        let photo = "Photos/Family/IMG_0003.jpg";
        let abs = media.layout.abs(photo).unwrap();
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        exif::tests::make_jpeg(&abs, "320x240");
        let jpeg = std::fs::read(&abs).unwrap();
        std::fs::write(
            &abs,
            exif::tests::inject_into_jpeg(
                &jpeg,
                &exif::tests::tiff_block(&exif::tests::TestExif::default()),
            ),
        )
        .unwrap();
        let clip = "Home Videos/Clips/IMG_0004.MOV";
        make_phone_clip(&media.layout.abs(clip).unwrap());
        for (rel, kind, category) in [
            (photo, MediaKind::Photo, Category::Photo),
            (clip, MediaKind::Video, Category::HomeVideo),
        ] {
            let id = insert_item(media, rel, kind, category).await;
            sqlx::query("UPDATE items SET taken_at = ? WHERE id = ?")
                .bind(file_date)
                .bind(&id)
                .execute(&media.db)
                .await
                .unwrap();
            media.process_item(&id).await.unwrap();
            assert_eq!(row(media, &id).await.taken_at, Some(1_718_274_060), "{rel}");
        }
    }

    fn mean_luma(path: &Path) -> f64 {
        let out = Command::new("ffmpeg")
            .args(["-hide_banner", "-i"])
            .arg(path)
            .args([
                "-vf",
                "signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-",
                "-f",
                "null",
                "-",
            ])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|l| l.strip_prefix("lavfi.signalstats.YAVG="))
            .and_then(|v| v.trim().parse().ok())
            .expect("signalstats output")
    }

    #[tokio::test]
    async fn hdr_thumbnail_made_before_the_probe_is_tone_mapped() {
        let h = harness().await;
        let media = &h.media;
        let encoders = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
            .unwrap();
        if !String::from_utf8_lossy(&encoders.stdout).contains("libx265") {
            eprintln!("libx265 missing; skipping");
            return;
        }
        let rel = "Home Videos/hdr-early.mp4";
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=640x360:r=25:d=2",
                "-pix_fmt",
                "yuv420p10le",
                "-c:v",
                "libx265",
                "-x265-params",
                "log-level=error:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc",
                "-tag:v",
                "hvc1",
            ],
            &media.layout.abs(rel).unwrap(),
        );
        let id = insert_item(media, rel, MediaKind::Video, Category::HomeVideo).await;
        // Probe and thumbnail jobs run in separate pools, so the thumbnail often comes first.
        media.thumbnail_item(&id).await.unwrap();
        let early = mean_luma(&media.thumb_path(&id, ThumbSize::Lg));
        media.process_item(&id).await.unwrap();
        media.thumbnail_item(&id).await.unwrap();
        let late = mean_luma(&media.thumb_path(&id, ThumbSize::Lg));
        assert!(
            (early - late).abs() < 3.0,
            "thumbnail before the probe has mean luma {early}, after it {late}"
        );
    }

    #[tokio::test]
    async fn clip_shorter_than_its_metadata_still_gets_a_thumbnail() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Home Videos/short.mp4";
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=320x240:r=25:d=2",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
            ],
            &media.layout.abs(rel).unwrap(),
        );
        let id = insert_item(media, rel, MediaKind::Video, Category::HomeVideo).await;
        // The stored length says 60 s, so the poster frame is looked for past the real end.
        sqlx::query("UPDATE items SET duration_s = 60 WHERE id = ?")
            .bind(&id)
            .execute(&media.db)
            .await
            .unwrap();
        media.thumbnail_item(&id).await.unwrap();
        assert_eq!(row(media, &id).await.thumb_state, ThumbState::Ready);
        assert_eq!(webp_size(&media.thumb_path(&id, ThumbSize::Lg)), (320, 240));
    }

    #[tokio::test]
    async fn non_images_are_not_accepted_as_output() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("x.webp");
        // What ffmpeg leaves when asked for a frame past the end: 8 bytes, exit code 0.
        let tmp = TempOutput::next_to(&dest);
        let path = tmp.path().to_path_buf();
        std::fs::write(&path, [0, 0, 0, 0, 0xF8, 0xFF, 0xFF, 0xFF]).unwrap();
        assert!(finish_temp(tmp, &dest).await.is_err());
        assert!(!dest.exists() && !path.exists());
        let tmp = TempOutput::next_to(&dest);
        let path = tmp.path().to_path_buf();
        std::fs::write(&path, b"RIFF\x0c\0\0\0WEBPVP8 \0\0\0\0").unwrap();
        finish_temp(tmp, &dest).await.unwrap();
        assert!(dest.exists() && !path.exists());
    }

    /// A photo view the browser cancels drops the conversion midway, and the tool is
    /// killed with it: the partial file it was writing goes too, instead of piling up
    /// in the cache.
    #[tokio::test]
    async fn a_cancelled_conversion_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("item.webp");
        let tmp = TempOutput::next_to(&dest);
        let path = tmp.path().to_path_buf();
        // A slow converter: it starts writing its output, then takes its time.
        let mut cmd = tools::command("sh");
        cmd.arg("-c")
            .arg("printf partial > \"$1\"; exec sleep 30")
            .arg("sh")
            .arg(&path);
        let mut conversion = Box::pin(run_into(tmp, cmd, "sh", &dest));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            tokio::select! {
                _ = &mut conversion => panic!("the slow converter finished"),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the converter never started"
            );
        }
        drop(conversion);
        assert!(!path.exists(), "the partial output was left behind");
        assert!(!dest.exists());
    }

    /// Temp files left by a crash or a power cut are removed on start; finished
    /// thumbnails and renditions stay.
    #[tokio::test]
    async fn leftover_temp_files_are_swept() {
        let dir = tempfile::tempdir().unwrap();
        let renditions = dir.path().join("renditions");
        let thumbs = dir.path().join("thumbs/ab");
        std::fs::create_dir_all(&renditions).unwrap();
        std::fs::create_dir_all(&thumbs).unwrap();
        let leftovers = [
            renditions.join(".01a1.webp.738e27764815c28d.tmp.webp"),
            thumbs.join(".01ab-sm.webp.987df5f94c454fb4.tmp.webp"),
        ];
        let kept = [
            renditions.join("01a1.webp"),
            thumbs.join("01ab-sm.webp"),
            thumbs.join(".hidden-but-not-ours.webp"),
        ];
        for path in leftovers.iter().chain(&kept) {
            std::fs::write(path, b"x").unwrap();
        }
        let removed = remove_temp_files(&[renditions.clone(), dir.path().join("thumbs")]).await;
        assert_eq!(removed, 2);
        for path in &leftovers {
            assert!(!path.exists(), "{}", path.display());
        }
        for path in &kept {
            assert!(path.exists(), "{}", path.display());
        }
    }

    #[tokio::test]
    async fn thumb_job_prepares_the_rendition_and_views_share_it() {
        let h = harness().await;
        let media = &h.media;
        let rel = "Photos/scan2.tiff";
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=3000x2000:d=1",
                "-frames:v",
                "1",
            ],
            &media.layout.abs(rel).unwrap(),
        );
        let id = insert_item(media, rel, MediaKind::Photo, Category::Photo).await;
        media.thumbnail_item(&id).await.unwrap();
        let rendition = media.rendition_path(&id);
        assert!(rendition.exists(), "the thumb job makes the browser copy");
        assert_eq!(webp_size(&rendition), (2560, 1707));

        // Many viewers at once all get the same finished file.
        let item = row(media, &id).await;
        std::fs::remove_file(&rendition).unwrap();
        let views = futures::future::join_all((0..6).map(|_| media.photo_view(&item))).await;
        for view in views {
            assert_eq!(view.unwrap(), rendition);
        }
        assert_eq!(webp_size(&rendition), (2560, 1707));
        let dir = rendition.parent().unwrap();
        assert!(
            std::fs::read_dir(dir).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")),
            "no temp files left"
        );
    }

    #[test]
    fn seek_points() {
        assert_eq!(thumb_seek(None), 3.0);
        assert_eq!(thumb_seek(Some(600.0)), 60.0);
        assert_eq!(thumb_seek(Some(20.0)), 3.0);
        assert_eq!(thumb_seek(Some(4.0)), 2.0);
        assert_eq!(thumb_seek(Some(0.0)), 3.0);
    }

    fn zone(name: &str) -> TimeZone {
        TimeZone::get(name).unwrap()
    }

    fn probed_row(rel: &str, category: Category, mtime: i64) -> ItemRow {
        crate::library::scanner::new_row(
            rel,
            category,
            ItemStatus::Ready,
            crate::library::scanner::Meta::default(),
            1,
            mtime,
            None,
        )
    }

    /// 2024-07-01T02:30:00Z: the evening of June 30 in New York, July 1 in UTC and Tokyo.
    const MONTH_EDGE: i64 = 1_719_801_000;

    #[test]
    fn a_file_time_is_read_in_the_server_zone() {
        let item = probed_row("Photos/scan.png", Category::Photo, MONTH_EDGE);
        let read = |name: &str| capture_time(&item, None, Dating::Keep, &zone(name)).unwrap();
        assert_eq!(read("America/New_York").local_text(), "2024-06-30T22:30:00");
        assert_eq!(read("America/New_York").month(), "2024-06");
        assert_eq!(read("UTC").local_text(), "2024-07-01T02:30:00");
        assert_eq!(read("UTC").month(), "2024-07");
        assert_eq!(read("Asia/Tokyo").local_text(), "2024-07-01T11:30:00");
        for name in ["America/New_York", "UTC", "Asia/Tokyo"] {
            assert_eq!(
                read(name).at,
                MONTH_EDGE,
                "{name}: the moment is the file time"
            );
            assert_eq!(read(name).offset, None, "{name}");
        }
    }

    #[test]
    fn the_probe_keeps_a_stored_time_unless_the_file_says_better() {
        let ny = zone("America/New_York");
        let mut item = probed_row("Photos/2024/06/beach.jpg", Category::Photo, MONTH_EDGE);
        let stored = Recorded::clock(2024, 6, 2, 9, 0, 0, None)
            .unwrap()
            .resolve(&ny);
        item.set_taken(stored);
        assert_eq!(capture_time(&item, None, Dating::Keep, &ny), stored);
        // EXIF with an offset wins and keeps the camera's clock.
        let exif = Recorded::clock(2024, 6, 30, 23, 45, 0, Some(9 * 60));
        let t = capture_time(&item, exif, Dating::Keep, &ny).unwrap();
        assert_eq!(t.local_text(), "2024-06-30T23:45:00");
        assert_eq!(t.offset, Some(540));
        assert_eq!(t.at, 1_719_758_700);
        // Movies and episodes never take a capture time from the file.
        let movie = probed_row(
            "Movies/Heat (1995)/Heat (1995).mkv",
            Category::Movie,
            MONTH_EDGE,
        );
        assert_eq!(capture_time(&movie, exif, Dating::Keep, &ny), None);
        // Other files take the metadata's, without a fallback.
        let other = probed_row("Other/scan.jpg", Category::Other, MONTH_EDGE);
        assert_eq!(capture_time(&other, None, Dating::Keep, &ny), None);
        assert_eq!(capture_time(&other, exif, Dating::Keep, &ny), Some(t));
    }

    /// A clip's UTC creation time and the phone's clock in its name (Android, Samsung):
    /// the probe keeps the name's clock with the offset between them, on any server.
    #[test]
    fn the_probe_pairs_a_utc_creation_time_with_the_name_clock() {
        let created = Recorded::utc(2024, 7, 1, 2, 30, 0);
        for server in ["UTC", "America/New_York"] {
            let zone = zone(server);
            for (rel, category) in [
                (
                    "Home Videos/2024/07/VID_20240630_223000.mp4",
                    Category::HomeVideo,
                ),
                (
                    "Home Videos/2024/07/20240630_223000.mp4",
                    Category::HomeVideo,
                ),
                ("Photos/Camera/20240630_223000.mp4", Category::Photo),
            ] {
                let mut item = probed_row(rel, category, MONTH_EDGE + 86_400);
                // Dated from its name when it was indexed.
                item.set_taken(
                    Recorded::clock(2024, 6, 30, 22, 30, 0, None)
                        .unwrap()
                        .resolve(&zone),
                );
                for dating in [Dating::Keep, Dating::Redate] {
                    let t = capture_time(&item, created, dating, &zone).unwrap();
                    assert_eq!(
                        (t.at, t.local_text().as_str(), t.offset),
                        (MONTH_EDGE, "2024-06-30T22:30:00", Some(-240)),
                        "{rel} {dating:?} on {server}"
                    );
                }
                // Written when a 40 second clip stopped.
                item.duration_s = Some(40.0);
                let stopped = Recorded::utc(2024, 7, 1, 2, 30, 40);
                let t = capture_time(&item, stopped, Dating::Keep, &zone).unwrap();
                assert_eq!((t.at, t.offset), (MONTH_EDGE, Some(-240)), "{rel}");
            }
        }
        // Without a clock in the name the moment is read in the server's zone, as before.
        let item = probed_row("Home Videos/clip.mp4", Category::HomeVideo, MONTH_EDGE);
        let t = capture_time(&item, created, Dating::Keep, &zone("UTC")).unwrap();
        assert_eq!(
            (t.local_text().as_str(), t.offset),
            ("2024-07-01T02:30:00", None)
        );
    }

    /// Rows dated before migration 0003: the migration stored each old moment's UTC reading
    /// as the clock and queued a probe that dates the row again.
    #[test]
    fn redating_rows_from_before_capture_times() {
        let ny = zone("America/New_York");
        let legacy = |rel: &str, old_at: i64| {
            let mut item = probed_row(rel, Category::Photo, MONTH_EDGE + 86_400);
            item.taken_at = Some(old_at);
            item.taken_local = Some(
                CaptureTime::from_stored(Some(old_at), None, None)
                    .unwrap()
                    .local_text(),
            );
            item
        };
        // Dated by its modification time: the same moment, now read in the server's zone.
        let item = legacy("Photos/2024/07/scan.png", MONTH_EDGE);
        let t = capture_time(&item, None, Dating::Redate, &ny).unwrap();
        assert_eq!(
            (t.at, t.local_text().as_str()),
            (MONTH_EDGE, "2024-06-30T22:30:00")
        );
        // Dated by its name, which the old code stored as if the clock were UTC: the clock
        // stays, the moment moves to the server's zone.
        let item = legacy("Photos/2024/06/IMG_20240630_223000.jpg", 1_719_786_600);
        let t = capture_time(&item, None, Dating::Redate, &ny).unwrap();
        assert_eq!(
            (t.at, t.local_text().as_str()),
            (MONTH_EDGE, "2024-06-30T22:30:00")
        );
        // EXIF read again: the offset comes back.
        let exif = Recorded::clock(2024, 6, 30, 22, 30, 0, Some(-240));
        let t = capture_time(&item, exif, Dating::Redate, &zone("UTC")).unwrap();
        assert_eq!((t.at, t.offset), (MONTH_EDGE, Some(-240)));
        // Keep mode would have left the old reading alone.
        let item = legacy("Photos/2024/07/scan.png", MONTH_EDGE);
        assert_eq!(
            capture_time(&item, None, Dating::Keep, &ny)
                .unwrap()
                .local_text(),
            "2024-07-01T02:30:00"
        );
    }

    #[test]
    fn tags_to_camera_and_date() {
        let mut tags = BTreeMap::new();
        assert_eq!(camera_from_tags(&tags), None);
        assert_eq!(taken_from_tags(&tags), None);
        tags.insert("com.android.manufacturer".to_string(), "Google".to_string());
        tags.insert("com.android.model".to_string(), "Pixel 8".to_string());
        tags.insert(
            "creation_time".to_string(),
            "2024-06-13T10:21:00.000000Z".to_string(),
        );
        assert_eq!(camera_from_tags(&tags).as_deref(), Some("Google Pixel 8"));
        // A container's creation time is a moment in UTC, with no clock of its own.
        assert_eq!(
            taken_from_tags(&tags),
            Some(Recorded::Instant(1_718_274_060))
        );
        // The Apple local date (clock and offset) wins over it when both exist.
        tags.insert(
            "com.apple.quicktime.creationdate".to_string(),
            "2024-06-13T12:00:00+0200".to_string(),
        );
        assert_eq!(
            taken_from_tags(&tags),
            Recorded::clock(2024, 6, 13, 12, 0, 0, Some(120))
        );
        // A date tag without a zone is the camera's clock.
        let mut tags = BTreeMap::new();
        tags.insert("date".to_string(), "2024-06-13 12:00:00".to_string());
        assert_eq!(
            taken_from_tags(&tags),
            Recorded::clock(2024, 6, 13, 12, 0, 0, None)
        );
    }

    #[test]
    fn mime_types() {
        assert_eq!(guess_mime(Path::new("a.MKV")), "video/x-matroska");
        assert_eq!(guess_mime(Path::new("a.ts")), "video/mp2t");
        assert_eq!(guess_mime(Path::new("a.mp4")), "video/mp4");
        assert_eq!(guess_mime(Path::new("a.HEIC")), "image/heic");
        assert_eq!(guess_mime(Path::new("a.jpg")), "image/jpeg");
        assert_eq!(
            guess_mime(Path::new("a.unknownext")),
            "application/octet-stream"
        );
    }

    /// libvips chooses a loader by content, not by name. A library file can come from a
    /// stranger (a torrent), so a "photo" may only be read by the loaders libvips
    /// trusts (JPEG, PNG, WebP, GIF, TIFF, HEIF), not by its untrusted ones (PDF, SVG,
    /// ImageMagick, OpenSlide, MATLAB, FITS, PPM, ...).
    #[tokio::test]
    async fn photos_are_read_only_by_trusted_vips_loaders() {
        if !tools::has_vips().await {
            eprintln!("skipping: libvips is not installed");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        // A PPM image (an untrusted loader) under a .jpg name.
        let disguised = dir.path().join("holiday.jpg");
        std::fs::write(
            &disguised,
            "P3\n2 2\n255\n255 0 0  0 255 0\n0 0 255  255 255 255\n",
        )
        .unwrap();
        let out = dir.path().join("out.webp");
        assert!(vips_resize(&disguised, &out, 64, 80).await.is_err());
        assert!(!out.exists());
        assert_eq!(
            image_size(&disguised).await,
            Some((2, 2)),
            "ffmpeg still reads it"
        );

        // A real JPEG goes through as before.
        let real = dir.path().join("real.jpg");
        ffmpeg(
            &["-f", "lavfi", "-i", "color=red:s=40x30", "-frames:v", "1"],
            &real,
        );
        vips_resize(&real, &out, 64, 80).await.unwrap();
        assert_eq!(webp_size(&out), (40, 30));
    }

    /// Camera RAW names open libvips' untrusted loaders (ImageMagick reads RAW), but only
    /// for files that really start like a RAW photo. Anything else under a RAW name (a
    /// PPM, an SVG for librsvg) gets the trusted loaders only, like any other photo.
    #[tokio::test]
    async fn raw_names_do_not_open_untrusted_loaders_for_other_content() {
        if !tools::has_vips().await {
            eprintln!("skipping: libvips is not installed");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.webp");
        let ppm = dir.path().join("holiday.nef");
        std::fs::write(
            &ppm,
            "P3\n2 2\n255\n255 0 0  0 255 0\n0 0 255  255 255 255\n",
        )
        .unwrap();
        let svg = dir.path().join("holiday.dng");
        std::fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#,
        )
        .unwrap();
        for disguised in [&ppm, &svg] {
            assert!(
                vips_resize(disguised, &out, 64, 80).await.is_err(),
                "{} was read by an untrusted loader",
                disguised.display()
            );
            assert!(!out.exists());
        }

        // A JPEG with a RAW name is still read (by the trusted JPEG loader).
        let jpeg = dir.path().join("renamed.cr2");
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "color=red:s=40x30",
                "-frames:v",
                "1",
                "-f",
                "mjpeg",
            ],
            &jpeg,
        );
        vips_resize(&jpeg, &out, 64, 80).await.unwrap();
        assert_eq!(webp_size(&out), (40, 30));
    }

    #[test]
    fn magick_raw_starts() {
        for header in [
            &b"IIRO\x08\0\0\0"[..],
            b"IIRS\x08\0\0\0",
            b"MMOR\0\0\0\x08",
            b"IIU\0\x08\0\0\0",
            b"FUJIFILMCCD-RAW 0201",
            b"\0\0\0\x18ftypcrx \0\0\0\x01",
        ] {
            assert!(starts_like_magick_raw(header), "{header:?}");
        }
        for header in [
            // TIFF-based RAW files go to the trusted TIFF loader.
            &b"II*\0\x08\0\0\0"[..],
            b"MM\0*\0\0\0\x08",
            b"P3\n2 2\n255\n",
            b"<svg xmlns=",
            b"%PDF-1.7",
            b"\0\0\0\x18ftypheic",
            b"IIR",
            b"",
        ] {
            assert!(!starts_like_magick_raw(header), "{header:?}");
        }
    }

    #[test]
    fn vipsheader_output() {
        assert_eq!(
            parse_vipsheader("a.jpg: 640x480 uchar, 3 bands, srgb, jpegload\n"),
            Some((640, 480))
        );
        assert_eq!(
            parse_vipsheader("my 2x2: file.png: 30x20 uchar, 4 bands, srgb, pngload"),
            Some((30, 20))
        );
        assert_eq!(parse_vipsheader(""), None);
    }

    #[test]
    fn display_sizes() {
        let mut summary = MediaSummary {
            video: Some(crate::models::VideoStream {
                width: 1920,
                height: 1080,
                rotation: 270,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(display_size(&summary), (Some(1080), Some(1920)));
        summary.video.as_mut().unwrap().rotation = 180;
        assert_eq!(display_size(&summary), (Some(1920), Some(1080)));
        assert_eq!(display_size(&MediaSummary::default()), (None, None));
    }
}
