//! ffprobe wrapper.
//!
//! Rotation is stored clockwise in degrees (0, 90, 180, 270), the convention of
//! the legacy `rotate` tag. ffprobe's display-matrix `rotation` is counter-clockwise,
//! so an iPhone portrait clip (`rotation: -90`) is stored as 90.
//!
//! Privacy: location tags (GPS coordinates) never leave this module.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::tools;
use crate::models::{AudioStream, MediaSummary, SubtitleStream, VideoStream};
use crate::{CoreError, Result};

/// ffprobe may take a while on network shares, but never longer than this.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// External subtitle extensions picked up next to a video, with their ffmpeg codec names.
const SIDECAR_SUBTITLES: [(&str, &str); 3] = [("srt", "subrip"), ("vtt", "webvtt"), ("ass", "ass")];

/// Longest tag value kept in the summary; longer values (lyrics, descriptions) are cut.
const MAX_TAG_VALUE: usize = 512;

/// `ffprobe -v error -print_format json -show_format -show_streams <path>` → summary,
/// including external same-stem subtitle files.
pub async fn probe(path: &Path) -> Result<MediaSummary> {
    let mut cmd = tools::command(tools::FFPROBE);
    cmd.args([
        "-v",
        "error",
        "-print_format",
        "json",
        "-show_format",
        "-show_streams",
    ])
    .arg(tools::ffmpeg_input(path));
    let output = tools::run(cmd, tools::FFPROBE, PROBE_TIMEOUT).await?;
    if !output.status.success() {
        return Err(CoreError::external(format!(
            "This file could not be read as media ({}).",
            tools::stderr_summary(&output.stderr)
        )));
    }
    let json = String::from_utf8_lossy(&output.stdout);
    let mut summary = parse_ffprobe_json(&json)?;
    let next_index = next_stream_index(&json);
    summary
        .subtitles
        .extend(external_subtitles(path, next_index).await);
    Ok(summary)
}

/// Parse ffprobe JSON output (separated for unit tests).
pub fn parse_ffprobe_json(json: &str) -> Result<MediaSummary> {
    let raw: RawProbe = serde_json::from_str(json).map_err(|e| {
        CoreError::external(format!(
            "ffprobe returned output that could not be read: {e}"
        ))
    })?;
    let format = raw.format.unwrap_or_default();

    let video_raw = raw
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video") && !s.is_attached_pic());

    let video = video_raw.map(|s| VideoStream {
        index: s.index,
        codec: s.codec(),
        profile: s.profile.clone().filter(|p| !p.is_empty()),
        pix_fmt: s.pix_fmt.clone().filter(|p| !p.is_empty()),
        width: s.width.unwrap_or(0),
        height: s.height.unwrap_or(0),
        fps: parse_rate(s.avg_frame_rate.as_deref())
            .or_else(|| parse_rate(s.r_frame_rate.as_deref())),
        hdr: detect_hdr(s),
        rotation: detect_rotation(s),
    });

    let audio = raw
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("audio"))
        .map(|s| AudioStream {
            index: s.index,
            codec: s.codec(),
            channels: s.channels,
            language: s.tag("language"),
            title: s.tag("title"),
        })
        .collect();

    let subtitles = raw
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("subtitle"))
        .map(|s| SubtitleStream {
            index: s.index,
            codec: s.codec(),
            language: s.tag("language"),
            title: s.tag("title"),
            external: false,
        })
        .collect();

    let duration_s = parse_f64(format.duration.as_ref())
        .filter(|d| *d > 0.0)
        .or_else(|| {
            video_raw
                .and_then(|s| parse_f64(s.duration.as_ref()))
                .filter(|d| *d > 0.0)
        })
        .or_else(|| {
            raw.streams
                .iter()
                .filter_map(|s| parse_f64(s.duration.as_ref()))
                .filter(|d| *d > 0.0)
                .reduce(f64::max)
        })
        .map(|d| (d * 1000.0).round() / 1000.0);

    let mut tags = BTreeMap::new();
    merge_tags(&mut tags, &format.tags);
    if let Some(stream) = video_raw {
        merge_tags(&mut tags, &stream.tags);
    }
    add_camera_aliases(&mut tags);

    Ok(MediaSummary {
        container: format.format_name.filter(|f| !f.is_empty()),
        duration_s,
        bitrate: parse_f64(format.bit_rate.as_ref())
            .map(|b| b as i64)
            .filter(|b| *b > 0),
        video,
        audio,
        subtitles,
        tags,
    })
}

