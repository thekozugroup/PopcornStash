//! The privileged half of PopcornStash and the torrent engine
//! (docs/architecture.md sections 1, 5 and 9).
//!
//! `popcornstash run` (root) is the supervisor: it applies policy routing and an
//! nftables kill switch for the engine user, brings up the VPN tunnel, runs a DNS
//! forwarder that only speaks through the tunnel, serves the adopt socket, and
//! spawns the web server and the engine with dropped privileges. The engine runs
//! librqbit with every socket bound to the tunnel interface and serves the
//! internal API and CONNECT proxy on loopback.

pub mod adopt;
pub mod api;
pub mod children;
pub mod engine;
pub mod proxy;
pub mod status;
pub mod supervisor;
pub mod sys;
pub mod vpn;

use std::net::SocketAddr;
use std::path::PathBuf;

/// Options for the root supervisor (`popcornstash run`).
#[derive(Debug, Clone)]
pub struct SupervisorOptions {
    /// `STASH_CONFIG` (appdata). VPN files live in `<config>/torrent/vpn`, engine state in `<config>/torrent/engine`.
    pub config_dir: PathBuf,
    /// `STASH_ROOT`. The engine writes to `<root>/Downloads/In Progress`.
    pub stash_root: PathBuf,
    /// User and group for the web server and every file PopcornStash creates.
    pub puid: u32,
    pub pgid: u32,
    /// User the engine runs as (`STASH_TORRENT_UID`). Distinct from `puid` and 0.
    pub engine_uid: u32,
    /// Run the engine without a tunnel (router-level VPN). Off by default.
    pub allow_no_vpn: bool,
    /// Binary to spawn for `serve` and `torrent-engine` (normally the current executable).
    pub exe: PathBuf,
    /// Runtime directory (`/run/popcornstash`).
    pub run_dir: PathBuf,
}

/// Options for the unprivileged engine (`popcornstash torrent-engine`).
#[derive(Debug, Clone)]
pub struct EngineOptions {
    /// `<config>/torrent/engine`: librqbit persistence.
    pub state_dir: PathBuf,
    /// `<root>/Downloads/In Progress`: where torrents are written.
    pub downloads_dir: PathBuf,
    /// `/run/popcornstash/status.json` written by the supervisor.
    pub status_file: PathBuf,
    /// Interface every torrent socket binds to. None only when `allow_no_vpn`.
    pub tunnel_if: Option<String>,
    pub allow_no_vpn: bool,
    /// Bearer token for the internal API (from `STASH_INTERNAL_TOKEN`).
    pub internal_token: String,
    pub api_bind: SocketAddr,
    pub proxy_bind: SocketAddr,
}

/// Run the supervisor until SIGTERM. Falls back to running only the web server
/// (torrents disabled) when it lacks root, NET_ADMIN or /dev/net/tun.
pub async fn run_supervisor(options: SupervisorOptions) -> anyhow::Result<()> {
    supervisor::run(options).await
}

/// Run the engine. Returns the process exit code (`EXIT_RELOAD` asks the supervisor to re-apply the VPN).
pub async fn run_engine(options: EngineOptions) -> anyhow::Result<i32> {
    engine::run(options).await
}
