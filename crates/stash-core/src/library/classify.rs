//! File classification rules (section 4).

use std::path::Path;

use jiff::tz::TimeZone;

use crate::capture::{self, Recorded};
use crate::media::exif::ExifInfo;
use crate::models::{Category, Classification, Confidence, MediaKind, MediaSummary};

use super::organizer::layout_for;
use super::parse::{ParsedName, parse_name, season_folder};

const VIDEO_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "mov", "avi", "webm", "ts", "m2ts", "mts", "wmv", "flv", "3gp", "mpg",
    "mpeg", "vob", "ogv",
];
const PHOTO_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "heif", "webp", "gif", "tif", "tiff", "avif", "dng", "cr2",
    "cr3", "nef", "arw", "orf", "rw2", "raf",
];
const AUDIO_EXTS: &[&str] = &["mp3", "m4a", "flac", "wav", "aac", "ogg", "opus"];

/// Media kind from the file extension.
pub fn media_kind(path: &Path) -> MediaKind {
    let Some(ext) = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
    else {
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
/// The returned `layout` is where the file would sit inside a library folder of its kind.
/// Capture times without a zone are placed in the server's time zone.
pub fn classify(
    rel_path: &str,
    probe: Option<&MediaSummary>,
    exif: Option<&ExifInfo>,
    mtime: i64,
) -> Classification {
    classify_in(rel_path, probe, exif, mtime, &capture::server_zone())
}

/// [`classify`] with the server's time zone given.
pub fn classify_in(
    rel_path: &str,
    probe: Option<&MediaSummary>,
    exif: Option<&ExifInfo>,
    mtime: i64,
    zone: &TimeZone,
) -> Classification {
    let path = Path::new(rel_path);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel_path.to_string());
    let stem = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_name.clone());
    let kind = media_kind(path);
    let parsed = parse_name(&stem);

    let mut c = match kind {
        MediaKind::Photo => photo(&stem, &parsed, exif, mtime, zone),
        MediaKind::Video => episode(rel_path, &parsed)
            .or_else(|| home_video(&stem, &parsed, probe, exif, mtime, zone))
            .or_else(|| movie(&parsed).filter(|c| c.confidence == Confidence::High))
            .or_else(|| from_download_folder(rel_path))
            .or_else(|| movie(&parsed))
            .unwrap_or_else(|| other(&stem, "We could not tell what this video is")),
        // Audio can only ever go to Other, so there is nothing to review.
        MediaKind::Audio => Classification {
            confidence: Confidence::High,
            ..other(&stem, "Audio files go to Other")
        },
        MediaKind::Other => other(&stem, "This is not a video or photo"),
    };
    c.layout = layout_for(&c, &file_name);
    c
}

fn base(
    category: Category,
    title: String,
    confidence: Confidence,
    reason: String,
) -> Classification {
    Classification {
        category,
        title,
        year: None,
        show_name: None,
        season: None,
        episode: None,
        episode_title: None,
        taken_at: None,
        taken_local: None,
        taken_offset: None,
        confidence,
        reason,
        layout: String::new(),
    }
}

