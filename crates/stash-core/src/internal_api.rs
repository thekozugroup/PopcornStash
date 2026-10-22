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
/// Privileged WireGuard transport sockets: encrypted outer UDP, never engine sockets.
pub const WIREGUARD_TRANSPORT_MARK: u32 = 0x5151;
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
    /// Refusing torrents because the tunnel is not up, or the public-address check said
    /// the address is not a VPN address ([`MSG_NOT_PROTECTED`]).
    Blocked,
}

/// The engine's blocked reason while the public-address check says the address is not
/// a VPN address (NordVPN `protected: false`). The web server reads it as
/// `protected: false` whatever else the engine reports.
pub const MSG_NOT_PROTECTED: &str = "The VPN is connected, but the public address is not a VPN address. Torrents stay stopped until a later check passes.";

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
    /// Fetch metadata and wait for an explicit file review before payload transfer.
    #[serde(default)]
    pub review_before_download: bool,
    pub magnet: Option<String>,
    pub torrent_base64: Option<String>,
    #[serde(default)]
    pub file_types: Option<Vec<TorrentFileKind>>,
    #[serde(default)]
    pub file_rules: Option<TorrentFileRules>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddTorrentResponse {
    pub id: String,
    pub info_hash: String,
    pub name: Option<String>,
}

/// Content groups for a torrent's files. Extension matching is case insensitive;
/// archives, documents and unknown extensions are Other (never executed).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TorrentFileKind {
    Video,
    Image,
    Subtitle,
    Other,
}

impl TorrentFileKind {
    pub fn of(path: &str) -> Self {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "mkv" | "mp4" | "m4v" | "mov" | "avi" | "webm" | "ts" | "m2ts" | "mpg" | "mpeg"
            | "wmv" | "flv" | "ogv" | "vob" | "mts" | "3gp" | "rm" | "rmvb" | "asf" => Self::Video,
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" | "avif" | "bmp" | "tif"
            | "tiff" | "dng" | "cr2" | "cr3" | "nef" | "arw" | "orf" | "rw2" | "raf" => Self::Image,
            "srt" | "ass" | "ssa" | "vtt" | "sub" | "idx" | "sup" => Self::Subtitle,
            _ => Self::Other,
        }
    }
    pub fn media_default() -> Vec<Self> {
        vec![Self::Video, Self::Subtitle]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TorrentFile {
    pub index: usize,
    /// Relative to this torrent's own output folder; never an absolute NAS path.
    pub path: String,
    pub size: u64,
    pub kind: TorrentFileKind,
    pub selected: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TorrentFiles {
    #[serde(default)]
    pub review_required: bool,
    pub ready: bool,
    pub files: Vec<TorrentFile>,
    #[serde(default)]
    pub rules: Option<TorrentFileRules>,
}

/// All specified conditions must match. Names match any listed term, ignoring
/// case and filename punctuation; exclusions always win. File size is read from
/// torrent metadata before payload downloading begins.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TorrentFileRules {
    pub file_types: Option<Vec<TorrentFileKind>>,
    pub name_contains: Vec<String>,
    pub name_excludes: Vec<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
}

impl TorrentFileRules {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .file_types
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 4)
        {
            return Err("Choose at least one file type.".into());
        }
        for names in [&self.name_contains, &self.name_excludes] {
            if names.len() > 32
                || names.iter().any(|s| {
                    normalized_name(s).is_empty()
                        || s.len() > 128
                        || s.chars().any(char::is_control)
                })
            {
                return Err("Use up to 32 names, each between 1 and 128 bytes.".into());
            }
        }
        if self.min_size.zip(self.max_size).is_some_and(|(a, b)| a > b)
            || [self.min_size, self.max_size]
                .into_iter()
                .flatten()
                .any(|v| v > 9_007_199_254_740_991)
        {
            return Err(
                "Choose a valid file size range, with minimum no greater than maximum.".into(),
            );
        }
        Ok(())
    }

    pub fn matches(&self, path: &str, size: u64) -> bool {
        let kind = TorrentFileKind::of(path);
        let name = normalized_name(path);
        self.file_types
            .as_ref()
            .is_none_or(|types| types.contains(&kind))
            && self.min_size.is_none_or(|v| size >= v)
            && self.max_size.is_none_or(|v| size <= v)
            && (self.name_contains.is_empty()
                || self
                    .name_contains
                    .iter()
                    .any(|s| name.contains(&normalized_name(s))))
            && !self
                .name_excludes
                .iter()
                .any(|s| name.contains(&normalized_name(s)))
    }
}

fn normalized_name(value: &str) -> String {
    value
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectTorrentFiles {
    pub indices: Vec<usize>,
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
    /// Permit uploading after completion. Older callers and saved limits default off.
    #[serde(default)]
    pub seed: bool,
    /// Append the maintained public tracker list. Older callers remain opted out.
    #[serde(default)]
    pub add_public_trackers: bool,
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

#[cfg(test)]
mod file_rule_tests {
    use super::*;

    #[test]
    fn rules_combine_type_name_exclusions_and_inclusive_size() {
        let rules = TorrentFileRules {
            file_types: Some(vec![TorrentFileKind::Video]),
            name_contains: vec!["Jane Doe".into(), "Álex Smith".into()],
            name_excludes: vec!["trailer".into()],
            min_size: Some(100),
            max_size: Some(200),
        };
        assert!(rules.validate().is_ok());
        assert!(rules.matches("CAST/JANE.DOE/Feature.MKV", 100));
        assert!(rules.matches("Álex_Smith/film.mp4", 200));
        for (path, size) in [
            ("Jane Doe/subtitle.srt", 150),
            ("Jane Doe/trailer.mp4", 150),
            ("Other/film.mp4", 150),
            ("Jane Doe/film.mp4", 99),
            ("Jane Doe/film.mp4", 201),
        ] {
            assert!(!rules.matches(path, size), "{path} {size}");
        }
    }

    #[test]
    fn invalid_rules_are_rejected_and_old_requests_still_decode() {
        for rules in [
            TorrentFileRules {
                file_types: Some(vec![]),
                ..Default::default()
            },
            TorrentFileRules {
                name_contains: vec!["...".into()],
                ..Default::default()
            },
            TorrentFileRules {
                name_excludes: vec!["x\ny".into()],
                ..Default::default()
            },
            TorrentFileRules {
                min_size: Some(2),
                max_size: Some(1),
                ..Default::default()
            },
            TorrentFileRules {
                max_size: Some(u64::MAX),
                ..Default::default()
            },
        ] {
            assert!(rules.validate().is_err());
        }
        let old: AddTorrentRequest =
            serde_json::from_str(r#"{"magnet":"magnet:?xt=test"}"#).unwrap();
        assert!(old.file_rules.is_none());
        assert!(TorrentFileRules::default().matches("readme.txt", 0));
        assert!(serde_json::from_str::<TorrentFileRules>(r#"{"min_szie":10}"#).is_err());
    }
}
