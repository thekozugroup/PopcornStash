//! Release-name and camera-name parsing (section 4).

use serde::{Deserialize, Serialize};

/// What a file name says about itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParsedName {
    /// Cleaned title (movie title, or show name for episodes).
    pub title: String,
    pub year: Option<i64>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    /// Last episode for multi-episode files (S01E02E03).
    pub episode_end: Option<i64>,
    pub episode_title: Option<String>,
    /// Release tags were present (1080p, BluRay, x264, WEB-DL, group…).
    pub has_release_tags: bool,
    /// The name looks like a phone/camera file (IMG_1234, VID_2024…, PXL_, GOPR, DJI_…).
    pub camera_style: bool,
    /// Date embedded in the name (unix seconds, UTC midnight or exact time when present).
    pub date: Option<i64>,
}

/// Parse a file stem (no extension) or folder name.
pub fn parse_name(stem: &str) -> ParsedName {
    let _ = stem;
    todo!("library agent: implement release/camera name parsing with tests")
}
