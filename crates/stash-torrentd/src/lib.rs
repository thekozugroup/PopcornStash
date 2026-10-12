//! The torrent side of PopcornStash, running in its own VPN container
//! (docs/architecture.md sections 5 and 9).
//!
//! `torrentd` (root) applies the VPN tunnel, routes, DNS and an nftables kill
//! switch, then spawns `torrent-engine` with dropped privileges. The engine runs
//! librqbit with every socket bound to the tunnel interface and serves the
//! internal API and CONNECT proxy.

pub mod api;
pub mod engine;
pub mod proxy;
pub mod status;
pub mod supervisor;
pub mod vpn;

use std::net::SocketAddr;
use std::path::PathBuf;

/// Options for the root supervisor (`popcornstash torrentd`).
#[derive(Debug, Clone)]
pub struct SupervisorOptions {
    /// Shared torrent dir (`/data`): internal token, `vpn/`, `engine/`, `status.json`.
    pub data_dir: PathBuf,
    /// `/stash/Downloads` (only Downloads is mounted in this container).
    pub downloads_dir: PathBuf,
    /// Owner of files written by the engine.
    pub puid: u32,
    pub pgid: u32,
    /// Run the engine without a tunnel (router-level VPN). Off by default.
    pub allow_no_vpn: bool,
    /// Binary to spawn for the engine (normally the current executable).
    pub engine_exe: PathBuf,
}

/// Options for the unprivileged engine (`popcornstash torrent-engine`).
#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub data_dir: PathBuf,
    pub downloads_dir: PathBuf,
    /// Interface every torrent socket binds to. None only when `allow_no_vpn`.
    pub tunnel_if: Option<String>,
    pub allow_no_vpn: bool,
    pub api_bind: SocketAddr,
    pub proxy_bind: SocketAddr,
}

/// Run the supervisor until SIGTERM. Requires root and NET_ADMIN.
pub async fn run_supervisor(options: SupervisorOptions) -> anyhow::Result<()> {
    supervisor::run(options).await
}

/// Run the engine. Returns the process exit code (`EXIT_RELOAD` asks the supervisor to re-apply the VPN).
pub async fn run_engine(options: EngineOptions) -> anyhow::Result<i32> {
    engine::run(options).await
}
