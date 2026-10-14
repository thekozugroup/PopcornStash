//! Shared application state handed to every handler.

use std::sync::Arc;

use stash_core::config::Config;
use stash_core::db::Db;
use stash_core::events::EventBus;
use stash_core::jobs::JobQueue;
use stash_core::library::Library;
use stash_core::media::MediaService;
use stash_core::media::transcode::TranscodeManager;
use stash_core::paths::StashLayout;
use stash_core::settings::SettingsStore;

use crate::auth::AuthService;
use crate::downloads::DownloadManager;
use crate::live::LiveBus;
use crate::security::RateLimiter;
use crate::torrentd_client::TorrentdClient;
use crate::uploads::UploadService;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Db,
    pub layout: Arc<StashLayout>,
    pub events: EventBus,
    /// The events with what decides who may see them, for the event streams.
    pub live: LiveBus,
    pub jobs: JobQueue,
    pub settings: SettingsStore,
    pub media: Arc<MediaService>,
    pub library: Arc<Library>,
    pub transcoder: Arc<TranscodeManager>,
    pub auth: Arc<AuthService>,
    pub downloads: Arc<DownloadManager>,
    pub uploads: Arc<UploadService>,
    pub torrentd: Arc<TorrentdClient>,
    pub rate_limiter: Arc<RateLimiter>,
    /// Outbound client for NordVPN API and similar (not for user downloads).
    pub http: reqwest::Client,
}
