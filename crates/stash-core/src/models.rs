//! Library and download models shared by the server and background workers.
//! JSON shapes follow docs/architecture.md section 8.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
        #[serde(rename_all = "snake_case")]
        #[sqlx(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = crate::CoreError;
            fn from_str(s: &str) -> crate::Result<Self> {
                match s {
                    $($text => Ok(Self::$variant),)+
                    other => Err(crate::CoreError::invalid(format!(
                        concat!("unknown ", stringify!($name), " {:?}"), other
                    ))),
                }
            }
        }
    };
}

text_enum!(
    /// What a file is, from its extension.
    MediaKind { Video => "video", Photo => "photo", Audio => "audio", Other => "other" }
);
text_enum!(
    /// Where an item belongs in the library.
    Category { Movie => "movie", Episode => "episode", HomeVideo => "home_video", Photo => "photo", Other => "other" }
);
text_enum!(ItemStatus { Ready => "ready", NeedsReview => "needs_review", Missing => "missing", Trashed => "trashed" });
text_enum!(ThumbState { Pending => "pending", Ready => "ready", Failed => "failed", None => "none" });
text_enum!(Role { Owner => "owner", Member => "member" });
text_enum!(DownloadKind { Torrent => "torrent", Http => "http", Upload => "upload" });
text_enum!(DownloadState {
    Queued => "queued",
    Downloading => "downloading",
    Paused => "paused",
    Seeding => "seeding",
    Completed => "completed",
    Sorting => "sorting",
    Sorted => "sorted",
    Failed => "failed",
    Cancelled => "cancelled",
});
text_enum!(Confidence { High => "high", Low => "low" });

impl DownloadState {
    /// Still moving bytes or waiting to.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Downloading | Self::Paused | Self::Seeding | Self::Sorting
        )
    }
}

/// One row of `items`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ItemRow {
    pub id: String,
    pub rel_path: String,
    pub kind: MediaKind,
    pub category: Category,
    pub status: ItemStatus,
    pub title: String,
    pub sort_title: String,
    pub year: Option<i64>,
    pub show_name: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub episode_title: Option<String>,
    pub size: i64,
    pub mtime: i64,
    pub quick_hash: Option<String>,
    pub mime: Option<String>,
    pub duration_s: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub media_json: Option<String>,
    pub taken_at: Option<i64>,
    pub camera: Option<String>,
    pub thumb_state: ThumbState,
    pub suggestion_json: Option<String>,
    pub favorite: bool,
    pub source_download_id: Option<String>,
    pub missing_since: Option<i64>,
    pub added_at: i64,
    pub updated_at: i64,
}

impl ItemRow {
    pub fn media(&self) -> Option<MediaSummary> {
        self.media_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
    }

    pub fn suggestion(&self) -> Option<Classification> {
        self.suggestion_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
    }

    /// API shape. `progress` is the requesting user's playback position.
    pub fn to_item(&self, progress: Option<Progress>) -> Item {
        Item {
            id: self.id.clone(),
            kind: self.kind,
            category: self.category,
            status: self.status,
            title: self.title.clone(),
            year: self.year,
            show_name: self.show_name.clone(),
            season: self.season,
            episode: self.episode,
            episode_title: self.episode_title.clone(),
            rel_path: self.rel_path.clone(),
            size: self.size,
            duration_s: self.duration_s,
            width: self.width,
            height: self.height,
            taken_at: self.taken_at,
            camera: self.camera.clone(),
            thumb: (self.thumb_state == ThumbState::Ready)
                .then(|| format!("/api/items/{}/thumb?size=sm&v={}", self.id, self.updated_at)),
            favorite: self.favorite,
            added_at: self.added_at,
            progress,
        }
    }
}

/// Playback position for one user.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Progress {
    pub position_s: f64,
    pub duration_s: f64,
    pub finished: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: MediaKind,
    pub category: Category,
    pub status: ItemStatus,
    pub title: String,
    pub year: Option<i64>,
    pub show_name: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub episode_title: Option<String>,
    pub rel_path: String,
    pub size: i64,
    pub duration_s: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub taken_at: Option<i64>,
    pub camera: Option<String>,
    pub thumb: Option<String>,
    pub favorite: bool,
    pub added_at: i64,
    pub progress: Option<Progress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDetail {
    #[serde(flatten)]
    pub item: Item,
    pub media: Option<MediaSummary>,
    pub mime: Option<String>,
    pub suggestion: Option<Classification>,
    pub source_download_id: Option<String>,
    pub next_episode_id: Option<String>,
    pub previous_episode_id: Option<String>,
}

