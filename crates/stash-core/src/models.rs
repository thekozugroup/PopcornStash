//! Library and download models shared by the server and background workers.
//! JSON shapes follow docs/architecture.md section 8.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::capture::CaptureTime;

macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
        #[serde(rename_all = "snake_case")]
        #[sqlx(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

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
text_enum!(
    /// Which kind of place holds an item (section 6): a library folder, `Other/`, `Inbox/`
    /// or `Downloads/Completed/`, a top-level folder that is not a library, or the trash.
    Place { Library => "library", OtherFiles => "other_files", Incoming => "incoming", Folder => "folder", Trash => "trash" }
);
text_enum!(
    /// Why a waiting Inbox item waits, worked out when it is read (section 8).
    Waiting { ChooseLibrary => "choose_library", FolderUnavailable => "folder_unavailable", LowConfidence => "low_confidence", Confirm => "confirm" }
);

text_enum!(
    /// What a library holds (section 3.1).
    ContentType { Video => "video", Photos => "photos" }
);
text_enum!(
    /// How a video library is arranged: series with seasons and episodes, or one-offs.
    Structure { Episodes => "episodes", OneOffs => "one_offs" }
);
text_enum!(Visibility { Everyone => "everyone", Selected => "selected" });
text_enum!(
    /// How clients draw a library (section 8, Layout), worked out by the server.
    Layout { Posters => "posters", Frames => "frames", Series => "series", Timeline => "timeline" }
);
text_enum!(
    /// Whether a library's folder can be used right now (section 3.1).
    FolderState { Ok => "ok", Missing => "missing", ReadOnly => "read_only" }
);

/// What decides who may see an item (section 7): kept with every item event so each
/// event stream can check it against its caller without reading the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemFacts {
    pub library_id: Option<String>,
    pub place: Place,
    pub added_by: Option<String>,
    pub status: ItemStatus,
    pub category: Category,
}

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
    /// When it was taken (section 4, Capture time), in three columns that
    /// [`ItemRow::taken`] reads together: the moment, the camera's clock and its offset.
    pub taken_at: Option<i64>,
    pub taken_local: Option<String>,
    pub taken_offset: Option<i32>,
    pub camera: Option<String>,
    pub thumb_state: ThumbState,
    pub suggestion_json: Option<String>,
    pub favorite: bool,
    pub source_download_id: Option<String>,
    pub missing_since: Option<i64>,
    pub added_at: i64,
    pub updated_at: i64,
    /// The library whose folder holds the file (section 3.1), kept by the database.
    pub library_id: Option<String>,
    pub place: Place,
    /// The person whose download, upload or share brought the file in.
    pub added_by: Option<String>,
}

impl ItemRow {
    pub fn facts(&self) -> ItemFacts {
        ItemFacts {
            library_id: self.library_id.clone(),
            place: self.place,
            added_by: self.added_by.clone(),
            status: self.status,
            category: self.category,
        }
    }

    /// When it was taken, from the three capture time columns.
    pub fn taken(&self) -> Option<CaptureTime> {
        CaptureTime::from_stored(
            self.taken_at,
            self.taken_local.as_deref(),
            self.taken_offset,
        )
    }

    pub fn set_taken(&mut self, taken: Option<CaptureTime>) {
        (self.taken_at, self.taken_local, self.taken_offset) = CaptureTime::columns(taken.as_ref());
    }

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
            taken_local: self.taken_local.clone(),
            taken_offset: self.taken_offset,
            camera: self.camera.clone(),
            thumb: (self.thumb_state == ThumbState::Ready)
                .then(|| format!("/api/items/{}/thumb?size=sm&v={}", self.id, self.updated_at)),
            favorite: self.favorite,
            added_at: self.added_at,
            progress,
            library_id: self.library_id.clone(),
            place: self.place,
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
    /// When it was taken (section 4, Capture time): the moment, unix seconds.
    pub taken_at: Option<i64>,
    /// The camera's clock where it was taken, `YYYY-MM-DDTHH:MM:SS`: what every viewer sees.
    pub taken_local: Option<String>,
    /// Minutes east of UTC, when the file wrote one beside the clock.
    pub taken_offset: Option<i32>,
    pub camera: Option<String>,
    pub thumb: Option<String>,
    pub favorite: bool,
    pub added_at: i64,
    pub progress: Option<Progress>,
    pub library_id: Option<String>,
    pub place: Place,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDetail {
    #[serde(flatten)]
    pub item: Item,
    pub media: Option<MediaSummary>,
    pub mime: Option<String>,
    pub suggestion: Option<Suggestion>,
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
    /// When it was taken, as on `Item` (section 4, Capture time). Suggestions stored
    /// before migration 0003 have only `taken_at`; the migration adds the other two.
    pub taken_at: Option<i64>,
    #[serde(default)]
    pub taken_local: Option<String>,
    #[serde(default)]
    pub taken_offset: Option<i32>,
    pub confidence: Confidence,
    /// Short human-readable explanation about the file itself, e.g. "Found S01E02 in the name".
    pub reason: String,
    /// Where the file would sit inside a library folder of its kind (`Title (Year)/Title
    /// (Year).mkv`, `2024/06/IMG_1.jpg`), or its name for `other`. Worked out by the
    /// classifier and never stored: where a file goes depends on the claims when it is read.
    #[serde(skip)]
    pub layout: String,
}

impl Classification {
    /// When it was taken, from the three capture time fields.
    pub fn taken(&self) -> Option<CaptureTime> {
        CaptureTime::from_stored(
            self.taken_at,
            self.taken_local.as_deref(),
            self.taken_offset,
        )
    }

    pub fn set_taken(&mut self, taken: Option<CaptureTime>) {
        (self.taken_at, self.taken_local, self.taken_offset) = CaptureTime::columns(taken.as_ref());
    }
}

/// A stored classification as a reader sees it (`ItemDetail.suggestion`, section 8): the
/// classification plus where it would go and why it waits, worked out at read time from
/// the claims, the library folders and the reader's access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Suggestion {
    #[serde(flatten)]
    pub classification: Classification,
    /// `library` or `other_files`; None when no library the reader can see claims the kind.
    pub place: Option<Place>,
    pub library_id: Option<String>,
    /// The relative path the file would get; None when `place` is None.
    pub destination: Option<String>,
    /// Why a `needs_review` item waits; None for other items.
    pub waiting: Option<Waiting>,
}

/// Where a download's visible items are now (section 8, `Download.destinations`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Destination {
    pub place: Place,
    pub library_id: Option<String>,
    pub count: i64,
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
    pub library_id: Option<String>,
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
            library_id: self.library_id.clone(),
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
            destinations: Vec::new(),
            created_by: self.created_by.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Download {
    pub id: String,
    #[serde(default)]
    pub library_id: Option<String>,
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
    /// Linked items the reader can see.
    pub item_ids: Vec<String>,
    /// Where those items are now, grouped by place and library in first-seen order.
    #[serde(default)]
    pub destinations: Vec<Destination>,
    /// Who added it. Server side only: decides who sees it (section 7), never sent.
    #[serde(skip)]
    pub created_by: Option<String>,
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
