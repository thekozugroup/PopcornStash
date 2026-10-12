//! Direct play vs HLS decision (section 5, "Playback decision").

use serde::{Deserialize, Serialize};

use crate::models::{AudioStream, MediaSummary};

/// What the browser says it can play (`POST /api/items/:id/playback` body).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ClientCaps {
    /// e.g. ["h264", "hevc", "av1", "vp9"]
    pub video_codecs: Vec<String>,
    /// e.g. ["aac", "mp3", "opus", "flac", "ac3", "eac3"]
    pub audio_codecs: Vec<String>,
    /// The display and browser can show HDR.
    pub hdr: bool,
    pub max_height: Option<u32>,
    pub start_s: Option<f64>,
    pub audio_index: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum PlaybackMode {
    Direct,
    Hls { reason: String },
}

/// Containers browsers play natively (by extension).
const DIRECT_EXTENSIONS: [&str; 4] = ["mp4", "m4v", "mov", "webm"];
/// Audio-only files browsers play natively (by extension).
const DIRECT_AUDIO_EXTENSIONS: [&str; 8] = ["mp3", "m4a", "aac", "flac", "opus", "ogg", "oga", "wav"];
/// Audio codecs every supported browser decodes.
const BASELINE_AUDIO: [&str; 4] = ["aac", "mp3", "opus", "flac"];
/// 8-bit 4:2:0 pixel formats; anything else (10-bit, 4:2:2, 4:4:4) H.264 will not play in browsers.
const BROWSER_H264_PIX_FMTS: [&str; 3] = ["yuv420p", "yuvj420p", "nv12"];

/// Decide how to play a file with extension `ext` (lowercase, no dot).
///
/// Direct play needs every rule to pass: a browser container (mp4/m4v/mov/webm by extension
/// and by probe), a supported video codec (H.264 always; HEVC/AV1/VP9 only when the client
/// lists them), supported audio (AAC/MP3/Opus/FLAC, absent, or listed by the client), no HDR
/// for an SDR client, height within the client's limit, and the default audio track.
pub fn decide(media: &MediaSummary, ext: &str, caps: &ClientCaps) -> PlaybackMode {
    match first_blocker(media, ext, caps) {
        None => PlaybackMode::Direct,
        Some(reason) => PlaybackMode::Hls { reason },
    }
}

fn first_blocker(media: &MediaSummary, ext: &str, caps: &ClientCaps) -> Option<String> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    let audio_only = media.video.is_none();

    // Container: by extension, then by what ffprobe found inside.
    let ext_ok = if audio_only {
        DIRECT_AUDIO_EXTENSIONS.contains(&ext.as_str()) || DIRECT_EXTENSIONS.contains(&ext.as_str())
    } else {
        DIRECT_EXTENSIONS.contains(&ext.as_str())
    };
    if !ext_ok {
        return Some(format!("{} files cannot play directly in a browser", display_ext(&ext)));
    }
    if let Some(container) = media.container.as_deref() {
        if !container_ok(container, &ext, audio_only) {
            return Some(format!("This {} file holds a format browsers cannot open", display_ext(&ext)));
        }
    }

    if let Some(video) = &media.video {
        let codec = normalize_codec(&video.codec);
        let supported = codec == "h264" || caps.video_codecs.iter().any(|c| normalize_codec(c) == codec);
        if !supported {
            return Some(format!("This browser cannot play {} video", codec_label(&codec)));
        }
        if codec == "h264" {
            if let Some(pix_fmt) = video.pix_fmt.as_deref() {
                if !BROWSER_H264_PIX_FMTS.contains(&pix_fmt) {
                    let what = if pix_fmt.contains("10") || pix_fmt.contains("12") { "10-bit" } else { "high-colour" };
                    return Some(format!("Browsers cannot play {what} H.264 video"));
                }
            }
        }
    }

    let selected = selected_audio(media, caps.audio_index);
    if let Some(audio) = selected {
        let codec = normalize_codec(&audio.codec);
        let supported = BASELINE_AUDIO.contains(&codec.as_str())
            || caps.audio_codecs.iter().any(|c| normalize_codec(c) == codec);
        if !supported {
            return Some(format!("This browser cannot play {} audio", codec_label(&codec)));
        }
    }
    if let (Some(audio), Some(default)) = (selected, media.audio.first()) {
        if audio.index != default.index {
            return Some("Switching to another audio track needs conversion".to_string());
        }
    }

    if let Some(video) = &media.video {
        if video.hdr.is_some() && !caps.hdr {
            return Some("This screen cannot show HDR, so the colours are converted".to_string());
        }
        if let Some(max) = caps.max_height.filter(|m| *m > 0) {
            let resolution = video.width.min(video.height);
            if resolution > i64::from(max) {
                return Some(format!("Lowered to {max}p for this device"));
            }
        }
    }
    None
}

