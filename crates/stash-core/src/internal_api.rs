//! Types shared by the main server and the torrent engine's internal API
//! (docs/architecture.md section 9), plus files in the shared `torrent/` dir.

use serde::{Deserialize, Serialize};

/// File in the shared torrent dir holding the internal bearer token.
pub const INTERNAL_TOKEN_FILE: &str = "internal.token";
/// Supervisor status file in the shared torrent dir.
pub const STATUS_FILE: &str = "status.json";
/// VPN directory inside the shared torrent dir.
pub const VPN_DIR: &str = "vpn";
pub const VPN_META_FILE: &str = "vpn.json";
pub const WIREGUARD_FILE: &str = "wireguard.conf";
pub const OPENVPN_FILE: &str = "openvpn.ovpn";
pub const OPENVPN_AUTH_FILE: &str = "openvpn.auth";
/// Engine persistence directory inside the shared torrent dir.
pub const ENGINE_DIR: &str = "engine";
/// Name of the tunnel interface inside the VPN container.
pub const TUNNEL_IF: &str = "psw0";
/// Exit code the engine uses to ask the supervisor to re-apply the VPN.
pub const EXIT_RELOAD: i32 = 75;
pub const ENGINE_PORT: u16 = 9091;
pub const PROXY_PORT: u16 = 9092;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VpnProvider {
    Nordvpn,
    Wireguard,
    Openvpn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VpnProtocol {
    Wireguard,
    Openvpn,
}

/// Contents of `vpn/vpn.json`, written by the main server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VpnMeta {
    pub provider: VpnProvider,
    pub protocol: VpnProtocol,
    /// Friendly name, e.g. "NordVPN Netherlands #812".
    pub label: Option<String>,
    pub server: Option<String>,
    pub country: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelState {
    Connected,
    Connecting,
    Disconnected,
    Error,
    NotConfigured,
}

/// Written by the supervisor to `status.json` every few seconds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TunnelStatus {
    pub state: TunnelState,
    pub provider: Option<VpnProvider>,
    pub protocol: Option<VpnProtocol>,
    pub label: Option<String>,
    pub server: Option<String>,
    pub country: Option<String>,
    pub interface: Option<String>,
    pub since: Option<i64>,
    pub error: Option<String>,
    pub kill_switch: bool,
    pub updated_at: i64,
}

impl TunnelStatus {
    pub fn not_configured(now: i64) -> Self {
        Self {
            state: TunnelState::NotConfigured,
            provider: None,
            protocol: None,
            label: None,
            server: None,
            country: None,
            interface: None,
            since: None,
            error: None,
            kill_switch: true,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineRunState {
    /// Accepting torrents.
    Running,
    /// Refusing torrents because the tunnel is not up.
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EngineState {
    pub state: EngineRunState,
    pub reason: Option<String>,
    pub allow_no_vpn: bool,
}

/// `GET /health` on the engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EngineHealth {
    pub vpn: TunnelStatus,
    pub engine: EngineState,
    pub public_ip: Option<String>,
    pub country: Option<String>,
    pub protected: Option<bool>,
    pub checked_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineTorrentState {
    Initializing,
    Downloading,
    Paused,
    Seeding,
    Error,
}

/// One entry of `GET /torrents`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EngineTorrent {
    pub id: String,
    pub info_hash: String,
    pub name: Option<String>,
    pub state: EngineTorrentState,
    pub progress: f64,
    pub total_bytes: Option<u64>,
    pub done_bytes: u64,
    pub uploaded_bytes: u64,
    pub down_bps: u64,
    pub up_bps: u64,
    pub peers: Option<u32>,
    pub eta_s: Option<u64>,
    pub error: Option<String>,
    /// Absolute path inside the containers, under `/stash/Downloads/In Progress`.
    pub output_dir: Option<String>,
}

/// `POST /torrents`. Exactly one field is set.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AddTorrentRequest {
    pub magnet: Option<String>,
    pub torrent_base64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddTorrentResponse {
    pub id: String,
    pub info_hash: String,
    pub name: Option<String>,
}

/// `PUT /limits`. Zero means unlimited.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct LimitsRequest {
    pub down_kbps: u64,
    pub up_kbps: u64,
}
