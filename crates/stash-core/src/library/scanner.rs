//! Full rescans and single-file indexing (section 5, Watcher).

use super::{Library, ScanReport};
use crate::models::ItemRow;
use crate::Result;

pub async fn rescan(lib: &Library) -> Result<ScanReport> {
    let _ = lib;
    todo!("library agent: walk the stash, add/update/move/missing, queue Inbox+Completed for sort")
}

pub async fn index_path(lib: &Library, rel_path: &str) -> Result<Option<ItemRow>> {
    let _ = (lib, rel_path);
    todo!("library agent: index in place, category from zone or classify, enqueue probe+thumb")
}

/// xxh3-64 of size + first 1 MiB + last 1 MiB, hex.
pub fn quick_hash(path: &std::path::Path) -> std::io::Result<String> {
    let _ = path;
    todo!("library agent")
}