/// The audio stream the player will use: the requested one when it exists, else the first.
pub fn selected_audio(media: &MediaSummary, audio_index: Option<i64>) -> Option<&AudioStream> {
    audio_index
        .and_then(|i| media.audio.iter().find(|a| a.index == i))
        .or_else(|| media.audio.first())
}

fn container_ok(container: &str, ext: &str, audio_only: bool) -> bool {
    let names: Vec<&str> = container.split(',').map(str::trim).collect();
    let has = |n: &str| names.contains(&n);
    match ext {
        "mp4" | "m4v" | "mov" | "m4a" => has("mp4") || has("mov"),
        "webm" => has("webm"),
        _ if audio_only => match ext {
            "mp3" => has("mp3"),
            "aac" => has("aac"),
            "flac" => has("flac"),
            "ogg" | "oga" | "opus" => has("ogg"),
            "wav" => has("wav"),
            _ => false,
        },
        _ => false,
    }
}

/// Lowercase codec name with common aliases folded (`h265` → `hevc`, `avc1` → `h264`).
pub fn normalize_codec(codec: &str) -> String {
    let c = codec.trim().to_ascii_lowercase();
    match c.as_str() {
        "h265" | "hev1" | "hvc1" | "x265" => "hevc".to_string(),
        "avc" | "avc1" | "x264" => "h264".to_string(),
        "av01" => "av1".to_string(),
        "vp09" => "vp9".to_string(),
        "ac-3" => "ac3".to_string(),
        "e-ac-3" | "ec-3" => "eac3".to_string(),
        "mp4a" => "aac".to_string(),
        _ => c,
    }
}

/// Human name for a codec in reasons shown to people.
pub fn codec_label(codec: &str) -> String {
    match codec {
        "hevc" => "HEVC".into(),
        "h264" => "H.264".into(),
        "av1" => "AV1".into(),
        "vp9" => "VP9".into(),
        "vp8" => "VP8".into(),
        "mpeg4" => "MPEG-4".into(),
        "mpeg2video" => "MPEG-2".into(),
        "mpeg1video" => "MPEG-1".into(),
        "msmpeg4v3" | "msmpeg4v2" => "DivX".into(),
        "wmv3" | "wmv2" | "wmv1" | "vc1" => "Windows Media".into(),
        "prores" => "ProRes".into(),
        "theora" => "Theora".into(),
        "ac3" => "Dolby Digital (AC-3)".into(),
        "eac3" => "Dolby Digital Plus (E-AC-3)".into(),
        "truehd" => "Dolby TrueHD".into(),
        "dts" => "DTS".into(),
        "vorbis" => "Vorbis".into(),
        "alac" => "Apple Lossless".into(),
        "wmav2" | "wmav1" | "wmapro" => "Windows Media".into(),
        other if other.starts_with("pcm_") => "PCM".into(),
        other => other.to_ascii_uppercase(),
    }
}