/// External subtitle files next to `path` with the same stem: `Movie.srt`, `Movie.en.srt`,
/// `Movie.eng.forced.vtt`. Indexes continue after the file's own streams.
pub async fn external_subtitles(path: &Path, first_index: i64) -> Vec<SubtitleStream> {
    let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str())) else {
        return Vec::new();
    };
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return Vec::new();
    };
    let mut names = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    names.sort();
    names
        .iter()
        .filter_map(|name| sidecar_subtitle(stem, name))
        .enumerate()
        .map(|(i, (codec, language, name))| SubtitleStream {
            index: first_index + i as i64,
            codec: codec.to_string(),
            language,
            title: Some(name),
            external: true,
        })
        .collect()
}

/// If `name` is a subtitle sidecar for `stem`, return (codec, language, file name).
fn sidecar_subtitle(stem: &str, name: &str) -> Option<(&'static str, Option<String>, String)> {
    let (base, ext) = name.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    let codec = SIDECAR_SUBTITLES
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, c)| *c)?;
    let rest = if base == stem {
        ""
    } else {
        base.strip_prefix(stem)?.strip_prefix('.')?
    };
    let language = rest
        .split('.')
        .find(|part| is_language_code(part))
        .map(str::to_ascii_lowercase);
    Some((codec, language, name.to_string()))
}

/// `en`, `eng`, `pt-BR`, `zh_Hans`-style codes. Words like `forced` or `sdh` are not languages.
fn is_language_code(part: &str) -> bool {
    let (lang, region) = match part.split_once(['-', '_']) {
        Some((l, r)) => (l, Some(r)),
        None => (part, None),
    };
    let lang_ok = (2..=3).contains(&lang.len()) && lang.chars().all(|c| c.is_ascii_alphabetic());
    let region_ok = region
        .is_none_or(|r| (2..=4).contains(&r.len()) && r.chars().all(|c| c.is_ascii_alphanumeric()));
    let marker_word = matches!(
        part.to_ascii_lowercase().as_str(),
        "sdh" | "cc" | "dub" | "sub"
    );
    lang_ok && region_ok && !marker_word
}

fn next_stream_index(json: &str) -> i64 {
    serde_json::from_str::<RawProbe>(json)
        .ok()
        .and_then(|raw| raw.streams.iter().map(|s| s.index).max())
        .map_or(0, |max| max + 1)
}

#[derive(Debug, Default, Deserialize)]
struct RawProbe {
    #[serde(default)]
    streams: Vec<RawStream>,
    #[serde(default)]
    format: Option<RawFormat>,
}

