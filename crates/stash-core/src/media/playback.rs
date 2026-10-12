//! Direct play vs HLS decision.

use serde::{Deserialize, Serialize};

use crate::models::MediaSummary;

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

/// Decide how to play a file with extension `ext` (lowercase, no dot).
pub fn decide(media: &MediaSummary, ext: &str, caps: &ClientCaps) -> PlaybackMode {
    let _ = (media, ext, caps);
    todo!("media agent: rules from section 5")
}
