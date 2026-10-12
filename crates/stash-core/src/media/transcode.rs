//! Live HLS transcoding sessions (section 5).

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::models::MediaSummary;
use crate::settings::SettingsStore;
use crate::Result;

pub const SEGMENT_SECONDS: f64 = 6.0;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct StreamOptions {
    pub max_height: Option<u32>,
    pub start_s: Option<f64>,
    pub audio_index: Option<i64>,
    /// Client can display HDR; otherwise HDR is tone mapped.
    pub hdr: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionInfo {
    pub session_id: String,
    /// `/api/stream/<session>/index.m3u8`
    pub url: String,
}

pub struct TranscodeManager {
    pub config: Arc<Config>,
    pub settings: SettingsStore,
}

impl TranscodeManager {
    pub fn new(config: Arc<Config>, settings: SettingsStore) -> Self {
        let _ = (&config, &settings);
        todo!("media agent")
    }

    /// Start a session for `item_id` whose original is `source`.
    pub async fn start(&self, item_id: &str, source: PathBuf, media: &MediaSummary, options: StreamOptions) -> Result<SessionInfo> {
        let _ = (item_id, source, media, options);
        todo!("media agent")
    }

    /// The full VOD playlist for a session.
    pub async fn playlist(&self, session_id: &str) -> Result<String> {
        let _ = session_id;
        todo!("media agent")
    }

    /// Path of a finished segment (waits for it or restarts ffmpeg as needed). `name` is e.g. `12.ts`.
    pub async fn segment(&self, session_id: &str, name: &str) -> Result<PathBuf> {
        let _ = (session_id, name);
        todo!("media agent")
    }

    pub async fn stop(&self, session_id: &str) {
        let _ = session_id;
        todo!("media agent")
    }

    /// Kill sessions idle for 60s and delete their folders.
    pub fn spawn_reaper(self: &Arc<Self>, cancel: CancellationToken) -> JoinHandle<()> {
        let _ = cancel;
        todo!("media agent")
    }
}
