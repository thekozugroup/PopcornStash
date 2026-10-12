//! File classification rules (section 4).

use std::path::Path;

use crate::media::exif::ExifInfo;
use crate::models::{Category, Classification, Confidence, MediaKind, MediaSummary};

use super::organizer::destination_for;
use super::parse::{ParsedName, parse_name, season_folder, unix_from_parts};

const VIDEO_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "mov", "avi", "webm", "ts", "m2ts", "mts", "wmv", "flv", "3gp", "mpg", "mpeg", "vob", "ogv",
];
const PHOTO_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "heif", "webp", "gif", "tif", "tiff", "avif", "dng", "cr2", "cr3", "nef", "arw", "orf",
    "rw2", "raf",
];
const AUDIO_EXTS: &[&str] = &["mp3", "m4a", "flac", "wav", "aac", "ogg", "opus"];

/// Media kind from the file extension.
pub fn media_kind(path: &Path) -> MediaKind {
    let Some(ext) = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()) else {
        return MediaKind::Other;
    };
    if VIDEO_EXTS.contains(&ext.as_str()) {
        MediaKind::Video
    } else if PHOTO_EXTS.contains(&ext.as_str()) {
        MediaKind::Photo
    } else if AUDIO_EXTS.contains(&ext.as_str()) {
        MediaKind::Audio
    } else {
        MediaKind::Other
    }
}

/// Classify a file. `rel_path` is relative to the stash root; `mtime` is unix seconds.
/// The returned `destination` is a relative path inside the library folders.
pub fn classify(rel_path: &str, probe: Option<&MediaSummary>, exif: Option<&ExifInfo>, mtime: i64) -> Classification {
    let path = Path::new(rel_path);
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| rel_path.to_string());
    let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| file_name.clone());
    let kind = media_kind(path);
    let parsed = parse_name(&stem);

    let mut c = match kind {
        MediaKind::Photo => photo(&stem, &parsed, exif, mtime),
        MediaKind::Video => episode(rel_path, &parsed)
            .or_else(|| home_video(&stem, &parsed, probe, exif, mtime))
            .or_else(|| movie(&parsed))
            .unwrap_or_else(|| other(&stem, "We could not tell what this video is")),
        MediaKind::Audio => other(&stem, "Audio files go to Other"),
        MediaKind::Other => other(&stem, "This is not a video or photo"),
    };
    c.destination = destination_for(&c, &file_name);
    c
}

fn base(category: Category, title: String, confidence: Confidence, reason: String) -> Classification {
    Classification {
        category,
        title,
        year: None,
        show_name: None,
        season: None,
        episode: None,
        episode_title: None,
        taken_at: None,
        confidence,
        reason,
        destination: String::new(),
    }
}

/// Rule 1: photos, dated by EXIF, then the name, then the modified time.
fn photo(stem: &str, parsed: &ParsedName, exif: Option<&ExifInfo>, mtime: i64) -> Classification {
    let camera = exif.and_then(|e| e.camera.clone());
    let (taken_at, source) = match (exif.and_then(|e| e.taken_at), parsed.date) {
        (Some(t), _) => (t, "dated from its EXIF data"),
        (None, Some(t)) => (t, "dated from its file name"),
        (None, None) => (mtime, "dated from when the file was last changed"),
    };
    let reason = match camera {
        Some(camera) => format!("Photo from {camera}, {source}"),
        None => format!("Photo, {source}"),
    };
    let mut c = base(Category::Photo, stem.to_string(), Confidence::High, reason);
    c.taken_at = Some(taken_at);
    c
}

/// Rule 2: an episode marker in the name.
fn episode(rel_path: &str, parsed: &ParsedName) -> Option<Classification> {
    let (season, episode) = (parsed.season?, parsed.episode?);
    let mut show = parsed.title.clone();
    if show.is_empty() {
        show = show_from_folders(rel_path).unwrap_or_default();
    }
    let marker = match parsed.episode_end {
        Some(end) => format!("S{season:02}E{episode:02}-E{end:02}"),
        None => format!("S{season:02}E{episode:02}"),
    };
    let (confidence, reason, title) = if show.is_empty() {
        (Confidence::Low, format!("Found {marker} in the name, but no show name"), marker.clone())
    } else {
        (Confidence::High, format!("Found {marker} in the name"), format!("{show} S{season:02}E{episode:02}"))
    };
    let mut c = base(Category::Episode, title, confidence, reason);
    c.show_name = (!show.is_empty()).then_some(show);
    c.season = Some(season);
    c.episode = Some(episode);
    c.episode_title = parsed.episode_title.clone();
    c.year = parsed.year;
    Some(c)
}

