//! /api/system: server facts for the owner's settings page
//! (see docs/architecture.md section 8).

use std::path::PathBuf;
use std::time::Duration;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use stash_core::config::HwAccel;
use stash_core::jobs::JobCounts;
use stash_core::media::tools;

use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;

/// How long the torrent engine gets to answer a health check.
const TORRENTD_TIMEOUT: Duration = Duration::from_secs(3);

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/system", get(system))
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Disk {
    pub total: u64,
    pub free: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HwAccelInfo {
    pub configured: HwAccel,
    pub available: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct TorrentdInfo {
    pub reachable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemInfo {
    pub version: String,
    pub stash_root: String,
    pub config_dir: String,
    pub disk: Disk,
    pub ffmpeg_version: Option<String>,
    pub hwaccel: HwAccelInfo,
    pub torrentd: TorrentdInfo,
    pub jobs: JobCounts,
    pub public_url: Option<String>,
    pub passkeys_ready: bool,
}

/// Size and free space of the filesystem holding `path` (space available to unprivileged users).
async fn disk_usage(path: PathBuf) -> Disk {
    let result = tokio::task::spawn_blocking(move || nix::sys::statvfs::statvfs(&path)).await;
    match result {
        Ok(Ok(stat)) => {
            #[allow(clippy::unnecessary_cast)] // field widths differ between platforms
            let fragment = stat.fragment_size() as u64;
            #[allow(clippy::unnecessary_cast)]
            let (blocks, available) = (stat.blocks() as u64, stat.blocks_available() as u64);
            Disk {
                total: blocks.saturating_mul(fragment),
                free: available.saturating_mul(fragment),
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "could not read disk usage of the stash");
            Disk::default()
        }
        Err(e) => {
            tracing::warn!(error = %e, "disk usage task failed");
            Disk::default()
        }
    }
}

async fn torrentd_reachable(state: &AppState) -> bool {
    if !state.torrentd.enabled() {
        return false;
    }
    let torrentd = state.torrentd.clone();
    // Run on its own task so a misbehaving client cannot fail the whole page.
    let check = tokio::spawn(async move { torrentd.health().await.is_ok() });
    matches!(
        tokio::time::timeout(TORRENTD_TIMEOUT, check).await,
        Ok(Ok(true))
    )
}

async fn system(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<SystemInfo>> {
    user.require_owner()?;
    let config = &state.config;
    let (disk, ffmpeg_version, available, reachable, jobs, settings) = tokio::join!(
        disk_usage(config.stash_root.clone()),
        tools::ffmpeg_version(),
        tools::available_hwaccels(),
        torrentd_reachable(&state),
        state.jobs.counts(),
        state.settings.get(),
    );
    Ok(Json(SystemInfo {
        version: stash_core::VERSION.to_string(),
        stash_root: config.stash_root.display().to_string(),
        config_dir: config.config_dir.display().to_string(),
        disk,
        ffmpeg_version,
        hwaccel: HwAccelInfo {
            configured: settings.transcode.hwaccel,
            available,
        },
        torrentd: TorrentdInfo { reachable },
        jobs: jobs?,
        public_url: config
            .public_url
            .as_ref()
            .map(|u| u.as_str().trim_end_matches('/').to_string()),
        passkeys_ready: config.passkeys_ready(),
    }))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;
    use crate::api::library::testing::TestApp;

    #[tokio::test]
    async fn disk_usage_reads_a_real_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let disk = disk_usage(dir.path().to_path_buf()).await;
        assert!(disk.total > 0);
        assert!(disk.free <= disk.total);
        let missing = disk_usage(dir.path().join("nope")).await;
        assert_eq!(missing, Disk::default());
    }

    #[tokio::test]
    async fn system_is_owner_only_and_has_the_contract_shape() {
        let app = TestApp::new().await;
        let (_, member) = app.user("member").await;
        let (status, _) = app.json("/api/system", &member).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (_, owner) = app.user("owner").await;
        let (status, json) = app.json("/api/system", &owner).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        for key in [
            "version",
            "stash_root",
            "config_dir",
            "disk",
            "ffmpeg_version",
            "hwaccel",
            "torrentd",
            "jobs",
            "public_url",
            "passkeys_ready",
        ] {
            assert!(json.get(key).is_some(), "missing {key}: {json}");
        }
        assert_eq!(json["torrentd"]["reachable"], false);
        assert_eq!(json["hwaccel"]["configured"], "none");
        assert_eq!(json["public_url"], "http://localhost:8788");
        assert_eq!(json["passkeys_ready"], true);
        assert!(json["disk"]["total"].as_u64().unwrap() > 0);
    }
}
