//! Client for the torrent engine's internal API in the VPN container (section 9).

use std::sync::Arc;

use stash_core::config::Config;
use stash_core::internal_api::{AddTorrentRequest, AddTorrentResponse, EngineHealth, EngineTorrent, LimitsRequest};
use stash_core::events::EventBus;
use stash_core::models::VpnStatus;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum TorrentdError {
    /// `STASH_TORRENTD_URL` is empty.
    #[error("Torrents are turned off on this server.")]
    Disabled,
    #[error("The torrent container is not reachable: {0}")]
    Unreachable(String),
    /// The engine refuses torrents because the VPN is not connected.
    #[error("{0}")]
    Blocked(String),
    #[error("Torrent engine error ({status}): {message}")]
    Api { status: u16, message: String },
}

pub struct TorrentdClient {
    pub config: Arc<Config>,
}

impl TorrentdClient {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    pub fn enabled(&self) -> bool {
        self.config.torrentd_url.is_some()
    }

    /// Create `<config>/torrent/` and the internal token (0640) if missing; return the token.
    pub async fn ensure_internal_token(config: &Config) -> anyhow::Result<String> {
        let _ = config;
        todo!("vpn agent")
    }

    pub async fn health(&self) -> Result<EngineHealth, TorrentdError> {
        todo!("vpn agent")
    }

    pub async fn list(&self) -> Result<Vec<EngineTorrent>, TorrentdError> {
        todo!("vpn agent")
    }

    pub async fn add(&self, request: &AddTorrentRequest) -> Result<AddTorrentResponse, TorrentdError> {
        let _ = request;
        todo!("vpn agent")
    }

    pub async fn pause(&self, id: &str) -> Result<(), TorrentdError> {
        let _ = id;
        todo!("vpn agent")
    }

    pub async fn resume(&self, id: &str) -> Result<(), TorrentdError> {
        let _ = id;
        todo!("vpn agent")
    }

    pub async fn remove(&self, id: &str, delete_files: bool) -> Result<(), TorrentdError> {
        let _ = (id, delete_files);
        todo!("vpn agent")
    }

    pub async fn set_limits(&self, limits: LimitsRequest) -> Result<(), TorrentdError> {
        let _ = limits;
        todo!("vpn agent")
    }

    pub async fn reload_vpn(&self) -> Result<(), TorrentdError> {
        todo!("vpn agent")
    }

    /// UI status: combines `vpn/vpn.json` with the engine's health.
    pub async fn vpn_status(&self) -> VpnStatus {
        todo!("vpn agent")
    }

    /// Poll engine health and publish `Event::Vpn` whenever the UI-visible status changes.
    pub fn spawn_status_watch(self: &Arc<Self>, events: EventBus, cancel: CancellationToken) {
        let _ = (events, cancel);
        todo!("vpn agent")
    }

    /// reqwest proxy for web downloads through the VPN (CONNECT proxy with basic auth), when configured.
    pub async fn download_proxy(&self) -> Option<reqwest::Proxy> {
        todo!("vpn agent")
    }
}
