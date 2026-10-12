//! Resumable chunked browser uploads (section 8, /api/uploads).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use stash_core::db::Db;
use stash_core::paths::StashLayout;
use tokio_util::sync::CancellationToken;

use crate::downloads::DownloadManager;

pub const CHUNK_SIZE: u64 = 16 * 1024 * 1024;
pub const MAX_CHUNK: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUpload {
    pub filename: String,
    pub size: i64,
    pub mime: Option<String>,
    pub last_modified: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadStatus {
    pub id: String,
    pub filename: String,
    pub size: i64,
    pub received: i64,
    pub state: String,
}

pub struct UploadService {
    pub db: Db,
    pub layout: Arc<StashLayout>,
    pub downloads: Arc<DownloadManager>,
}

impl UploadService {
    pub fn new(db: Db, layout: Arc<StashLayout>, downloads: Arc<DownloadManager>) -> Self {
        Self { db, layout, downloads }
    }

    /// Remove partial uploads untouched for 7 days.
    pub fn spawn_cleanup(self: &Arc<Self>, cancel: CancellationToken) {
        let _ = cancel;
        todo!("downloads agent")
    }
}