/// The nearest parent folder that names a show ("Downloads/Completed/The Office/Season 2/S02E03.mkv").
fn show_from_folders(rel_path: &str) -> Option<String> {
    let parts: Vec<&str> = rel_path.split('/').collect();
    let dirs = parts.get(..parts.len().saturating_sub(1))?;
    for dir in dirs.iter().rev() {
        let lower = dir.to_ascii_lowercase();
        if season_folder(dir).is_some()
            || matches!(lower.as_str(), "inbox" | "downloads" | "completed" | "in progress" | "shows" | "tv" | "tv shows")
        {
            continue;
        }
        let title = parse_name(dir).title;
        if !title.is_empty() {
            return Some(title);
        }
    }
    None
}

/// Rule 3: camera metadata or a camera-style file name.
fn home_video(
    stem: &str,
    parsed: &ParsedName,
    probe: Option<&MediaSummary>,
    exif: Option<&ExifInfo>,
    mtime: i64,
) -> Option<Classification> {
    let hint = probe.and_then(camera_hint);
    if hint.is_none() && !parsed.camera_style {
        return None;
    }
    let reason = match &hint {
        Some(h) => h.reason.clone(),
        None => "The name looks like a phone or camera file".to_string(),
    };
    let taken_at = probe
        .and_then(creation_time)
        .or_else(|| exif.and_then(|e| e.taken_at))
        .or(parsed.date)
        .unwrap_or(mtime);
    let mut c = base(Category::HomeVideo, stem.to_string(), Confidence::High, reason);
    c.taken_at = Some(taken_at);
    Some(c)
}

/// Rule 4: a title followed by a year.
fn movie(parsed: &ParsedName) -> Option<Classification> {
    if parsed.title.is_empty() {
        return None;
    }
    match parsed.year {
        Some(year) => {
            let mut c = base(
                Category::Movie,
                parsed.title.clone(),
                Confidence::High,
                format!("Found the year {year} in the name"),
            );
            c.year = Some(year);
            Some(c)
        }
        None if parsed.has_release_tags => Some(base(
            Category::Movie,
            parsed.title.clone(),
            Confidence::Low,
            "Looks like a movie release, but the name has no year".to_string(),
        )),
        None => None,
    }
}

/// Rule 5: everything else.
fn other(stem: &str, reason: &str) -> Classification {
    base(Category::Other, stem.to_string(), Confidence::Low, reason.to_string())
}

// ---------------------------------------------------------------------------
// Probe tags

struct CameraHint {
    label: Option<String>,
    reason: String,
}

/// Case-insensitive tag lookup; empty values count as absent.
fn tag<'a>(summary: &'a MediaSummary, key: &str) -> Option<&'a str> {
    summary
        .tags
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
}

fn join_make_model(make: Option<&str>, model: Option<&str>) -> Option<String> {
    match (make, model) {
        (Some(make), Some(model)) if model.to_ascii_lowercase().starts_with(&make.to_ascii_lowercase()) => {
            Some(model.to_string())
        }
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (Some(one), None) | (None, Some(one)) => Some(one.to_string()),
        (None, None) => None,
    }
}

