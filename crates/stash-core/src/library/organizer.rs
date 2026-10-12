//! Moving files: automatic sorting, explicit organize, review, trash.

use super::{Library, OrganizeRequest, ReviewAction};
use crate::models::{Classification, ItemRow};
use crate::Result;

/// Relative destination path for a classification and original file name.
pub fn destination_for(classification: &Classification, file_name: &str) -> String {
    let _ = (classification, file_name);
    todo!("library agent: Movies/Title (Year)/Title (Year).ext, Shows/…, Home Videos/YYYY/MM/…, Photos/YYYY/MM/…, Other/…")
}

pub async fn sort_path(lib: &Library, rel_path: &str, download_id: Option<&str>) -> Result<Vec<String>> {
    let _ = (lib, rel_path, download_id);
    todo!("library agent")
}

pub async fn organize(lib: &Library, item_id: &str, request: OrganizeRequest) -> Result<ItemRow> {
    let _ = (lib, item_id, request);
    todo!("library agent")
}

pub async fn review(lib: &Library, item_id: &str, action: ReviewAction) -> Result<ItemRow> {
    let _ = (lib, item_id, action);
    todo!("library agent")
}

pub async fn trash_item(lib: &Library, item_id: &str, user_id: Option<&str>) -> Result<()> {
    let _ = (lib, item_id, user_id);
    todo!("library agent")
}

pub async fn restore_item(lib: &Library, item_id: &str) -> Result<ItemRow> {
    let _ = (lib, item_id);
    todo!("library agent")
}

pub async fn empty_trash(lib: &Library, older_than_days: Option<u32>) -> Result<u64> {
    let _ = (lib, older_than_days);
    todo!("library agent")
}