fn display_ext(ext: &str) -> String {
    if ext.is_empty() { "These".to_string() } else { ext.to_ascii_uppercase() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::VideoStream;

    fn video(codec: &str, width: i64, height: i64) -> VideoStream {
        VideoStream {
            index: 0,
            codec: codec.into(),
            pix_fmt: Some("yuv420p".into()),
            width,
            height,
            ..VideoStream::default()
        }
    }

    fn audio(index: i64, codec: &str) -> AudioStream {
        AudioStream { index, codec: codec.into(), channels: Some(2), ..AudioStream::default() }
    }

    fn mp4(v: VideoStream, a: Vec<AudioStream>) -> MediaSummary {
        MediaSummary {
            container: Some("mov,mp4,m4a,3gp,3g2,mj2".into()),
            duration_s: Some(60.0),
            video: Some(v),
            audio: a,
            ..MediaSummary::default()
        }
    }

    fn caps(video: &[&str], audio: &[&str]) -> ClientCaps {
        ClientCaps {
            video_codecs: video.iter().map(|s| s.to_string()).collect(),
            audio_codecs: audio.iter().map(|s| s.to_string()).collect(),
            ..ClientCaps::default()
        }
    }

    fn hls(reason: &str) -> PlaybackMode {
        PlaybackMode::Hls { reason: reason.to_string() }
    }

    #[test]
    fn decision_table() {
        let h264 = mp4(video("h264", 1920, 1080), vec![audio(1, "aac")]);
        let hevc = mp4(video("hevc", 3840, 2160), vec![audio(1, "aac")]);
        let mut hdr = hevc.clone();
        hdr.video.as_mut().unwrap().hdr = Some("hlg".into());
        let mut mkv = h264.clone();
        mkv.container = Some("matroska,webm".into());
        let mut webm = mp4(video("vp9", 1280, 720), vec![audio(1, "opus")]);
        webm.container = Some("matroska,webm".into());
        let ac3 = mp4(video("h264", 1920, 1080), vec![audio(1, "ac3")]);
        let silent = mp4(video("h264", 640, 360), vec![]);
        let two_tracks = mp4(video("h264", 1920, 1080), vec![audio(1, "aac"), audio(2, "aac")]);
        let mut hi10 = h264.clone();
        hi10.video.as_mut().unwrap().pix_fmt = Some("yuv420p10le".into());
        let mut avi = h264.clone();
        avi.container = Some("avi".into());
        let mpeg4 = mp4(video("mpeg4", 720, 480), vec![audio(1, "mp3")]);

        let none = ClientCaps::default();
        let chrome = caps(&["h264", "vp9", "av1"], &["aac", "mp3", "opus", "flac"]);
        let safari = ClientCaps { hdr: true, ..caps(&["h264", "hevc"], &["aac", "mp3", "ac3", "eac3", "flac"]) };
        let hevc_sdr = caps(&["h264", "hevc"], &[]);

        let table: Vec<(&str, &MediaSummary, &str, &ClientCaps, PlaybackMode)> = vec![
            ("h264 mp4 plays anywhere", &h264, "mp4", &none, PlaybackMode::Direct),
            ("m4v and mov are fine", &h264, "m4v", &none, PlaybackMode::Direct),
            ("mov", &h264, "mov", &chrome, PlaybackMode::Direct),
            ("extension case is ignored", &h264, "MP4", &none, PlaybackMode::Direct),
            ("mkv never direct", &mkv, "mkv", &chrome, hls("MKV files cannot play directly in a browser")),
            ("avi never direct", &avi, "avi", &chrome, hls("AVI files cannot play directly in a browser")),
            ("mp4 name but avi inside", &avi, "mp4", &chrome, hls("This MP4 file holds a format browsers cannot open")),
            ("hevc without support", &hevc, "mp4", &chrome, hls("This browser cannot play HEVC video")),
            ("hevc with support", &hevc, "mp4", &hevc_sdr, PlaybackMode::Direct),
            ("hevc alias h265", &hevc, "mov", &caps(&["h265"], &[]), PlaybackMode::Direct),
            ("hdr on sdr client", &hdr, "mov", &hevc_sdr, hls("This screen cannot show HDR, so the colours are converted")),
            ("hdr on hdr client", &hdr, "mov", &safari, PlaybackMode::Direct),
            ("webm vp9 with support", &webm, "webm", &chrome, PlaybackMode::Direct),
            ("webm vp9 without support", &webm, "webm", &none, hls("This browser cannot play VP9 video")),
            ("ac3 without support", &ac3, "mp4", &chrome, hls("This browser cannot play Dolby Digital (AC-3) audio")),
            ("ac3 with support", &ac3, "mp4", &safari, PlaybackMode::Direct),
            ("no audio is fine", &silent, "mp4", &none, PlaybackMode::Direct),
            ("10-bit h264", &hi10, "mp4", &chrome, hls("Browsers cannot play 10-bit H.264 video")),
            ("old mpeg4 part 2", &mpeg4, "mp4", &chrome, hls("This browser cannot play MPEG-4 video")),
            ("default track", &two_tracks, "mp4", &ClientCaps { audio_index: Some(1), ..chrome.clone() }, PlaybackMode::Direct),
            (
                "second track needs hls",
                &two_tracks,
                "mp4",
                &ClientCaps { audio_index: Some(2), ..chrome.clone() },
                hls("Switching to another audio track needs conversion"),
            ),
            ("unknown track falls back to default", &two_tracks, "mp4", &ClientCaps { audio_index: Some(9), ..chrome.clone() }, PlaybackMode::Direct),
            ("height within limit", &h264, "mp4", &ClientCaps { max_height: Some(1080), ..chrome.clone() }, PlaybackMode::Direct),
            ("height over limit", &h264, "mp4", &ClientCaps { max_height: Some(720), ..chrome.clone() }, hls("Lowered to 720p for this device")),
        ];
        for (name, media, ext, caps, expected) in table {
            assert_eq!(decide(media, ext, caps), expected, "{name}");
        }
    }

    #[test]
    fn portrait_phone_video_uses_short_edge() {
        // 1920x1080 coded, rotated to portrait: still a 1080p video.
        let mut phone = mp4(video("h264", 1920, 1080), vec![audio(1, "aac")]);
        phone.video.as_mut().unwrap().rotation = 90;
        let c = ClientCaps { max_height: Some(1080), ..ClientCaps::default() };
        assert_eq!(decide(&phone, "mov", &c), PlaybackMode::Direct);
        let c = ClientCaps { max_height: Some(480), ..ClientCaps::default() };
        assert_eq!(decide(&phone, "mov", &c), hls("Lowered to 480p for this device"));
    }

    #[test]
    fn audio_only_files() {
        let mp3 = MediaSummary {
            container: Some("mp3".into()),
            audio: vec![audio(0, "mp3")],
            ..MediaSummary::default()
        };
        assert_eq!(decide(&mp3, "mp3", &ClientCaps::default()), PlaybackMode::Direct);
        let m4a = MediaSummary {
            container: Some("mov,mp4,m4a,3gp,3g2,mj2".into()),
            audio: vec![audio(0, "alac")],
            ..MediaSummary::default()
        };
        assert_eq!(decide(&m4a, "m4a", &ClientCaps::default()), hls("This browser cannot play Apple Lossless audio"));
        let wma = MediaSummary { container: Some("asf".into()), audio: vec![audio(0, "wmav2")], ..MediaSummary::default() };
        assert_eq!(decide(&wma, "wma", &ClientCaps::default()), hls("WMA files cannot play directly in a browser"));
    }

    #[test]
    fn missing_probe_relies_on_extension() {
        let unknown = MediaSummary::default();
        assert_eq!(decide(&unknown, "mp4", &ClientCaps::default()), PlaybackMode::Direct);
        assert_eq!(decide(&unknown, "", &ClientCaps::default()), hls("These files cannot play directly in a browser"));
    }

    #[test]
    fn serializes_with_mode_tag() {
        assert_eq!(serde_json::to_value(PlaybackMode::Direct).unwrap(), serde_json::json!({"mode": "direct"}));
        assert_eq!(
            serde_json::to_value(hls("x")).unwrap(),
            serde_json::json!({"mode": "hls", "reason": "x"})
        );
        let parsed: ClientCaps = serde_json::from_str(r#"{"video_codecs": ["h264"], "hdr": true}"#).unwrap();
        assert!(parsed.hdr);
        assert!(parsed.audio_codecs.is_empty());
    }
}
