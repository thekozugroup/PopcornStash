//! Media processing: ffprobe, EXIF, thumbnails, photo renditions, playback
//! decisions and live HLS transcoding (section 5, Media).

pub mod exif;
pub mod filters;
pub mod playback;
pub mod probe;
pub mod thumbs;
pub mod tools;
pub mod transcode;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::config::Config;
use crate::db::Db;
use crate::events::EventBus;
use crate::models::{ItemRow, MediaSummary};
use crate::paths::StashLayout;
use crate::settings::SettingsStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThumbSize {
    /// 400px long edge, grids.
    Sm,
    /// 1280px long edge, detail views.
    Lg,
}

impl ThumbSize {
    pub fn long_edge(self) -> u32 {
        match self {
            ThumbSize::Sm => 400,
            ThumbSize::Lg => 1280,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            ThumbSize::Sm => "sm",
            ThumbSize::Lg => "lg",
        }
    }
}

pub struct MediaService {
    pub config: Arc<Config>,
    pub db: Db,
    pub settings: SettingsStore,
    pub layout: Arc<StashLayout>,
    pub events: EventBus,
    /// Shared with the library move lock: file-version checks and cache publication
    /// must be atomic with replacement invalidation, without locking conversions.
    pub(crate) publication: Arc<tokio::sync::Mutex<()>>,
}

impl MediaService {
    pub fn new(
        config: Arc<Config>,
        db: Db,
        settings: SettingsStore,
        layout: Arc<StashLayout>,
        events: EventBus,
    ) -> Self {
        Self {
            config,
            db,
            settings,
            layout,
            events,
            publication: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Run ffprobe on a file.
    pub async fn probe_file(&self, abs: &Path) -> Result<MediaSummary> {
        probe::probe(abs).await
    }

    /// Read EXIF from a photo. None when the file has no EXIF.
    pub async fn read_exif(&self, abs: &Path) -> Result<Option<exif::ExifInfo>> {
        let abs = abs.to_path_buf();
        tokio::task::spawn_blocking(move || exif::read(&abs))
            .await
            .map_err(|e| crate::CoreError::Other(e.into()))?
    }

    /// Probe job handler: fill duration, dimensions, media_json, the capture time, camera, mime.
    pub async fn process_item(&self, item_id: &str) -> Result<()> {
        thumbs::process_item(self, item_id, thumbs::Dating::Keep).await
    }

    /// The probe jobs migration 0003 queues (payload `{"redate": true}`): the same, with the
    /// capture time read again under section 4's rules instead of kept.
    pub async fn redate_item(&self, item_id: &str) -> Result<()> {
        thumbs::process_item(self, item_id, thumbs::Dating::Redate).await
    }

    /// Thumb job handler: write `sm` and `lg` WebP thumbnails (and a rendition for HEIC/RAW/TIFF photos).
    pub async fn thumbnail_item(&self, item_id: &str) -> Result<()> {
        thumbs::thumbnail_item(self, item_id).await
    }

    /// Where a thumbnail lives (whether or not it exists yet).
    pub fn thumb_path(&self, item_id: &str, size: ThumbSize) -> PathBuf {
        let shard = item_id
            .get(item_id.len().saturating_sub(2)..)
            .unwrap_or("00");
        self.config
            .thumbs_dir()
            .join(shard)
            .join(format!("{item_id}-{}.webp", size.as_str()))
    }

    /// Where a photo rendition lives.
    pub fn rendition_path(&self, item_id: &str) -> PathBuf {
        self.config.renditions_dir().join(format!("{item_id}.webp"))
    }

    /// File to serve for `GET /api/items/:id/view`: the original when browsers can show it,
    /// else the cached rendition (created on demand).
    pub async fn photo_view(&self, item: &ItemRow) -> Result<PathBuf> {
        thumbs::photo_view(self, item).await
    }
}
