//! File classification rules (section 4).

use std::path::Path;

use crate::media::exif::ExifInfo;
use crate::models::{Classification, MediaKind, MediaSummary};

/// Media kind from the file extension.
pub fn media_kind(path: &Path) -> MediaKind {
    let _ = path;
    todo!("library agent: extension table from section 4")
}

/// Classify a file. `rel_path` is relative to the stash root; `mtime` is unix seconds.
/// The returned `destination` is a relative path inside the library folders.
pub fn classify(rel_path: &str, probe: Option<&MediaSummary>, exif: Option<&ExifInfo>, mtime: i64) -> Classification {
    let _ = (rel_path, probe, exif, mtime);
    todo!("library agent: rules from section 4, first match wins")
}
