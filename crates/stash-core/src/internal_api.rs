//! Types and constants shared by the supervisor (`popcornstash run`), the web
//! server and the torrent engine (docs/architecture.md sections 3, 5 and 9).

use serde::{Deserialize, Serialize};

/// Development fallback: token file in the torrent dir, used only when
/// `STASH_INTERNAL_TOKEN` is unset (serve and engine started by hand).
pub const INTERNAL_TOKEN_FILE: &str = "internal.token";
/// Runtime directory created by the supervisor (tmpfs).
pub const RUN_DIR: &str = "/run/popcornstash";
/// Tunnel status written by the supervisor, read by the engine (inside RUN_DIR).
pub const STATUS_FILE: &str = "status.json";
/// Adopt socket (inside RUN_DIR), owner PUID, 0600.
pub const SUPERVISOR_SOCKET: &str = "supervisor.sock";
/// Engine API and proxy addresses (loopback only).
pub const ENGINE_API_ADDR: &str = "127.0.0.1:9091";
pub const ENGINE_PROXY_ADDR: &str = "127.0.0.1:9092";
/// Supervisor DNS forwarder that only resolves through the tunnel.
pub const DNS_FORWARDER_ADDR: &str = "127.0.0.54:5354";
/// Routing table for the engine user and the forwarder's marked sockets.
pub const ROUTE_TABLE: u32 = 51820;
/// Socket mark used by the supervisor's DNS forwarder upstream sockets.
pub const FWMARK: u32 = 0x5150;
/// nftables table name (family inet).
pub const NFT_TABLE: &str = "popcornstash";
/// VPN directory inside the shared torrent dir.
pub const VPN_DIR: &str = "vpn";
pub const VPN_META_FILE: &str = "vpn.json";
pub const WIREGUARD_FILE: &str = "wireguard.conf";
pub const OPENVPN_FILE: &str = "openvpn.ovpn";
pub const OPENVPN_AUTH_FILE: &str = "openvpn.auth";
/// Engine persistence directory inside the torrent dir (owned by the engine user).
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

/// The lowest speed limit the engine can keep, in KiB/s. librqbit moves data in 16 KiB
/// blocks and waits for one second's allowance to cover a whole block, so a lower limit
/// would not slow transfers down: it would stop every one of them.
pub const MIN_SPEED_LIMIT_KBPS: u64 = 16;

/// `PUT /limits`. Zero means unlimited; other values below [`MIN_SPEED_LIMIT_KBPS`] count
/// as that minimum.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct LimitsRequest {
    pub down_kbps: u64,
    pub up_kbps: u64,
}

/// One request line on the supervisor's adopt socket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SupervisorRequest {
    /// Re-own `Downloads/In Progress/<name>` (a single path component) as PUID:PGID.
    Adopt { name: String },
}

/// One response line on the supervisor's adopt socket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupervisorResponse {
    pub ok: bool,
    pub error: Option<String>,
}