fn camera_hint(summary: &MediaSummary) -> Option<CameraHint> {
    let apple = join_make_model(tag(summary, "com.apple.quicktime.make"), tag(summary, "com.apple.quicktime.model"));
    if let Some(label) = apple {
        return Some(CameraHint { reason: format!("Phone video from {label}"), label: Some(label) });
    }
    let android = join_make_model(tag(summary, "com.android.manufacturer"), tag(summary, "com.android.model"));
    if let Some(label) = android {
        return Some(CameraHint { reason: format!("Phone video from {label}"), label: Some(label) });
    }
    if tag(summary, "com.android.version").is_some() {
        return Some(CameraHint { label: None, reason: "Video from an Android phone".to_string() });
    }
    if let Some(label) = join_make_model(tag(summary, "make"), tag(summary, "model")) {
        return Some(CameraHint { reason: format!("Camera video from {label}"), label: Some(label) });
    }
    let handler = tag(summary, "handler_name").or_else(|| tag(summary, "handler")).unwrap_or_default();
    if handler.to_ascii_lowercase().contains("gopro") {
        return Some(CameraHint { label: Some("GoPro".to_string()), reason: "GoPro video".to_string() });
    }
    let encoder = tag(summary, "encoder").unwrap_or_default();
    if encoder.to_ascii_lowercase().contains("dji") {
        return Some(CameraHint { label: Some("DJI".to_string()), reason: "DJI camera video".to_string() });
    }
    if encoder.to_ascii_lowercase().contains("gopro") {
        return Some(CameraHint { label: Some("GoPro".to_string()), reason: "GoPro video".to_string() });
    }
    None
}

/// "Apple iPhone 15 Pro", "GoPro", … from probe tags, when the file came from a camera.
pub fn camera_from_tags(summary: &MediaSummary) -> Option<String> {
    camera_hint(summary).and_then(|h| h.label)
}

/// Capture time from `com.apple.quicktime.creationdate` (local time with offset) or
/// `creation_time` (UTC). Placeholder dates (1904, 1970) are ignored.
pub fn creation_time(summary: &MediaSummary) -> Option<i64> {
    ["com.apple.quicktime.creationdate", "creation_time", "date"]
        .iter()
        .filter_map(|key| tag(summary, key))
        .find_map(parse_timestamp)
}