/// Result of classifying a file (section 4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Classification {
    pub category: Category,
    pub title: String,
    pub year: Option<i64>,
    pub show_name: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub episode_title: Option<String>,
    pub taken_at: Option<i64>,
    pub confidence: Confidence,
    /// Short human-readable explanation, e.g. "Found S01E02 in the name".
    pub reason: String,
    /// Suggested destination, relative to the stash root.
    pub destination: String,
}

/// ffprobe summary stored in `items.media_json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MediaSummary {
    pub container: Option<String>,
    pub duration_s: Option<f64>,
    pub bitrate: Option<i64>,
    pub video: Option<VideoStream>,
    #[serde(default)]
    pub audio: Vec<AudioStream>,
    #[serde(default)]
    pub subtitles: Vec<SubtitleStream>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VideoStream {
    pub index: i64,
    pub codec: String,
    pub profile: Option<String>,
    pub pix_fmt: Option<String>,
    pub width: i64,
    pub height: i64,
    pub fps: Option<f64>,
    /// `hdr10`, `hlg`, `dolby_vision` or None for SDR.
    pub hdr: Option<String>,
    /// Display rotation in degrees (0, 90, 180, 270).
    pub rotation: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AudioStream {
    pub index: i64,
    pub codec: String,
    pub channels: Option<i64>,
    pub language: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SubtitleStream {
    pub index: i64,
    pub codec: String,
    pub language: Option<String>,
    pub title: Option<String>,
    pub external: bool,
}

/// One row of `downloads`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DownloadRow {
    pub id: String,
    pub kind: DownloadKind,
    pub name: String,
    pub source: String,
    pub state: DownloadState,
    pub progress: f64,
    pub total_bytes: Option<i64>,
    pub done_bytes: i64,
    pub down_bps: i64,
    pub up_bps: i64,
    pub peers: Option<i64>,
    pub eta_s: Option<i64>,
    pub error: Option<String>,
    pub rel_path: Option<String>,
    pub info_hash: Option<String>,
    pub engine_id: Option<String>,
    pub created_by: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub sorted_at: Option<i64>,
}

impl DownloadRow {
    pub fn to_download(&self, item_ids: Vec<String>) -> Download {
        Download {
            id: self.id.clone(),
            kind: self.kind,
            name: self.name.clone(),
            source: self.source.clone(),
            state: self.state,
            progress: self.progress,
            total_bytes: self.total_bytes,
            done_bytes: self.done_bytes,
            down_bps: self.down_bps,
            up_bps: self.up_bps,
            peers: self.peers,
            eta_s: self.eta_s,
            error: self.error.clone(),
            created_at: self.created_at,
            completed_at: self.completed_at,
            item_ids,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Download {
    pub id: String,
    pub kind: DownloadKind,
    pub name: String,
    pub source: String,
    pub state: DownloadState,
    pub progress: f64,
    pub total_bytes: Option<i64>,
    pub done_bytes: i64,
    pub down_bps: i64,
    pub up_bps: i64,
    pub peers: Option<i64>,
    pub eta_s: Option<i64>,
    pub error: Option<String>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub item_ids: Vec<String>,
}

/// VPN state as shown to the UI (section 8).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VpnStatus {
    pub configured: bool,
    pub reachable: bool,
    pub state: VpnState,
    pub provider: Option<crate::internal_api::VpnProvider>,
    pub protocol: Option<crate::internal_api::VpnProtocol>,
    pub label: Option<String>,
    pub server: Option<String>,
    pub country: Option<String>,
    pub public_ip: Option<String>,
    pub protected: Option<bool>,
    pub since: Option<i64>,
    pub error: Option<String>,
    pub kill_switch: bool,
    pub allow_no_vpn: bool,
    /// Torrents are off on this server by design (degraded mode, or `serve` running
    /// without the supervisor); `error` says why. Nothing waits for the VPN then:
    /// adding a torrent answers `vpn_unavailable`.
    #[serde(default)]
    pub torrents_off: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VpnState {
    Connected,
    Connecting,
    Disconnected,
    Error,
    NotConfigured,
    Unreachable,
}

impl VpnStatus {
    /// Status when the torrent container cannot be reached or torrents are disabled.
    pub fn unreachable(configured: bool, error: impl Into<String>) -> Self {
        Self {
            configured,
            reachable: false,
            state: VpnState::Unreachable,
            provider: None,
            protocol: None,
            label: None,
            server: None,
            country: None,
            public_ip: None,
            protected: None,
            since: None,
            error: Some(error.into()),
            kill_switch: true,
            allow_no_vpn: false,
            torrents_off: false,
        }
    }
}