/// Rule 1: photos, dated by EXIF, then the name, then the modified time.
fn photo(
    stem: &str,
    parsed: &ParsedName,
    exif: Option<&ExifInfo>,
    mtime: i64,
    zone: &TimeZone,
) -> Classification {
    let camera = exif.and_then(|e| e.camera.clone());
    let dated = [
        (exif.and_then(|e| e.taken), "dated from its EXIF data"),
        (parsed.date, "dated from its file name"),
        (
            Recorded::instant(mtime),
            "dated from when the file was last changed",
        ),
    ]
    .into_iter()
    .find_map(|(source, why)| Some((source?.resolve(zone)?, why)));
    let source = dated.map_or("with no date we could read", |(_, why)| why);
    let reason = match camera {
        Some(camera) => format!("Photo from {camera}, {source}"),
        None => format!("Photo, {source}"),
    };
    let mut c = base(Category::Photo, stem.to_string(), Confidence::High, reason);
    c.set_taken(dated.map(|(taken, _)| taken));
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
        (
            Confidence::Low,
            format!("Found {marker} in the name, but no show name"),
            marker.clone(),
        )
    } else {
        (
            Confidence::High,
            format!("Found {marker} in the name"),
            format!("{show} S{season:02}E{episode:02}"),
        )
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
            || matches!(
                lower.as_str(),
                "inbox" | "downloads" | "completed" | "in progress" | "shows" | "tv" | "tv shows"
            )
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

/// Torrent folders often hold files with meaningless names ("abc-720p.mkv") while the
/// folder carries the release name. Only used under `Downloads/Completed/`, never for the
/// owner's own folders, where "Vacation 2019/" is not a movie.
fn from_download_folder(rel_path: &str) -> Option<Classification> {
    let rest = rel_path.strip_prefix("Downloads/Completed/")?;
    let mut dirs: Vec<&str> = rest.split('/').collect();
    dirs.pop();
    let folder = *dirs.iter().rev().find(|d| season_folder(d).is_none())?;
    let parsed = parse_name(folder);
    if let Some(mut c) = episode(rel_path, &parsed) {
        c.reason = c.reason.replace("in the name", "in the folder name");
        return Some(c);
    }
    let mut c = movie(&parsed).filter(|c| c.confidence == Confidence::High)?;
    c.reason = c.reason.replace("in the name", "in the folder name");
    Some(c)
}

/// Rule 3: camera metadata or a camera-style file name.
fn home_video(
    stem: &str,
    parsed: &ParsedName,
    probe: Option<&MediaSummary>,
    exif: Option<&ExifInfo>,
    mtime: i64,
    zone: &TimeZone,
) -> Option<Classification> {
    let hint = probe.and_then(camera_hint);
    if hint.is_none() && !parsed.camera_style {
        return None;
    }
    let reason = match &hint {
        Some(h) => h.reason.clone(),
        None => "The name looks like a phone or camera file".to_string(),
    };
    let taken = capture::first_of(
        [
            probe.and_then(creation_time),
            exif.and_then(|e| e.taken),
            parsed.date,
            Recorded::instant(mtime),
        ],
        zone,
    );
    let mut c = base(
        Category::HomeVideo,
        stem.to_string(),
        Confidence::High,
        reason,
    );
    c.set_taken(taken);
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
    base(
        Category::Other,
        stem.to_string(),
        Confidence::Low,
        reason.to_string(),
    )
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
        (Some(make), Some(model))
            if model
                .to_ascii_lowercase()
                .starts_with(&make.to_ascii_lowercase()) =>
        {
            Some(model.to_string())
        }
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (Some(one), None) | (None, Some(one)) => Some(one.to_string()),
        (None, None) => None,
    }
}

fn camera_hint(summary: &MediaSummary) -> Option<CameraHint> {
    let apple = join_make_model(
        tag(summary, "com.apple.quicktime.make"),
        tag(summary, "com.apple.quicktime.model"),
    );
    if let Some(label) = apple {
        return Some(CameraHint {
            reason: format!("Phone video from {label}"),
            label: Some(label),
        });
    }
    let android = join_make_model(
        tag(summary, "com.android.manufacturer"),
        tag(summary, "com.android.model"),
    );
    if let Some(label) = android {
        return Some(CameraHint {
            reason: format!("Phone video from {label}"),
            label: Some(label),
        });
    }
    if tag(summary, "com.android.version").is_some() {
        return Some(CameraHint {
            label: None,
            reason: "Video from an Android phone".to_string(),
        });
    }
    if let Some(label) = join_make_model(tag(summary, "make"), tag(summary, "model")) {
        return Some(CameraHint {
            reason: format!("Camera video from {label}"),
            label: Some(label),
        });
    }
    let handler = tag(summary, "handler_name")
        .or_else(|| tag(summary, "handler"))
        .unwrap_or_default();
    if handler.to_ascii_lowercase().contains("gopro") {
        return Some(CameraHint {
            label: Some("GoPro".to_string()),
            reason: "GoPro video".to_string(),
        });
    }
    let encoder = tag(summary, "encoder").unwrap_or_default();
    if encoder.to_ascii_lowercase().contains("dji") {
        return Some(CameraHint {
            label: Some("DJI".to_string()),
            reason: "DJI camera video".to_string(),
        });
    }
    if encoder.to_ascii_lowercase().contains("gopro") {
        return Some(CameraHint {
            label: Some("GoPro".to_string()),
            reason: "GoPro video".to_string(),
        });
    }
    None
}

/// "Apple iPhone 15 Pro", "GoPro", … from probe tags, when the file came from a camera.
pub fn camera_from_tags(summary: &MediaSummary) -> Option<String> {
    camera_hint(summary).and_then(|h| h.label)
}