/// `2024-06-13T10:21:00.000000Z`, `2024-06-13T10:21:00+0200`, `2024-06-13 10:21:00`.
pub(crate) fn parse_timestamp(value: &str) -> Option<i64> {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?\s*(Z|[+-]\d{2}:?\d{2})?$")
            .expect("static regex is valid")
    });
    let c = RE.captures(value.trim())?;
    let num = |i: usize| c[i].parse::<i64>().ok();
    let utc = unix_from_parts(num(1)?, num(2)?, num(3)?, num(4)?, num(5)?, num(6)?)?;
    let offset = match c.get(7).map(|m| m.as_str()) {
        None | Some("Z") => 0,
        Some(o) => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let digits: String = o.chars().filter(char::is_ascii_digit).collect();
            let hours: i64 = digits.get(..2)?.parse().ok()?;
            let minutes: i64 = digits.get(2..4)?.parse().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
    };
    let ts = utc - offset;
    // 1971 onward: 1904/1970 are what cameras write when the clock was never set.
    (ts >= 31_536_000 && ts <= crate::now() + 2 * 86_400).then_some(ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(tags: &[(&str, &str)]) -> MediaSummary {
        MediaSummary {
            tags: tags.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            ..MediaSummary::default()
        }
    }

    const MTIME: i64 = 1_700_000_000; // 2023-11-14

    #[test]
    fn kinds() {
        assert_eq!(media_kind(Path::new("a/b.MKV")), MediaKind::Video);
        assert_eq!(media_kind(Path::new("x.m2ts")), MediaKind::Video);
        assert_eq!(media_kind(Path::new("x.HEIC")), MediaKind::Photo);
        assert_eq!(media_kind(Path::new("x.cr3")), MediaKind::Photo);
        assert_eq!(media_kind(Path::new("x.flac")), MediaKind::Audio);
        assert_eq!(media_kind(Path::new("x.srt")), MediaKind::Other);
        assert_eq!(media_kind(Path::new("README")), MediaKind::Other);
    }

    #[test]
    fn photo_dates() {
        let exif = ExifInfo { taken_at: Some(1_718_273_700), camera: Some("Apple iPhone 15 Pro".into()), ..Default::default() };
        let c = classify("Inbox/IMG_1234.HEIC", None, Some(&exif), MTIME);
        assert_eq!(c.category, Category::Photo);
        assert_eq!(c.confidence, Confidence::High);
        assert_eq!(c.taken_at, Some(1_718_273_700));
        assert_eq!(c.destination, "Photos/2024/06/IMG_1234.HEIC");
        assert_eq!(c.reason, "Photo from Apple iPhone 15 Pro, dated from its EXIF data");

        let c = classify("Inbox/PXL_20230102_030405678.jpg", None, None, MTIME);
        assert_eq!(c.destination, "Photos/2023/01/PXL_20230102_030405678.jpg");
        assert_eq!(c.reason, "Photo, dated from its file name");

        let c = classify("Inbox/scan.png", None, None, MTIME);
        assert_eq!(c.taken_at, Some(MTIME));
        assert_eq!(c.destination, "Photos/2023/11/scan.png");
        assert!(c.reason.contains("last changed"));

        // A photo with an episode-like name is still a photo (rule 1 wins).
        let c = classify("Inbox/Show.S01E02.jpg", None, None, MTIME);
        assert_eq!(c.category, Category::Photo);
    }

    #[test]
    fn episodes() {
        let c = classify("Downloads/Completed/The.Office.US.S02E03.720p.WEB-DL.x264-GRP.mkv", None, None, MTIME);
        assert_eq!(c.category, Category::Episode);
        assert_eq!(c.confidence, Confidence::High);
        assert_eq!(c.show_name.as_deref(), Some("The Office US"));
        assert_eq!((c.season, c.episode), (Some(2), Some(3)));
        assert_eq!(c.reason, "Found S02E03 in the name");
        assert_eq!(c.destination, "Shows/The Office US/Season 02/The Office US - S02E03.mkv");

        let c = classify("Inbox/Show.Name.S01E02E03.mkv", None, None, MTIME);
        assert_eq!(c.destination, "Shows/Show Name/Season 01/Show Name - S01E02-E03.mkv");
        assert_eq!(c.reason, "Found S01E02-E03 in the name");

        // Show name from the folder when the file only has the marker.
        let c = classify("Downloads/Completed/Severance/Season 2/S02E05.mkv", None, None, MTIME);
        assert_eq!(c.show_name.as_deref(), Some("Severance"));
        assert_eq!(c.confidence, Confidence::High);

        let c = classify("Inbox/S01E02.mkv", None, None, MTIME);
        assert_eq!(c.confidence, Confidence::Low);
        assert_eq!(c.show_name, None);
    }

    #[test]
    fn home_videos_from_tags() {
        let probe = summary(&[
            ("com.apple.quicktime.make", "Apple"),
            ("com.apple.quicktime.model", "iPhone 15 Pro"),
            ("com.apple.quicktime.creationdate", "2024-06-13T12:21:00+0200"),
            ("creation_time", "2024-06-13T10:21:05.000000Z"),
        ]);
        let c = classify("Inbox/Beach day.mov", Some(&probe), None, MTIME);
        assert_eq!(c.category, Category::HomeVideo);
        assert_eq!(c.confidence, Confidence::High);
        assert_eq!(c.reason, "Phone video from Apple iPhone 15 Pro");
        assert_eq!(c.taken_at, parse_timestamp("2024-06-13T10:21:00Z"));
        assert_eq!(c.destination, "Home Videos/2024/06/Beach day.mov");
        assert_eq!(camera_from_tags(&probe).as_deref(), Some("Apple iPhone 15 Pro"));

        let probe = summary(&[("com.android.version", "14"), ("creation_time", "2023-02-01T08:00:00Z")]);
        let c = classify("Inbox/clip.mp4", Some(&probe), None, MTIME);
        assert_eq!(c.category, Category::HomeVideo);
        assert_eq!(c.destination, "Home Videos/2023/02/clip.mp4");

        let probe = summary(&[("handler_name", "\tGoPro AVC  "), ("creation_time", "1970-01-01T00:00:00Z")]);
        let c = classify("Inbox/ride.mp4", Some(&probe), None, MTIME);
        assert_eq!(c.reason, "GoPro video");
        assert_eq!(c.taken_at, Some(MTIME), "placeholder creation_time is ignored");

        let probe = summary(&[("encoder", "DJI Mini 3")]);
        assert_eq!(classify("Inbox/x.mp4", Some(&probe), None, MTIME).reason, "DJI camera video");

        let probe = summary(&[("make", "Canon"), ("model", "Canon EOS R6")]);
        assert_eq!(classify("Inbox/x.mp4", Some(&probe), None, MTIME).reason, "Camera video from Canon EOS R6");

        // Release files carry an encoder tag but no camera data.
        let probe = summary(&[("encoder", "Lavf60.3.100")]);
        assert_eq!(classify("Inbox/Heat.1995.mkv", Some(&probe), None, MTIME).category, Category::Movie);
    }

    #[test]
    fn home_videos_from_names() {
        let c = classify("Inbox/VID_20240613_101500.mp4", None, None, MTIME);
        assert_eq!(c.category, Category::HomeVideo);
        assert_eq!(c.destination, "Home Videos/2024/06/VID_20240613_101500.mp4");
        assert_eq!(c.reason, "The name looks like a phone or camera file");

        let c = classify("Inbox/IMG_1234.MOV", None, None, MTIME);
        assert_eq!(c.destination, "Home Videos/2023/11/IMG_1234.MOV");

        for name in ["GOPR0123.MP4", "GX010123.MP4", "DJI_0042.MP4", "MVI_1234.MOV", "RPReplay_Final1690000000.MP4"] {
            let c = classify(&format!("Inbox/{name}"), None, None, MTIME);
            assert_eq!(c.category, Category::HomeVideo, "{name}");
            assert_eq!(c.confidence, Confidence::High, "{name}");
        }
        let c = classify("Inbox/Screen Recording 2024-06-13 at 10.15.00.mov", None, None, MTIME);
        assert_eq!(c.destination, "Home Videos/2024/06/Screen Recording 2024-06-13 at 10.15.00.mov");
    }

    #[test]
    fn movies() {
        let c = classify("Downloads/Completed/Heat.1995.1080p.BluRay.x264-GRP/Heat.1995.1080p.BluRay.x264-GRP.mkv", None, None, MTIME);
        assert_eq!(c.category, Category::Movie);
        assert_eq!(c.confidence, Confidence::High);
        assert_eq!((c.title.as_str(), c.year), ("Heat", Some(1995)));
        assert_eq!(c.reason, "Found the year 1995 in the name");
        assert_eq!(c.destination, "Movies/Heat (1995)/Heat (1995).mkv");

        let c = classify("Inbox/Blade Runner 2049 (2017).MP4", None, None, MTIME);
        assert_eq!(c.destination, "Movies/Blade Runner 2049 (2017)/Blade Runner 2049 (2017).mp4");

        let c = classify("Inbox/Some.Movie.1080p.BluRay.x264.mkv", None, None, MTIME);
        assert_eq!(c.category, Category::Movie);
        assert_eq!(c.confidence, Confidence::Low);
        assert_eq!(c.destination, "Movies/Some Movie/Some Movie.mkv");
    }

    #[test]
    fn others() {
        let c = classify("Inbox/Our Wedding.mp4", None, None, MTIME);
        assert_eq!(c.category, Category::Other);
        assert_eq!(c.confidence, Confidence::Low);
        assert_eq!(c.destination, "Other/Our Wedding.mp4");

        let c = classify("Inbox/song.mp3", None, None, MTIME);
        assert_eq!(c.category, Category::Other);
        assert_eq!(c.confidence, Confidence::Low);
        assert_eq!(c.destination, "Other/song.mp3");

        let c = classify("Inbox/notes.pdf", None, None, MTIME);
        assert_eq!(c.category, Category::Other);
    }

    #[test]
    fn timestamps() {
        assert_eq!(parse_timestamp("2024-06-13T10:21:00Z"), Some(1_718_274_060));
        assert_eq!(parse_timestamp("2024-06-13T10:21:00.000000Z"), Some(1_718_274_060));
        assert_eq!(parse_timestamp("2024-06-13T12:21:00+0200"), Some(1_718_274_060));
        assert_eq!(parse_timestamp("2024-06-13T05:21:00-05:00"), Some(1_718_274_060));
        assert_eq!(parse_timestamp("2024-06-13 10:21:00"), Some(1_718_274_060));
        assert_eq!(parse_timestamp("1904-01-01T00:00:00Z"), None);
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), None);
        assert_eq!(parse_timestamp("not a date"), None);
    }
}