#[derive(Debug, Default, Deserialize)]
struct RawFormat {
    format_name: Option<String>,
    duration: Option<Value>,
    bit_rate: Option<Value>,
    #[serde(default)]
    tags: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
struct RawStream {
    #[serde(default)]
    index: i64,
    codec_name: Option<String>,
    codec_type: Option<String>,
    codec_tag_string: Option<String>,
    profile: Option<String>,
    pix_fmt: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    color_transfer: Option<String>,
    channels: Option<i64>,
    duration: Option<Value>,
    #[serde(default)]
    disposition: BTreeMap<String, Value>,
    #[serde(default)]
    tags: BTreeMap<String, Value>,
    #[serde(default)]
    side_data_list: Vec<Value>,
}

impl RawStream {
    fn codec(&self) -> String {
        self.codec_name
            .clone()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn is_attached_pic(&self) -> bool {
        self.disposition.get("attached_pic").and_then(Value::as_i64) == Some(1)
    }

    /// A tag value by case-insensitive key, ignoring blanks.
    fn tag(&self, key: &str) -> Option<String> {
        self.tags
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .and_then(|(_, v)| value_to_string(v))
            .filter(|v| !v.is_empty())
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn parse_f64(value: Option<&Value>) -> Option<f64> {
    let v = match value? {
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    v.is_finite().then_some(v)
}

/// `30000/1001` → 29.97 (three decimals). None for `0/0` or garbage.
fn parse_rate(rate: Option<&str>) -> Option<f64> {
    let rate = rate?.trim();
    let value = match rate.split_once('/') {
        Some((n, d)) => {
            let (n, d) = (n.trim().parse::<f64>().ok()?, d.trim().parse::<f64>().ok()?);
            if d == 0.0 {
                return None;
            }
            n / d
        }
        None => rate.parse::<f64>().ok()?,
    };
    (value.is_finite() && value > 0.0).then(|| (value * 1000.0).round() / 1000.0)
}

fn detect_hdr(stream: &RawStream) -> Option<String> {
    let dovi_side_data = stream.side_data_list.iter().any(|sd| {
        sd.get("side_data_type")
            .and_then(Value::as_str)
            .is_some_and(|t| t.to_ascii_lowercase().contains("dovi"))
    });
    let dovi_tag = matches!(
        stream.codec_tag_string.as_deref(),
        Some("dvh1" | "dvhe" | "dva1" | "dvav")
    );
    if dovi_side_data || dovi_tag {
        return Some("dolby_vision".to_string());
    }
    match stream.color_transfer.as_deref() {
        Some("smpte2084") => Some("hdr10".to_string()),
        Some("arib-std-b67") => Some("hlg".to_string()),
        _ => None,
    }
}

/// Clockwise display rotation, normalized to 0/90/180/270.
fn detect_rotation(stream: &RawStream) -> i64 {
    let from_matrix = stream.side_data_list.iter().find_map(|sd| {
        let is_matrix = sd
            .get("side_data_type")
            .and_then(Value::as_str)
            .is_none_or(|t| t.eq_ignore_ascii_case("display matrix"));
        if !is_matrix {
            return None;
        }
        match sd.get("rotation")? {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    });
    if let Some(ccw) = from_matrix {
        return normalize_rotation(-ccw);
    }
    stream
        .tag("rotate")
        .and_then(|r| r.parse::<f64>().ok())
        .map(normalize_rotation)
        .unwrap_or(0)
}

/// Any angle → nearest of 0, 90, 180, 270.
pub fn normalize_rotation(degrees: f64) -> i64 {
    if !degrees.is_finite() {
        return 0;
    }
    let quarter = (degrees / 90.0).round() as i64;
    quarter.rem_euclid(4) * 90
}

/// Location and GPS tags are never stored.
pub fn is_private_tag(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("location")
        || key.starts_with("com.apple.quicktime.location")
        || key.contains("gps")
        || key == "©xyz"
        || key == "xyz"
        || key.ends_with(".xyz")
}

/// Insert tags with lowercase keys. Earlier sources win: format tags beat stream tags.
fn merge_tags(into: &mut BTreeMap<String, String>, tags: &BTreeMap<String, Value>) {
    for (key, value) in tags {
        let key = key.trim().to_ascii_lowercase();
        if key.is_empty() || key.len() > 128 || is_private_tag(&key) || into.contains_key(&key) {
            continue;
        }
        let Some(mut value) = value_to_string(value) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        if value.chars().count() > MAX_TAG_VALUE {
            value = value.chars().take(MAX_TAG_VALUE).collect();
        }
        into.insert(key, value);
    }
}

/// Phones store make/model under vendor keys; add plain `make`/`model` so every reader
/// (classifier, camera field) finds them in one place.
fn add_camera_aliases(tags: &mut BTreeMap<String, String>) {
    for (alias, sources) in [
        (
            "make",
            ["com.apple.quicktime.make", "com.android.manufacturer"],
        ),
        ("model", ["com.apple.quicktime.model", "com.android.model"]),
    ] {
        if tags.contains_key(alias) {
            continue;
        }
        if let Some(value) = sources.iter().find_map(|k| tags.get(*k)).cloned() {
            tags.insert(alias.to_string(), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// iPhone 15 Pro portrait clip: HEVC Main 10 HLG, rotation -90 in the display matrix,
    /// Apple make/model/creation date and a GPS location that must be dropped.
    const IPHONE_HLG: &str = r#"{
      "streams": [
        {"index": 0, "codec_name": "hevc", "profile": "Main 10", "codec_type": "video", "codec_tag_string": "hvc1",
         "width": 3840, "height": 2160, "pix_fmt": "yuv420p10le", "color_range": "tv", "color_space": "bt2020nc",
         "color_transfer": "arib-std-b67", "color_primaries": "bt2020",
         "r_frame_rate": "30/1", "avg_frame_rate": "16380/547", "duration": "63.200000", "bit_rate": "11800000",
         "disposition": {"default": 1, "attached_pic": 0},
         "tags": {"creation_time": "2024-06-13T10:21:00.000000Z", "language": "und", "handler_name": "Core Media Video",
                  "vendor_id": "[0][0][0][0]", "encoder": "HEVC"},
         "side_data_list": [{"side_data_type": "Display Matrix",
                             "displaymatrix": "\n00000000:            0       65536           0\n",
                             "rotation": -90}]},
        {"index": 1, "codec_name": "aac", "profile": "LC", "codec_type": "audio", "sample_rate": "44100",
         "channels": 2, "channel_layout": "stereo", "duration": "63.200000",
         "disposition": {"default": 1, "attached_pic": 0},
         "tags": {"creation_time": "2024-06-13T10:21:00.000000Z", "language": "und", "handler_name": "Core Media Audio"}},
        {"index": 2, "codec_type": "data", "codec_tag_string": "mebx", "tags": {"handler_name": "Core Media Metadata"}}
      ],
      "format": {"filename": "IMG_0001.MOV", "nb_streams": 3, "format_name": "mov,mp4,m4a,3gp,3g2,mj2",
                 "duration": "63.200000", "size": "93000000", "bit_rate": "12000000",
                 "tags": {"major_brand": "qt  ", "creation_time": "2024-06-13T10:21:00.000000Z",
                          "com.apple.quicktime.location.accuracy.horizontal": "4.7",
                          "com.apple.quicktime.location.ISO6709": "+37.7749-122.4194+010.000/",
                          "com.apple.quicktime.make": "Apple", "com.apple.quicktime.model": "iPhone 15 Pro",
                          "com.apple.quicktime.software": "17.5",
                          "com.apple.quicktime.creationdate": "2024-06-13T12:21:00+0200"}}
    }"#;

    /// Matroska release: H.264, two audio tracks, embedded subtitles.
    const MKV_H264: &str = r#"{
      "streams": [
        {"index": 0, "codec_name": "h264", "profile": "High", "codec_type": "video", "width": 1920, "height": 1080,
         "pix_fmt": "yuv420p", "color_transfer": "bt709", "r_frame_rate": "24000/1001", "avg_frame_rate": "24000/1001",
         "disposition": {"default": 1, "attached_pic": 0},
         "tags": {"BPS": "8000000", "DURATION": "01:58:00.000000000", "language": "eng"}},
        {"index": 1, "codec_name": "eac3", "codec_type": "audio", "channels": 6, "channel_layout": "5.1(side)",
         "disposition": {"default": 1}, "tags": {"language": "eng", "title": "Surround 5.1"}},
        {"index": 2, "codec_name": "aac", "codec_type": "audio", "channels": 2,
         "disposition": {"default": 0}, "tags": {"language": "fre", "title": "Stereo"}},
        {"index": 3, "codec_name": "subrip", "codec_type": "subtitle", "disposition": {"default": 0, "forced": 0},
         "tags": {"language": "eng"}},
        {"index": 4, "codec_name": "hdmv_pgs_subtitle", "codec_type": "subtitle", "disposition": {"forced": 1},
         "tags": {"language": "spa", "title": "Forced"}}
      ],
      "format": {"format_name": "matroska,webm", "duration": "7080.042000", "bit_rate": "9500000",
                 "tags": {"title": "Heat", "ENCODER": "libebml v1.4.2 + libmatroska v1.6.4", "creation_time": "2019-03-01T00:00:00.000000Z"}}
    }"#;

    /// Audio-only MP3 with ID3 tags and embedded cover art.
    const MP3_COVER: &str = r#"{
      "streams": [
        {"index": 0, "codec_name": "mp3", "codec_type": "audio", "sample_rate": "44100", "channels": 2,
         "avg_frame_rate": "0/0", "r_frame_rate": "0/0", "duration": "215.484082", "bit_rate": "320000",
         "disposition": {"default": 0, "attached_pic": 0}, "tags": {"encoder": "LAME3.100"}},
        {"index": 1, "codec_name": "mjpeg", "profile": "Baseline", "codec_type": "video", "width": 600, "height": 600,
         "pix_fmt": "yuvj420p", "r_frame_rate": "90000/1", "avg_frame_rate": "0/0",
         "disposition": {"default": 0, "attached_pic": 1}, "tags": {"comment": "Cover (front)"}}
      ],
      "format": {"format_name": "mp3", "duration": "215.484082", "bit_rate": "320712",
                 "tags": {"title": "Song", "artist": "Band", "album": "Record", "date": "2001"}}
    }"#;

    /// HDR10 UHD file with a legacy rotate tag and mastering metadata.
    const HDR10: &str = r#"{
      "streams": [
        {"index": 0, "codec_name": "hevc", "profile": "Main 10", "codec_type": "video", "width": 3840, "height": 2160,
         "pix_fmt": "yuv420p10le", "color_space": "bt2020nc", "color_transfer": "smpte2084", "color_primaries": "bt2020",
         "r_frame_rate": "24000/1001", "avg_frame_rate": "24000/1001",
         "disposition": {"default": 1, "attached_pic": 0}, "tags": {"rotate": "180", "language": "und"},
         "side_data_list": [{"side_data_type": "Mastering display metadata", "red_x": "34000/50000"},
                            {"side_data_type": "Content light level metadata", "max_content": 1000, "max_average": 400}]},
        {"index": 1, "codec_name": "truehd", "codec_type": "audio", "channels": 8, "tags": {"language": "eng"}}
      ],
      "format": {"format_name": "matroska,webm", "duration": "5400.5", "bit_rate": "60000000", "tags": {}}
    }"#;

    const DOLBY_VISION: &str = r#"{
      "streams": [
        {"index": 0, "codec_name": "hevc", "codec_type": "video", "codec_tag_string": "dvh1", "width": 3840, "height": 2160,
         "color_transfer": "smpte2084", "avg_frame_rate": "24/1",
         "side_data_list": [{"side_data_type": "DOVI configuration record", "dv_profile": 8, "dv_bl_signal_compatibility_id": 1}]}
      ],
      "format": {"format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "10.0"}
    }"#;

    #[test]
    fn iphone_hevc_hlg_with_rotation() {
        let m = parse_ffprobe_json(IPHONE_HLG).unwrap();
        assert_eq!(m.container.as_deref(), Some("mov,mp4,m4a,3gp,3g2,mj2"));
        assert_eq!(m.duration_s, Some(63.2));
        assert_eq!(m.bitrate, Some(12_000_000));
        let v = m.video.as_ref().unwrap();
        assert_eq!(v.index, 0);
        assert_eq!(v.codec, "hevc");
        assert_eq!(v.profile.as_deref(), Some("Main 10"));
        assert_eq!(v.pix_fmt.as_deref(), Some("yuv420p10le"));
        assert_eq!((v.width, v.height), (3840, 2160));
        assert_eq!(v.fps, Some(29.945));
        assert_eq!(v.hdr.as_deref(), Some("hlg"));
        assert_eq!(v.rotation, 90);
        assert_eq!(m.audio.len(), 1);
        assert_eq!(m.audio[0].codec, "aac");
        assert_eq!(m.audio[0].channels, Some(2));
        assert_eq!(m.audio[0].language.as_deref(), Some("und"));
        assert!(m.subtitles.is_empty());
        // Tags: lowercase, merged, location dropped, camera aliases added.
        assert_eq!(m.tags.get("make").map(String::as_str), Some("Apple"));
        assert_eq!(
            m.tags.get("model").map(String::as_str),
            Some("iPhone 15 Pro")
        );
        assert_eq!(
            m.tags.get("com.apple.quicktime.make").map(String::as_str),
            Some("Apple")
        );
        assert_eq!(
            m.tags.get("creation_time").map(String::as_str),
            Some("2024-06-13T10:21:00.000000Z")
        );
        assert_eq!(
            m.tags.get("handler_name").map(String::as_str),
            Some("Core Media Video")
        );
        assert!(
            m.tags.keys().all(|k| !k.contains("location")),
            "{:?}",
            m.tags
        );
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("37.7749"));
    }

    #[test]
    fn mkv_two_audio_tracks_and_subtitles() {
        let m = parse_ffprobe_json(MKV_H264).unwrap();
        assert_eq!(m.container.as_deref(), Some("matroska,webm"));
        assert_eq!(m.duration_s, Some(7080.042));
        let v = m.video.as_ref().unwrap();
        assert_eq!(v.codec, "h264");
        assert_eq!(v.fps, Some(23.976));
        assert_eq!(v.hdr, None);
        assert_eq!(v.rotation, 0);
        assert_eq!(m.audio.len(), 2);
        assert_eq!(m.audio[0].index, 1);
        assert_eq!(m.audio[0].codec, "eac3");
        assert_eq!(m.audio[0].channels, Some(6));
        assert_eq!(m.audio[0].title.as_deref(), Some("Surround 5.1"));
        assert_eq!(m.audio[1].index, 2);
        assert_eq!(m.audio[1].language.as_deref(), Some("fre"));
        assert_eq!(m.subtitles.len(), 2);
        assert_eq!(m.subtitles[0].codec, "subrip");
        assert_eq!(m.subtitles[0].language.as_deref(), Some("eng"));
        assert!(!m.subtitles[0].external);
        assert_eq!(m.subtitles[1].title.as_deref(), Some("Forced"));
        // Video stream tags are merged in with lowercase keys.
        assert_eq!(m.tags.get("bps").map(String::as_str), Some("8000000"));
        assert_eq!(
            m.tags.get("encoder").map(String::as_str),
            Some("libebml v1.4.2 + libmatroska v1.6.4")
        );
        assert_eq!(m.tags.get("title").map(String::as_str), Some("Heat"));
    }

    #[test]
    fn audio_only_mp3_skips_cover_art() {
        let m = parse_ffprobe_json(MP3_COVER).unwrap();
        assert_eq!(m.container.as_deref(), Some("mp3"));
        assert!(
            m.video.is_none(),
            "attached cover art is not a video stream"
        );
        assert_eq!(m.audio.len(), 1);
        assert_eq!(m.audio[0].codec, "mp3");
        assert_eq!(m.duration_s, Some(215.484));
        assert_eq!(m.bitrate, Some(320_712));
        assert_eq!(m.tags.get("artist").map(String::as_str), Some("Band"));
    }

    #[test]
    fn hdr10_and_rotate_tag() {
        let m = parse_ffprobe_json(HDR10).unwrap();
        let v = m.video.as_ref().unwrap();
        assert_eq!(v.hdr.as_deref(), Some("hdr10"));
        assert_eq!(v.rotation, 180);
        assert_eq!(m.duration_s, Some(5400.5));
        assert_eq!(m.audio[0].codec, "truehd");
        assert_eq!(m.audio[0].channels, Some(8));
        assert_eq!(m.tags.get("rotate").map(String::as_str), Some("180"));
    }

    #[test]
    fn dolby_vision_from_side_data() {
        let m = parse_ffprobe_json(DOLBY_VISION).unwrap();
        assert_eq!(m.video.unwrap().hdr.as_deref(), Some("dolby_vision"));
        assert!(m.audio.is_empty());
        assert_eq!(m.bitrate, None);
    }

    #[test]
    fn garbage_is_an_error_and_empty_is_fine() {
        assert!(parse_ffprobe_json("not json").is_err());
        let m = parse_ffprobe_json("{}").unwrap();
        assert_eq!(m, MediaSummary::default());
        // Zero or negative durations are not real durations.
        let m =
            parse_ffprobe_json(r#"{"format": {"duration": "0.000000", "format_name": "image2"}}"#)
                .unwrap();
        assert_eq!(m.duration_s, None);
    }

    #[test]
    fn rotation_normalizes() {
        assert_eq!(normalize_rotation(0.0), 0);
        assert_eq!(normalize_rotation(90.0), 90);
        assert_eq!(normalize_rotation(-90.0), 270);
        assert_eq!(normalize_rotation(-270.0), 90);
        assert_eq!(normalize_rotation(180.0), 180);
        assert_eq!(normalize_rotation(-180.0), 180);
        assert_eq!(normalize_rotation(450.0), 90);
        assert_eq!(normalize_rotation(89.6), 90);
        assert_eq!(normalize_rotation(f64::NAN), 0);
    }

    #[test]
    fn frame_rates() {
        assert_eq!(parse_rate(Some("30000/1001")), Some(29.97));
        assert_eq!(parse_rate(Some("25/1")), Some(25.0));
        assert_eq!(parse_rate(Some("0/0")), None);
        assert_eq!(parse_rate(Some("x/y")), None);
        assert_eq!(parse_rate(None), None);
    }

    #[test]
    fn private_tags() {
        for key in [
            "location",
            "location-eng",
            "com.apple.quicktime.location.ISO6709",
            "©xyz",
            "GPSCoordinates",
            "com.android.gps",
        ] {
            assert!(is_private_tag(key), "{key}");
        }
        for key in ["make", "model", "creation_time", "encoder", "handler_name"] {
            assert!(!is_private_tag(key), "{key}");
        }
    }

    #[test]
    fn sidecar_names() {
        assert_eq!(
            sidecar_subtitle("Heat (1995)", "Heat (1995).srt"),
            Some(("subrip", None, "Heat (1995).srt".to_string()))
        );
        assert_eq!(
            sidecar_subtitle("Heat (1995)", "Heat (1995).en.srt"),
            Some((
                "subrip",
                Some("en".to_string()),
                "Heat (1995).en.srt".to_string()
            ))
        );
        assert_eq!(
            sidecar_subtitle("Heat (1995)", "Heat (1995).eng.forced.VTT").map(|s| (s.0, s.1)),
            Some(("webvtt", Some("eng".to_string())))
        );
        assert_eq!(
            sidecar_subtitle("Heat (1995)", "Heat (1995).sdh.ass").map(|s| s.1),
            Some(None)
        );
        assert_eq!(
            sidecar_subtitle("Heat (1995)", "Heat (1995).pt-BR.srt").map(|s| s.1),
            Some(Some("pt-br".into()))
        );
        assert_eq!(sidecar_subtitle("Heat (1995)", "Heat (1995).mkv"), None);
        assert_eq!(
            sidecar_subtitle("Heat (1995)", "Heat (1995) extras.srt"),
            None
        );
        assert_eq!(sidecar_subtitle("Heat", "Heater.srt"), None);
        assert_eq!(sidecar_subtitle("Heat (1995)", "Other.en.srt"), None);
    }

    #[tokio::test]
    async fn external_subtitles_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("Movie.mkv");
        std::fs::write(&video, b"x").unwrap();
        for name in [
            "Movie.en.srt",
            "Movie.srt",
            "Movie.fr.vtt",
            "Movie.nfo",
            "Other.srt",
        ] {
            std::fs::write(dir.path().join(name), b"1").unwrap();
        }
        let subs = external_subtitles(&video, 3).await;
        assert_eq!(subs.len(), 3, "{subs:?}");
        assert!(subs.iter().all(|s| s.external));
        assert_eq!(
            subs.iter().map(|s| s.index).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        let en = subs
            .iter()
            .find(|s| s.title.as_deref() == Some("Movie.en.srt"))
            .unwrap();
        assert_eq!(en.language.as_deref(), Some("en"));
        assert_eq!(en.codec, "subrip");
        let fr = subs
            .iter()
            .find(|s| s.title.as_deref() == Some("Movie.fr.vtt"))
            .unwrap();
        assert_eq!(fr.codec, "webvtt");
    }

    #[tokio::test]
    async fn probe_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let clip = dir.path().join("clip:odd name.mov");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=320x240:r=25:d=2",
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                "sine=d=2",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-c:a",
                "aac",
                "-shortest",
            ])
            .args([
                "-movflags",
                "use_metadata_tags",
                "-metadata",
                "com.apple.quicktime.make=Apple",
            ])
            .args([
                "-metadata",
                "com.apple.quicktime.location.ISO6709=+37.7749-122.4194/",
                "-f",
                "mov",
            ])
            .arg(&clip)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(
            dir.path().join("clip:odd name.en.srt"),
            "1\n00:00:00,000 --> 00:00:01,000\nHi\n",
        )
        .unwrap();
        let m = probe(&clip).await.unwrap();
        let v = m.video.as_ref().unwrap();
        assert_eq!((v.codec.as_str(), v.width, v.height), ("h264", 320, 240));
        assert_eq!(v.fps, Some(25.0));
        assert!((m.duration_s.unwrap() - 2.0).abs() < 0.2);
        assert_eq!(m.audio[0].codec, "aac");
        assert_eq!(m.tags.get("make").map(String::as_str), Some("Apple"));
        assert!(m.tags.keys().all(|k| !k.contains("location")));
        assert_eq!(m.subtitles.len(), 1);
        assert_eq!(m.subtitles[0].index, 2);
        assert!(m.subtitles[0].external);

        let missing = probe(&dir.path().join("nope.mp4")).await.unwrap_err();
        assert!(
            missing
                .to_string()
                .starts_with("This file could not be read as media"),
            "{missing}"
        );
        let junk = dir.path().join("junk.mp4");
        std::fs::write(&junk, b"definitely not a video").unwrap();
        assert!(probe(&junk).await.is_err());
    }
}