/// When the clip was recorded, from its tags: `com.apple.quicktime.creationdate` (the
/// phone's clock with its offset), else `creation_time` (a moment in UTC), else `date`.
/// Placeholder dates (1904, 1970) are ignored.
pub fn creation_time(summary: &MediaSummary) -> Option<Recorded> {
    ["com.apple.quicktime.creationdate", "creation_time", "date"]
        .iter()
        .filter_map(|key| tag(summary, key))
        .find_map(capture::parse_tag_time)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the file goes in the starting libraries (or Other files).
    fn dest(c: &Classification) -> String {
        match c.category {
            Category::Movie => format!("Movies/{}", c.layout),
            Category::Episode => format!("Shows/{}", c.layout),
            Category::HomeVideo => format!("Home Videos/{}", c.layout),
            Category::Photo => format!("Photos/{}", c.layout),
            Category::Other => format!("Other/{}", c.layout),
        }
    }

    fn summary(tags: &[(&str, &str)]) -> MediaSummary {
        MediaSummary {
            tags: tags
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
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
        let exif = ExifInfo {
            taken: Recorded::clock(2024, 6, 13, 10, 15, 0, Some(0)),
            camera: Some("Apple iPhone 15 Pro".into()),
            ..Default::default()
        };
        let c = classify("Inbox/IMG_1234.HEIC", None, Some(&exif), MTIME);
        assert_eq!(c.category, Category::Photo);
        assert_eq!(c.confidence, Confidence::High);
        assert_eq!(c.taken_at, Some(1_718_273_700));
        assert_eq!(dest(&c), "Photos/2024/06/IMG_1234.HEIC");
        assert_eq!(
            c.reason,
            "Photo from Apple iPhone 15 Pro, dated from its EXIF data"
        );

        let c = classify("Inbox/PXL_20230102_030405678.jpg", None, None, MTIME);
        assert_eq!(dest(&c), "Photos/2023/01/PXL_20230102_030405678.jpg");
        assert_eq!(c.reason, "Photo, dated from its file name");

        let c = classify("Inbox/scan.png", None, None, MTIME);
        assert_eq!(c.taken_at, Some(MTIME));
        assert_eq!(dest(&c), "Photos/2023/11/scan.png");
        assert!(c.reason.contains("last changed"));

        // A photo with an episode-like name is still a photo (rule 1 wins).
        let c = classify("Inbox/Show.S01E02.jpg", None, None, MTIME);
        assert_eq!(c.category, Category::Photo);
    }

    #[test]
    fn episodes() {
        let c = classify(
            "Downloads/Completed/The.Office.US.S02E03.720p.WEB-DL.x264-GRP.mkv",
            None,
            None,
            MTIME,
        );
        assert_eq!(c.category, Category::Episode);
        assert_eq!(c.confidence, Confidence::High);
        assert_eq!(c.show_name.as_deref(), Some("The Office US"));
        assert_eq!((c.season, c.episode), (Some(2), Some(3)));
        assert_eq!(c.reason, "Found S02E03 in the name");
        assert_eq!(
            dest(&c),
            "Shows/The Office US/Season 02/The Office US - S02E03.mkv"
        );

        let c = classify("Inbox/Show.Name.S01E02E03.mkv", None, None, MTIME);
        assert_eq!(
            dest(&c),
            "Shows/Show Name/Season 01/Show Name - S01E02-E03.mkv"
        );
        assert_eq!(c.reason, "Found S01E02-E03 in the name");

        // Show name from the folder when the file only has the marker.
        let c = classify(
            "Downloads/Completed/Severance/Season 2/S02E05.mkv",
            None,
            None,
            MTIME,
        );
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
            (
                "com.apple.quicktime.creationdate",
                "2024-06-13T12:21:00+0200",
            ),
            ("creation_time", "2024-06-13T10:21:05.000000Z"),
        ]);
        let c = classify("Inbox/Beach day.mov", Some(&probe), None, MTIME);
        assert_eq!(c.category, Category::HomeVideo);
        assert_eq!(c.confidence, Confidence::High);
        assert_eq!(c.reason, "Phone video from Apple iPhone 15 Pro");
        // The phone's clock and offset from the Apple tag, not the UTC creation_time.
        assert_eq!(c.taken_at, Some(1_718_274_060));
        assert_eq!(c.taken_local.as_deref(), Some("2024-06-13T12:21:00"));
        assert_eq!(c.taken_offset, Some(120));
        assert_eq!(dest(&c), "Home Videos/2024/06/Beach day.mov");
        assert_eq!(
            camera_from_tags(&probe).as_deref(),
            Some("Apple iPhone 15 Pro")
        );

        let probe = summary(&[
            ("com.android.version", "14"),
            ("creation_time", "2023-02-01T08:00:00Z"),
        ]);
        let c = classify("Inbox/clip.mp4", Some(&probe), None, MTIME);
        assert_eq!(c.category, Category::HomeVideo);
        assert_eq!(dest(&c), "Home Videos/2023/02/clip.mp4");

        let probe = summary(&[
            ("handler_name", "\tGoPro AVC  "),
            ("creation_time", "1970-01-01T00:00:00Z"),
        ]);
        let c = classify("Inbox/ride.mp4", Some(&probe), None, MTIME);
        assert_eq!(c.reason, "GoPro video");
        assert_eq!(
            c.taken_at,
            Some(MTIME),
            "placeholder creation_time is ignored"
        );

        let probe = summary(&[("encoder", "DJI Mini 3")]);
        assert_eq!(
            classify("Inbox/x.mp4", Some(&probe), None, MTIME).reason,
            "DJI camera video"
        );

        let probe = summary(&[("make", "Canon"), ("model", "Canon EOS R6")]);
        assert_eq!(
            classify("Inbox/x.mp4", Some(&probe), None, MTIME).reason,
            "Camera video from Canon EOS R6"
        );

        // Release files carry an encoder tag but no camera data.
        let probe = summary(&[("encoder", "Lavf60.3.100")]);
        assert_eq!(
            classify("Inbox/Heat.1995.mkv", Some(&probe), None, MTIME).category,
            Category::Movie
        );
    }

    #[test]
    fn home_videos_from_names() {
        let c = classify("Inbox/VID_20240613_101500.mp4", None, None, MTIME);
        assert_eq!(c.category, Category::HomeVideo);
        assert_eq!(dest(&c), "Home Videos/2024/06/VID_20240613_101500.mp4");
        assert_eq!(c.reason, "The name looks like a phone or camera file");

        let c = classify("Inbox/IMG_1234.MOV", None, None, MTIME);
        assert_eq!(dest(&c), "Home Videos/2023/11/IMG_1234.MOV");

        for name in [
            "GOPR0123.MP4",
            "GX010123.MP4",
            "DJI_0042.MP4",
            "MVI_1234.MOV",
            "RPReplay_Final1690000000.MP4",
        ] {
            let c = classify(&format!("Inbox/{name}"), None, None, MTIME);
            assert_eq!(c.category, Category::HomeVideo, "{name}");
            assert_eq!(c.confidence, Confidence::High, "{name}");
        }
        let c = classify(
            "Inbox/Screen Recording 2024-06-13 at 10.15.00.mov",
            None,
            None,
            MTIME,
        );
        assert_eq!(
            dest(&c),
            "Home Videos/2024/06/Screen Recording 2024-06-13 at 10.15.00.mov"
        );
    }

    #[test]
    fn movies() {
        let c = classify(
            "Downloads/Completed/Heat.1995.1080p.BluRay.x264-GRP/Heat.1995.1080p.BluRay.x264-GRP.mkv",
            None,
            None,
            MTIME,
        );
        assert_eq!(c.category, Category::Movie);
        assert_eq!(c.confidence, Confidence::High);
        assert_eq!((c.title.as_str(), c.year), ("Heat", Some(1995)));
        assert_eq!(c.reason, "Found the year 1995 in the name");
        assert_eq!(dest(&c), "Movies/Heat (1995)/Heat (1995).mkv");

        let c = classify("Inbox/Blade Runner 2049 (2017).MP4", None, None, MTIME);
        assert_eq!(
            dest(&c),
            "Movies/Blade Runner 2049 (2017)/Blade Runner 2049 (2017).mp4"
        );

        let c = classify("Inbox/Some.Movie.1080p.BluRay.x264.mkv", None, None, MTIME);
        assert_eq!(c.category, Category::Movie);
        assert_eq!(c.confidence, Confidence::Low);
        assert_eq!(dest(&c), "Movies/Some Movie/Some Movie.mkv");
    }

    #[test]
    fn download_folder_names_help_with_meaningless_file_names() {
        let c = classify(
            "Downloads/Completed/Heat.1995.1080p.BluRay.x264-GRP/grp-heat-1080p.mkv",
            None,
            None,
            MTIME,
        );
        assert_eq!(
            (c.category, c.confidence),
            (Category::Movie, Confidence::High)
        );
        assert_eq!(c.reason, "Found the year 1995 in the folder name");
        assert_eq!(dest(&c), "Movies/Heat (1995)/Heat (1995).mkv");

        let c = classify(
            "Downloads/Completed/Show.Name.S01E05.720p.WEB/abc.mkv",
            None,
            None,
            MTIME,
        );
        assert_eq!(
            (c.category, c.confidence),
            (Category::Episode, Confidence::High)
        );
        assert_eq!(c.reason, "Found S01E05 in the folder name");
        assert_eq!(dest(&c), "Shows/Show Name/Season 01/Show Name - S01E05.mkv");

        // A good file name still wins over the folder.
        let c = classify(
            "Downloads/Completed/Heat.1995.1080p/Heat.1995.Directors.Cut.1080p.mkv",
            None,
            None,
            MTIME,
        );
        assert_eq!(c.reason, "Found the year 1995 in the name");

        // Never for the owner's folders or the Inbox: "Vacation 2019" is not a movie.
        assert_eq!(
            classify("Inbox/Vacation 2019/clip.mp4", None, None, MTIME).category,
            Category::Other
        );
        assert_eq!(
            classify("Family 2019/clip.mp4", None, None, MTIME).category,
            Category::Other
        );
    }

    #[test]
    fn others() {
        let c = classify("Inbox/Our Wedding.mp4", None, None, MTIME);
        assert_eq!(c.category, Category::Other);
        assert_eq!(c.confidence, Confidence::Low);
        assert_eq!(dest(&c), "Other/Our Wedding.mp4");

        let c = classify("Inbox/song.mp3", None, None, MTIME);
        assert_eq!(c.category, Category::Other);
        assert_eq!(c.confidence, Confidence::High);
        assert_eq!(dest(&c), "Other/song.mp3");

        let c = classify("Inbox/notes.pdf", None, None, MTIME);
        assert_eq!(c.category, Category::Other);
    }

    fn zone(name: &str) -> TimeZone {
        TimeZone::get(name).unwrap()
    }

    /// 2024-07-01T02:30:00Z: the evening of June 30 in New York, July 1 in UTC and Tokyo.
    const MONTH_EDGE: i64 = 1_719_801_000;

    /// Section 4, Capture time: what each kind of date becomes, wherever the server is.
    #[test]
    fn capture_times_follow_the_camera_clock() {
        let servers = ["UTC", "America/New_York", "Asia/Tokyo"];
        // EXIF with an offset: the clock and the moment, the same on every server.
        let exif = ExifInfo {
            taken: Recorded::clock(2024, 6, 30, 23, 30, 0, Some(-240)),
            ..Default::default()
        };
        for server in servers {
            let c = classify_in("Inbox/IMG_0001.HEIC", None, Some(&exif), 5, &zone(server));
            assert_eq!(
                c.taken_local.as_deref(),
                Some("2024-06-30T23:30:00"),
                "{server}"
            );
            assert_eq!(c.taken_offset, Some(-240), "{server}");
            assert_eq!(c.taken_at, Some(1_719_804_600), "{server}");
            assert_eq!(dest(&c), "Photos/2024/06/IMG_0001.HEIC", "{server}");
        }
        // EXIF without an offset: the clock as it is, never UTC; the moment from the server.
        let exif = ExifInfo {
            taken: Recorded::clock(2024, 6, 30, 23, 30, 0, None),
            ..Default::default()
        };
        for server in servers {
            let c = classify_in("Inbox/DSC01234.JPG", None, Some(&exif), 5, &zone(server));
            assert_eq!(
                c.taken_local.as_deref(),
                Some("2024-06-30T23:30:00"),
                "{server}"
            );
            assert_eq!(c.taken_offset, None, "{server}");
            assert_eq!(dest(&c), "Photos/2024/06/DSC01234.JPG", "{server}");
        }
        let c = classify_in(
            "Inbox/DSC01234.JPG",
            None,
            Some(&exif),
            5,
            &zone("America/New_York"),
        );
        assert_eq!(c.taken_at, Some(1_719_804_600), "23:30 EDT");
        // A QuickTime date with a zone keeps both.
        let probe = summary(&[
            ("com.apple.quicktime.make", "Apple"),
            (
                "com.apple.quicktime.creationdate",
                "2024-06-30T23:30:00-0400",
            ),
            ("creation_time", "2024-07-01T03:30:00.000000Z"),
        ]);
        for server in servers {
            let c = classify_in("Inbox/IMG_0002.MOV", Some(&probe), None, 5, &zone(server));
            assert_eq!(
                c.taken_local.as_deref(),
                Some("2024-06-30T23:30:00"),
                "{server}"
            );
            assert_eq!(c.taken_offset, Some(-240), "{server}");
            assert_eq!(dest(&c), "Home Videos/2024/06/IMG_0002.MOV", "{server}");
        }
        // A container creation time alone is a moment in UTC: its clock comes from the
        // server's zone.
        let probe = summary(&[
            ("com.android.version", "14"),
            ("creation_time", "2024-07-01T02:30:00.000000Z"),
        ]);
        let clip =
            |server: &str| classify_in("Inbox/clip.mp4", Some(&probe), None, 5, &zone(server));
        assert_eq!(
            clip("America/New_York").taken_local.as_deref(),
            Some("2024-06-30T22:30:00")
        );
        assert_eq!(
            dest(&clip("America/New_York")),
            "Home Videos/2024/06/clip.mp4"
        );
        assert_eq!(dest(&clip("UTC")), "Home Videos/2024/07/clip.mp4");
        assert_eq!(
            clip("Asia/Tokyo").taken_local.as_deref(),
            Some("2024-07-01T11:30:00")
        );
        for server in servers {
            assert_eq!(clip(server).taken_at, Some(MONTH_EDGE));
            assert_eq!(clip(server).taken_offset, None);
        }
        // A QuickTime date without a zone is a bare clock.
        let probe = summary(&[("make", "Canon"), ("date", "2024-06-30 23:30:00")]);
        let c = classify_in(
            "Inbox/MVI_0001.MP4",
            Some(&probe),
            None,
            5,
            &zone("Asia/Tokyo"),
        );
        assert_eq!(c.taken_local.as_deref(), Some("2024-06-30T23:30:00"));
        assert_eq!(c.taken_offset, None);
        assert_eq!(dest(&c), "Home Videos/2024/06/MVI_0001.MP4");
        // Only a file time: the server's zone gives the clock, and with it the month.
        let scan =
            |server: &str| classify_in("Inbox/scan.png", None, None, MONTH_EDGE, &zone(server));
        assert_eq!(
            scan("America/New_York").taken_local.as_deref(),
            Some("2024-06-30T22:30:00")
        );
        assert_eq!(dest(&scan("America/New_York")), "Photos/2024/06/scan.png");
        assert_eq!(
            scan("UTC").taken_local.as_deref(),
            Some("2024-07-01T02:30:00")
        );
        assert_eq!(dest(&scan("UTC")), "Photos/2024/07/scan.png");
        assert_eq!(dest(&scan("Asia/Tokyo")), "Photos/2024/07/scan.png");
        for server in servers {
            assert_eq!(scan(server).taken_at, Some(MONTH_EDGE));
            assert_eq!(scan(server).taken_offset, None);
        }
        // A file name's date is the camera's clock; a Pixel's is UTC.
        let named = classify_in(
            "Inbox/IMG_20240630_233000.jpg",
            None,
            None,
            5,
            &zone("Asia/Tokyo"),
        );
        assert_eq!(named.taken_local.as_deref(), Some("2024-06-30T23:30:00"));
        let pixel = classify_in(
            "Inbox/PXL_20240701_023000000.jpg",
            None,
            None,
            5,
            &zone("America/New_York"),
        );
        assert_eq!(pixel.taken_local.as_deref(), Some("2024-06-30T22:30:00"));
        assert_eq!(pixel.taken_at, Some(MONTH_EDGE));
        // A file time of zero (a clock never set) leaves the photo undated.
        let c = classify_in("Inbox/scan.png", None, None, 0, &TimeZone::UTC);
        assert_eq!(c.taken_at, None);
        assert_eq!(dest(&c), "Photos/Undated/scan.png");
    }
}
