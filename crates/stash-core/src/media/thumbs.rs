//! Probe/thumbnail job handlers and photo renditions.

use std::path::PathBuf;

use super::MediaService;
use crate::models::ItemRow;
use crate::Result;

pub async fn process_item(media: &MediaService, item_id: &str) -> Result<()> {
    let _ = (media, item_id);
    todo!("media agent: probe videos/audio, EXIF photos, update items row")
}

pub async fn thumbnail_item(media: &MediaService, item_id: &str) -> Result<()> {
    let _ = (media, item_id);
    todo!("media agent: ffmpeg frame at 10% for video, vipsthumbnail for photos, set thumb_state")
}

pub async fn photo_view(media: &MediaService, item: &ItemRow) -> Result<PathBuf> {
    let _ = (media, item);
    todo!("media agent")
}
