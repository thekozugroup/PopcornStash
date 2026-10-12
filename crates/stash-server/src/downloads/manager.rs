use std::sync::Arc;

use serde::{Deserialize, Serialize};
use stash_core::config::Config;
use stash_core::db::Db;
use stash_core::events::EventBus;
use stash_core::jobs::JobQueue;
use stash_core::library::Library;
use stash_core::models::Download;
use stash_core::paths::StashLayout;
use stash_core::settings::SettingsStore;
use tokio_util::sync::CancellationToken;

use crate::error::ApiResult;
use crate::torrentd_client::TorrentdClient;

/// Headers the Chrome extension may forward. Kept in memory only.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ForwardedHeaders {
    pub cookie: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
}

/// `POST /api/downloads`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddDownloadRequest {
    pub url: String,
    pub filename: Option<String>,
    pub headers: Option<ForwardedHeaders>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadFilter {
    #[default]
    All,
    Active,
    Finished,
}

pub struct DownloadManager {
    pub config: Arc<Config>,
    pub db: Db,
    pub layout: Arc<StashLayout>,
    pub events: EventBus,
    pub jobs: JobQueue,
    pub settings: SettingsStore,
    pub library: Arc<Library>,
    pub torrentd: Arc<TorrentdClient>,
}

impl DownloadManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<Config>,
        db: Db,
        layout: Arc<StashLayout>,
        events: EventBus,
        jobs: JobQueue,
        settings: SettingsStore,
        library: Arc<Library>,
        torrentd: Arc<TorrentdClient>,
    ) -> Self {
        Self { config, db, layout, events, jobs, settings, library, torrentd }
    }

    /// Recover state after restart and spawn the web-download workers and the torrent poller.
    pub fn start(self: &Arc<Self>, cancel: CancellationToken) {
        let _ = cancel;
        todo!("downloads agent")
    }

    /// Add an http(s) link or magnet link.
    pub async fn add_url(&self, user_id: &str, request: AddDownloadRequest) -> ApiResult<Download> {
        let _ = (user_id, request);
        todo!("downloads agent")
    }

    /// Add a .torrent file.
    pub async fn add_torrent_file(&self, user_id: &str, bytes: Vec<u8>, filename: Option<String>) -> ApiResult<Download> {
        let _ = (user_id, bytes, filename);
        todo!("downloads agent")
    }

    /// A finished browser upload that now sits at `rel_path` (under In Progress); moves it to Completed and sorts.
    pub async fn upload_finished(&self, user_id: &str, filename: &str, rel_path: &str, size: i64) -> ApiResult<Download> {
        let _ = (user_id, filename, rel_path, size);
        todo!("downloads agent")
    }

    pub async fn list(&self, filter: DownloadFilter, limit: u32) -> ApiResult<Vec<Download>> {
        let _ = (filter, limit);
        todo!("downloads agent")
    }

    pub async fn get(&self, id: &str) -> ApiResult<Download> {
        let _ = id;
        todo!("downloads agent")
    }

    pub async fn pause(&self, id: &str) -> ApiResult<Download> {
        let _ = id;
        todo!("downloads agent")
    }

    pub async fn resume(&self, id: &str) -> ApiResult<Download> {
        let _ = id;
        todo!("downloads agent")
    }

    pub async fn remove(&self, id: &str, delete_files: bool) -> ApiResult<()> {
        let _ = (id, delete_files);
        todo!("downloads agent")
    }
}
