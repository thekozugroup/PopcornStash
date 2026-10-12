//! EXIF reading for photos.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExifInfo {
    /// DateTimeOriginal (or DateTime) as unix seconds, interpreted with OffsetTimeOriginal when present, else UTC.
    pub taken_at: Option<i64>,
    /// "Make Model", e.g. "Apple iPhone 15 Pro".
    pub camera: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    /// EXIF orientation 1–8.
    pub orientation: Option<u16>,
}

/// Blocking EXIF read (JPEG, HEIC, TIFF, PNG, WebP, DNG). Ok(None) when absent.
pub fn read(path: &Path) -> Result<Option<ExifInfo>> {
    let _ = path;
    todo!("media agent: kamadak-exif")
}
