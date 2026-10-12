//! ffprobe wrapper.

use std::path::Path;

use crate::models::MediaSummary;
use crate::Result;

/// `ffprobe -v error -print_format json -show_format -show_streams <path>` → summary.
pub async fn probe(path: &Path) -> Result<MediaSummary> {
    let _ = path;
    todo!("media agent")
}

/// Parse ffprobe JSON output (separated for unit tests).
pub fn parse_ffprobe_json(json: &str) -> Result<MediaSummary> {
    let _ = json;
    todo!("media agent: codecs, hdr detection (smpte2084 → hdr10, arib-std-b67 → hlg, dovi side data), rotation from side_data/tags")
}
