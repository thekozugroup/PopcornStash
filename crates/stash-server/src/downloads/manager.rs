//! Download manager: one row in `downloads` per web link, torrent or finished
//! upload. Everything flows `Downloads/In Progress` → `Downloads/Completed` → sort
//! (docs/architecture.md section 5).

use std::collections::HashSet;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use dashmap::{DashMap, DashSet};
use serde::{Deserialize, Serialize};
use stash_core::config::Config;
use stash_core::db::Db;
use stash_core::events::{Event, EventBus};
use stash_core::jobs::JobQueue;
use stash_core::library::Library;
use stash_core::models::{Download, DownloadKind, DownloadRow, DownloadState};
use stash_core::paths::{self, StashLayout, Zone};
use stash_core::settings::{HttpViaVpn, SettingsStore};
use stash_core::{new_id, now};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::source::{self, Magnet};
use super::ssrf::AddressGuard;
use super::torrents::InFlight;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::torrentd_client::{TorrentdClient, TorrentdError};

/// Largest `.torrent` file accepted (upload or link).
pub const MAX_TORRENT_FILE: usize = 10 * 1024 * 1024;
/// Shown while a torrent waits for the engine.
pub const MSG_WAITING_FOR_VPN: &str = "Waiting for the VPN to connect.";
/// Web downloads set to `always` use the VPN and it is down.
pub const MSG_VPN_REQUIRED: &str =
    "Downloads are set to use the VPN, and the VPN is not connected.";
/// Web downloads set to `always` use the VPN and the link uses a port its proxy cannot reach.
pub const MSG_VPN_PORTS: &str =
    "Downloads through the VPN can only reach ports 80 and 443, and this link uses another port.";
/// A link whose private parts were only kept in memory, after a restart.
pub const MSG_LINK_EXPIRED: &str = "This link needed a private access code or your browser's sign-in, which are not kept after a restart. Send the link again.";
/// Someone re-added a stored link that lost its private parts.
pub const MSG_LINK_INCOMPLETE: &str = "This link is missing the private access code it had. Add it again from the page you got it from.";
const MSG_BEING_SORTED: &str = "This download is being sorted. Try again in a moment.";

/// Headers the Chrome extension may forward. Kept in memory only.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ForwardedHeaders {
    pub cookie: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
}

impl std::fmt::Debug for ForwardedHeaders {
    // Never print header values: they can carry session cookies.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let shown = |v: &Option<String>| if v.is_some() { "<set>" } else { "<none>" };
        f.debug_struct("ForwardedHeaders")
            .field("cookie", &shown(&self.cookie))
            .field("referer", &shown(&self.referer))
            .field("user_agent", &shown(&self.user_agent))
            .finish()
    }
}

impl ForwardedHeaders {
    /// Drop empty values; None when nothing is left.
    fn cleaned(self) -> Option<Self> {
        let keep = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let cleaned = Self {
            cookie: keep(self.cookie),
            referer: keep(self.referer),
            user_agent: keep(self.user_agent),
        };
        (cleaned.cookie.is_some() || cleaned.referer.is_some() || cleaned.user_agent.is_some())
            .then_some(cleaned)
    }
}

/// `POST /api/downloads`.
#[derive(Clone, Serialize, Deserialize)]
pub struct AddDownloadRequest {
    pub url: String,
    pub filename: Option<String>,
    pub headers: Option<ForwardedHeaders>,
}

impl std::fmt::Debug for AddDownloadRequest {
    // The URL may carry a token in its query; print only what is safe to store.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let url = Url::parse(&self.url)
            .map(|u| source::redact_url(&u))
            .unwrap_or_else(|_| "<unparsed>".into());
        f.debug_struct("AddDownloadRequest")
            .field("url", &url)
            .field("filename", &self.filename)
            .field("headers", &self.headers)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadFilter {
    #[default]
    All,
    Active,
    Finished,
}

/// What a web download needs that must never be stored: the full URL (it may carry a
/// token) and the forwarded headers.
#[derive(Clone)]
pub(crate) struct RequestSecrets {
    pub url: Url,
    pub headers: ForwardedHeaders,
}

/// Timings, shortened by tests.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tuning {
    /// First retry delay for web downloads; doubles each attempt.
    pub retry_delay: Duration,
    /// Web worker loop tick (it also wakes on demand).
    pub worker_tick: Duration,
    /// Torrent poll interval while something is moving.
    pub torrent_fast: Duration,
    /// Torrent poll interval when idle.
    pub torrent_slow: Duration,
    /// Longest wait between polls while the engine is unreachable.
    pub engine_backoff_max: Duration,
    /// A torrent missing from the engine list for this long has failed.
    pub torrent_missing: Duration,
    /// Wait before trying again to move a finished torrent that could not be moved.
    pub finish_retry: Duration,
    /// Longest time fetching a `.torrent` link may take.
    pub torrent_fetch_timeout: Duration,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            retry_delay: Duration::from_secs(2),
            worker_tick: Duration::from_secs(2),
            torrent_fast: Duration::from_secs(2),
            torrent_slow: Duration::from_secs(10),
            engine_backoff_max: Duration::from_secs(60),
            torrent_missing: Duration::from_secs(60),
            finish_retry: Duration::from_secs(60),
            torrent_fetch_timeout: Duration::from_secs(60),
        }
    }
}

/// Fields of a new `downloads` row.
pub(crate) struct NewDownload<'a> {
    pub kind: DownloadKind,
    pub name: &'a str,
    pub source: &'a str,
    pub state: DownloadState,
    pub created_by: &'a str,
    pub rel_path: Option<&'a str>,
    pub total_bytes: Option<i64>,
    pub done_bytes: i64,
    pub error: Option<&'a str>,
}

pub struct DownloadManager {
    pub config: Arc<Config>,
    pub db: Db,
    pub layout: Arc<StashLayout>,
    pub events: EventBus,
    pub jobs: JobQueue,
    pub settings: SettingsStore,
    pub library: Arc<Library>,
    pub torrentd: Arc<TorrentdClient>,
    /// Destination policy for every web request.
    pub(crate) guard: Arc<AddressGuard>,
    /// Full URLs and forwarded headers by download id. Memory only.
    pub(crate) secrets: DashMap<String, RequestSecrets>,
    /// Running web downloads and how to stop them.
    pub(crate) running: DashMap<String, CancellationToken>,
    /// Rows still being added: the torrent poller does not add them and the web workers
    /// do not start them yet.
    pub(crate) adding: DashSet<String>,
    /// Names claimed in `In Progress` by downloads that have not written their file yet.
    reserved: Mutex<HashSet<String>>,
    pub(crate) http_wake: Notify,
    pub(crate) torrent_wake: Notify,
    tuning: RwLock<Tuning>,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn not_found() -> ApiError {
    ApiError::not_found("That download no longer exists.")
}

impl DownloadManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<Config>,
        db: Db,
        layout: Arc<StashLayout>,
        events: EventBus,
        jobs: JobQueue,
        settings: SettingsStore,
        library: Arc<Library>,
        torrentd: Arc<TorrentdClient>,
    ) -> Self {
        let guard = AddressGuard::new();
        // The engine API and its proxy are never valid download targets.
        for url in config
            .torrentd_url
            .iter()
            .chain(config.torrentd_proxy.iter())
        {
            if let Some(host) = url.host_str() {
                guard.block_name(host);
            }
        }
        Self {
            config,
            db,
            layout,
            events,
            jobs,
            settings,
            library,
            torrentd,
            guard: Arc::new(guard),
            secrets: DashMap::new(),
            running: DashMap::new(),
            adding: DashSet::new(),
            reserved: Mutex::new(HashSet::new()),
            http_wake: Notify::new(),
            torrent_wake: Notify::new(),
            tuning: RwLock::new(Tuning::default()),
        }
    }

    /// Trust a private or local address as a web download destination. Nothing in the
    /// API calls this; it exists for tests and deliberate local setups.
    pub fn allow_destination(&self, ip: IpAddr) {
        self.guard.allow(ip);
    }

    pub(crate) fn tuning(&self) -> Tuning {
        *self.tuning.read().unwrap_or_else(|p| p.into_inner())
    }

    #[cfg(test)]
    pub(crate) fn set_tuning(&self, tuning: Tuning) {
        *self.tuning.write().unwrap_or_else(|p| p.into_inner()) = tuning;
    }

    /// Recover state after restart and spawn the web-download workers and the torrent poller.
    pub fn start(self: &Arc<Self>, cancel: CancellationToken) {
        let manager = self.clone();
        tokio::spawn(async move {
            if let Err(e) = manager.recover().await {
                tracing::warn!(error = %e, "could not recover downloads after restart");
            }
            super::http::spawn_worker_loop(manager.clone(), cancel.clone());
            if manager.torrentd.enabled() {
                super::torrents::spawn_poller(manager.clone(), cancel.clone());
            }
        });
    }

    /// Web downloads that were running resume; anything waiting for the sorter is queued again.
    async fn recover(&self) -> ApiResult<()> {
        let t = now();
        sqlx::query(
            "UPDATE downloads SET state = 'queued', down_bps = 0, eta_s = NULL, updated_at = ?
             WHERE kind = 'http' AND state = 'downloading'",
        )
        .bind(t)
        .execute(&self.db)
        .await?;
        // Names of partial web downloads stay claimed across restarts, even before
        // their .part file exists.
        let claimed: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM downloads WHERE kind = 'http' AND rel_path LIKE 'Downloads/In Progress/%.part'
             AND state IN ('queued', 'downloading', 'paused', 'failed')",
        )
        .fetch_all(&self.db)
        .await?;
        lock(&self.reserved).extend(claimed.into_iter().map(|(name,)| name));
        let waiting: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, rel_path FROM downloads WHERE state IN ('sorting', 'completed') AND rel_path IS NOT NULL",
        )
        .fetch_all(&self.db)
        .await?;
        for (id, rel) in waiting {
            if StashLayout::zone(&rel) != Zone::Completed {
                continue;
            }
            if let Err(e) = self.library.enqueue_sort(&rel, Some(&id)).await {
                tracing::warn!(error = %e, download = %id, "could not queue a sort after restart");
                continue;
            }
            sqlx::query("UPDATE downloads SET state = 'sorting', updated_at = ? WHERE id = ? AND state = 'completed'")
                .bind(t)
                .bind(&id)
                .execute(&self.db)
                .await?;
        }
        Ok(())
    }

    /// Add an http(s) link or magnet link.
    pub async fn add_url(&self, user_id: &str, request: AddDownloadRequest) -> ApiResult<Download> {
        let raw = request.url.trim();
        if raw.is_empty() {
            return Err(ApiError::bad_request("Paste a link to download."));
        }
        if raw
            .get(..7)
            .is_some_and(|p| p.eq_ignore_ascii_case("magnet:"))
        {
            self.require_torrents()?;
            let magnet = Magnet::parse(raw)
                .ok_or_else(|| ApiError::bad_request("That magnet link is not valid."))?;
            let add = stash_core::internal_api::AddTorrentRequest {
                magnet: Some(raw.to_string()),
                torrent_base64: None,
            };
            return self
                .add_torrent(user_id, add, magnet.name(), magnet.redacted())
                .await;
        }
        let url = Url::parse(raw).map_err(|_| ApiError::bad_request("That link is not valid."))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ApiError::bad_request(
                "Only http, https and magnet links can be downloaded.",
            ));
        }
        // A stored source sent back (a retry): its private parts are gone.
        if source::is_redacted(&url) {
            return Err(ApiError::bad_request(MSG_LINK_INCOMPLETE));
        }
        if url.host_str().is_none_or(str::is_empty) {
            return Err(ApiError::bad_request("That link is not valid."));
        }
        let headers = request.headers.and_then(ForwardedHeaders::cleaned);

        if source::is_torrent_url(&url) {
            self.require_torrents()?;
            self.guard
                .check_url(&url)
                .await
                .map_err(|e| ApiError::bad_request(e.message()))?;
            let bytes = self
                .fetch_torrent_file(&url, headers.unwrap_or_default())
                .await?;
            let filename = source::name_from_url(&url);
            return self.add_torrent_file(user_id, bytes, filename).await;
        }

        self.guard
            .check_url(&url)
            .await
            .map_err(|e| ApiError::bad_request(e.message()))?;
        let chosen = request
            .filename
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let (name, rel_path) = match chosen {
            Some(filename) => {
                let name = self.reserve_name(&paths::sanitize_filename(filename));
                let rel = format!("{}/{}.part", paths::IN_PROGRESS, name);
                (name, Some(rel))
            }
            None => {
                let guess =
                    source::name_from_url(&url).or_else(|| url.host_str().map(str::to_string));
                (
                    paths::sanitize_filename(guess.as_deref().unwrap_or("download")),
                    None,
                )
            }
        };
        // A download that depends on the browser's cookies cannot be fetched again after a
        // restart (the cookies are memory only), so it is stored as incomplete too.
        let needs_session = headers.as_ref().is_some_and(|h| h.cookie.is_some());
        let stored_source = source::redact_url_with(&url, needs_session);
        // Workers leave the row alone until its `queued` event is out, so events arrive
        // in order and the answer below is the row as added.
        let id = new_id();
        let hold = InFlight::new(&self.adding, &id);
        self.insert_row_as(
            &id,
            NewDownload {
                kind: DownloadKind::Http,
                name: &name,
                source: &stored_source,
                state: DownloadState::Queued,
                created_by: user_id,
                rel_path: rel_path.as_deref(),
                total_bytes: None,
                done_bytes: 0,
                error: None,
            },
        )
        .await
        .inspect_err(|_| {
            if rel_path.is_some() {
                self.release_name(&name);
            }
        })?;
        self.secrets.insert(
            id.clone(),
            RequestSecrets {
                url,
                headers: headers.unwrap_or_default(),
            },
        );
        tracing::info!(download = %id, source = %stored_source, "web download queued");
        let download = self.publish_id(&id).await;
        drop(hold);
        self.http_wake.notify_one();
        download
    }

    /// Add a .torrent file.
    pub async fn add_torrent_file(
        &self,
        user_id: &str,
        bytes: Vec<u8>,
        filename: Option<String>,
    ) -> ApiResult<Download> {
        self.require_torrents()?;
        if bytes.len() > MAX_TORRENT_FILE {
            return Err(ApiError::bad_request(
                "The .torrent file is larger than 10 MB.",
            ));
        }
        let info_name = super::torrents::torrent_name(&bytes)
            .map_err(|_| ApiError::bad_request("That file is not a valid .torrent file."))?;
        let filename = filename.map(|f| paths::sanitize_filename(&f));
        let name = info_name
            .or_else(|| {
                filename.as_deref().map(|f| {
                    let lower = f.to_ascii_lowercase();
                    lower
                        .strip_suffix(".torrent")
                        .map(|_| f[..f.len() - 8].to_string())
                        .unwrap_or_else(|| f.to_string())
                })
            })
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "Torrent".to_string());
        let source = filename.unwrap_or_else(|| "upload.torrent".to_string());
        use base64::Engine as _;
        let add = stash_core::internal_api::AddTorrentRequest {
            magnet: None,
            torrent_base64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
        };
        self.add_torrent(user_id, add, name, source).await
    }

    /// A finished browser upload that now sits at `rel_path` (in the private partial
    /// uploads folder): moves it straight to Completed under `filename` (sanitized,
    /// never overwriting), records the download and queues the sort. If the download
    /// cannot be recorded, the file goes back to `rel_path` so completing can be tried
    /// again.
    pub async fn upload_finished(
        &self,
        user_id: &str,
        filename: &str,
        rel_path: &str,
        size: i64,
    ) -> ApiResult<Download> {
        let single_part = rel_path
            .strip_prefix(paths::UPLOADS_TMP)
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some_and(|name| {
                !name.is_empty() && !name.contains('/') && name != "." && name != ".."
            });
        if !single_part {
            return Err(ApiError::bad_request(
                "That upload is not in the partial uploads folder.",
            ));
        }
        let src = self.layout.abs(rel_path)?;
        match tokio::fs::symlink_metadata(&src).await {
            Ok(meta) if meta.is_file() => {}
            _ => return Err(ApiError::not_found("The uploaded file is missing.")),
        }
        let name = paths::sanitize_filename(filename);
        let (dest, rel) = self.move_to_completed(&src, &name).await.map_err(|e| {
            tracing::error!(error = %e, "could not move an upload to Completed");
            ApiError::new(
                ErrorCode::Internal,
                "The upload could not be moved to Downloads/Completed.",
            )
        })?;
        let inserted = self
            .insert_row(NewDownload {
                kind: DownloadKind::Upload,
                name: &name,
                source: &name,
                state: DownloadState::Completed,
                created_by: user_id,
                rel_path: Some(&rel),
                total_bytes: Some(size),
                done_bytes: size,
                error: None,
            })
            .await;
        let id = match inserted {
            Ok(id) => id,
            Err(e) => {
                if let Err(back) = paths::move_path(&dest, &src).await {
                    tracing::warn!(error = %back, "could not return an upload after a failure");
                }
                return Err(e);
            }
        };
        self.complete_and_sort(&id, &rel, Some(size)).await?;
        self.get(&id).await
    }

    pub async fn list(&self, filter: DownloadFilter, limit: u32) -> ApiResult<Vec<Download>> {
        let limit = i64::from(limit.clamp(1, 500));
        let filter_sql = match filter {
            DownloadFilter::All => "",
            DownloadFilter::Active => {
                "WHERE state IN ('queued', 'downloading', 'paused', 'seeding', 'sorting')"
            }
            DownloadFilter::Finished => {
                "WHERE state IN ('completed', 'sorted', 'failed', 'cancelled')"
            }
        };
        let sql = format!(
            "SELECT * FROM downloads {filter_sql} ORDER BY created_at DESC, id DESC LIMIT ?"
        );
        let rows: Vec<DownloadRow> = sqlx::query_as(&sql).bind(limit).fetch_all(&self.db).await?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let ids = serde_json::to_string(&rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>())
            .map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
        let links: Vec<(String, String)> = sqlx::query_as(
            "SELECT download_id, item_id FROM download_items
             WHERE download_id IN (SELECT value FROM json_each(?)) ORDER BY item_id",
        )
        .bind(ids)
        .fetch_all(&self.db)
        .await?;
        Ok(rows
            .iter()
            .map(|row| {
                let items = links
                    .iter()
                    .filter(|(d, _)| *d == row.id)
                    .map(|(_, i)| i.clone())
                    .collect();
                row.to_download(items)
            })
            .collect())
    }

    pub async fn get(&self, id: &str) -> ApiResult<Download> {
        let row = self.load_row(id).await?;
        let items = self.item_ids(id).await?;
        Ok(row.to_download(items))
    }

    pub async fn pause(&self, id: &str) -> ApiResult<Download> {
        let row = self.load_row(id).await?;
        match row.kind {
            DownloadKind::Upload => return Err(ApiError::conflict("Uploads cannot be paused.")),
            DownloadKind::Http => {
                if !matches!(
                    row.state,
                    DownloadState::Queued | DownloadState::Downloading | DownloadState::Paused
                ) {
                    return Err(ApiError::conflict(
                        "Only downloads in progress can be paused.",
                    ));
                }
                self.set_state_if(id, DownloadState::Paused, &["queued", "downloading"])
                    .await?;
                if let Some(token) = self.running.get(id) {
                    token.cancel();
                }
            }
            DownloadKind::Torrent => {
                if !matches!(
                    row.state,
                    DownloadState::Queued
                        | DownloadState::Downloading
                        | DownloadState::Seeding
                        | DownloadState::Paused
                ) {
                    return Err(ApiError::conflict(
                        "Only downloads in progress can be paused.",
                    ));
                }
                if let Some(engine_id) = row.engine_id.as_deref()
                    && row.state != DownloadState::Paused
                {
                    self.torrentd
                        .pause(engine_id)
                        .await
                        .map_err(|e| engine_error(&e))?;
                }
                self.set_state_if(
                    id,
                    DownloadState::Paused,
                    &["queued", "downloading", "seeding"],
                )
                .await?;
                self.torrent_wake.notify_one();
            }
        }
        self.publish_id(id).await
    }

    pub async fn resume(&self, id: &str) -> ApiResult<Download> {
        let row = self.load_row(id).await?;
        match row.kind {
            DownloadKind::Upload => {
                return Err(ApiError::conflict("Uploads cannot be resumed here."));
            }
            DownloadKind::Http => match row.state {
                DownloadState::Paused | DownloadState::Failed
                    if !self.secrets.contains_key(id)
                        && source::is_redacted_source(&row.source) =>
                {
                    return Err(ApiError::conflict(MSG_LINK_EXPIRED));
                }
                DownloadState::Paused | DownloadState::Failed => {
                    sqlx::query(
                        "UPDATE downloads SET state = 'queued', error = NULL, down_bps = 0, eta_s = NULL, updated_at = ?
                         WHERE id = ? AND state IN ('paused', 'failed')",
                    )
                    .bind(now())
                    .bind(id)
                    .execute(&self.db)
                    .await?;
                    self.http_wake.notify_one();
                }
                DownloadState::Queued | DownloadState::Downloading => {}
                _ => return Err(ApiError::conflict("This download has already finished.")),
            },
            DownloadKind::Torrent => match (row.state, row.engine_id.as_deref()) {
                (DownloadState::Paused, Some(engine_id)) => {
                    self.torrentd
                        .resume(engine_id)
                        .await
                        .map_err(|e| engine_error(&e))?;
                    self.set_state_if(id, DownloadState::Downloading, &["paused"])
                        .await?;
                    self.torrent_wake.notify_one();
                }
                (DownloadState::Paused, None) => {
                    sqlx::query(
                        "UPDATE downloads SET state = 'queued', error = ?, updated_at = ? WHERE id = ? AND state = 'paused'",
                    )
                    .bind(MSG_WAITING_FOR_VPN)
                    .bind(now())
                    .bind(id)
                    .execute(&self.db)
                    .await?;
                    self.torrent_wake.notify_one();
                }
                (
                    DownloadState::Queued | DownloadState::Downloading | DownloadState::Seeding,
                    _,
                ) => {}
                _ => {
                    return Err(ApiError::conflict(
                        "This download cannot be resumed. Add it again.",
                    ));
                }
            },
        }
        self.publish_id(id).await
    }

    pub async fn remove(&self, id: &str, delete_files: bool) -> ApiResult<()> {
        let row = self.load_row(id).await?;
        if row.state == DownloadState::Sorting && self.sort_pending(&row).await? {
            return Err(ApiError::conflict(MSG_BEING_SORTED));
        }
        match row.kind {
            DownloadKind::Http => self.stop_worker(id).await,
            DownloadKind::Torrent => {
                if let (Some(engine_id), true) = (row.engine_id.as_deref(), torrent_in_engine(&row))
                {
                    match self.torrentd.remove(engine_id, delete_files).await {
                        Ok(())
                        | Err(TorrentdError::Disabled)
                        | Err(TorrentdError::Api { status: 404, .. }) => {}
                        Err(e) => return Err(engine_error(&e)),
                    }
                }
                let _ = tokio::fs::remove_file(self.pending_path(id)).await;
            }
            DownloadKind::Upload => {}
        }
        // The worker may have finished meanwhile.
        let row = self.load_row(id).await?;
        if row.state == DownloadState::Sorting && self.sort_pending(&row).await? {
            return Err(ApiError::conflict(MSG_BEING_SORTED));
        }
        // Once sorted, the files live in the library and the old path may belong to a
        // newer download: only files the download still owns are deleted.
        let owns_files = !matches!(row.state, DownloadState::Sorted | DownloadState::Cancelled);
        if delete_files
            && owns_files
            && let Some(rel) = row.rel_path.as_deref()
        {
            self.delete_download_files(rel).await;
        }
        let mut gone = row.clone();
        gone.state = DownloadState::Cancelled;
        self.events
            .publish(Event::Download(gone.to_download(Vec::new())));
        sqlx::query("DELETE FROM downloads WHERE id = ?")
            .bind(id)
            .execute(&self.db)
            .await?;
        self.secrets.remove(id);
        if row.kind == DownloadKind::Http
            && row
                .rel_path
                .as_deref()
                .is_some_and(|r| r.ends_with(".part"))
        {
            self.release_name(&row.name);
        }
        tracing::info!(download = %id, delete_files, "download removed");
        Ok(())
    }

    /// True while a sort job for this download (or its path) is queued or running.
    async fn sort_pending(&self, row: &DownloadRow) -> ApiResult<bool> {
        let (pending,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM jobs WHERE kind = 'sort' AND state IN ('queued', 'running')
               AND (json_extract(payload, '$.download_id') = ? OR json_extract(payload, '$.rel_path') = ?))",
        )
        .bind(&row.id)
        .bind(row.rel_path.as_deref())
        .fetch_one(&self.db)
        .await?;
        Ok(pending)
    }

    // --- shared helpers -----------------------------------------------------------

    fn require_torrents(&self) -> ApiResult<()> {
        if self.torrentd.enabled() {
            Ok(())
        } else {
            Err(ApiError::new(
                ErrorCode::VpnUnavailable,
                crate::torrentd_client::disabled_message(&self.config),
            ))
        }
    }

    /// Which proxy a request for `url` uses, per `http_via_vpn`. The VPN proxy only
    /// reaches ports 80 and 443: `when_available` goes direct for other ports and
    /// `always` refuses them. Err is a user-facing message.
    pub(crate) async fn choose_proxy(&self, url: &Url) -> Result<Option<reqwest::Proxy>, String> {
        let setting = self.settings.get().await.http_via_vpn;
        let proxy_reaches = proxy_reaches(url);
        match setting {
            HttpViaVpn::Never => Ok(None),
            HttpViaVpn::WhenAvailable => {
                if self.torrentd.enabled() && proxy_reaches {
                    Ok(self.torrentd.download_proxy().await)
                } else {
                    Ok(None)
                }
            }
            HttpViaVpn::Always => {
                let proxy = if self.torrentd.enabled() {
                    self.torrentd.download_proxy().await
                } else {
                    None
                };
                match proxy {
                    None => Err(MSG_VPN_REQUIRED.to_string()),
                    Some(_) if !proxy_reaches => Err(MSG_VPN_PORTS.to_string()),
                    Some(proxy) => Ok(Some(proxy)),
                }
            }
        }
    }

    pub(crate) async fn load_row(&self, id: &str) -> ApiResult<DownloadRow> {
        sqlx::query_as::<_, DownloadRow>("SELECT * FROM downloads WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.db)
            .await?
            .ok_or_else(not_found)
    }

    async fn item_ids(&self, id: &str) -> ApiResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT item_id FROM download_items WHERE download_id = ? ORDER BY item_id",
        )
        .bind(id)
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().map(|(i,)| i).collect())
    }

    /// Publish the stored state of a download and return it.
    pub(crate) async fn publish_id(&self, id: &str) -> ApiResult<Download> {
        let download = self.get(id).await?;
        self.events.publish(Event::Download(download.clone()));
        Ok(download)
    }

    /// Publish without reading the database (progress ticks).
    pub(crate) fn publish_row(&self, row: &DownloadRow) {
        self.events
            .publish(Event::Download(row.to_download(Vec::new())));
    }

    pub(crate) async fn insert_row(&self, new: NewDownload<'_>) -> ApiResult<String> {
        let id = new_id();
        self.insert_row_as(&id, new).await?;
        Ok(id)
    }

    /// Insert a row with a chosen id.
    pub(crate) async fn insert_row_as(&self, id: &str, new: NewDownload<'_>) -> ApiResult<()> {
        let t = now();
        let progress = match new.total_bytes {
            Some(total) if total > 0 => (new.done_bytes as f64 / total as f64).clamp(0.0, 1.0),
            Some(_) if new.state == DownloadState::Completed => 1.0,
            _ => 0.0,
        };
        let completed_at = (new.state == DownloadState::Completed).then_some(t);
        sqlx::query(
            "INSERT INTO downloads (id, kind, name, source, state, progress, total_bytes, done_bytes, down_bps, up_bps,
                                    error, rel_path, created_by, created_at, updated_at, completed_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(new.kind.as_str())
        .bind(new.name)
        .bind(new.source)
        .bind(new.state.as_str())
        .bind(progress)
        .bind(new.total_bytes)
        .bind(new.done_bytes)
        .bind(new.error)
        .bind(new.rel_path)
        .bind(new.created_by)
        .bind(t)
        .bind(t)
        .bind(completed_at)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Set `state` when the current state is one of `from`. Returns whether it changed.
    pub(crate) async fn set_state_if(
        &self,
        id: &str,
        state: DownloadState,
        from: &[&str],
    ) -> ApiResult<bool> {
        let placeholders = vec!["?"; from.len()].join(", ");
        let sql = format!(
            "UPDATE downloads SET state = ?, updated_at = ? WHERE id = ? AND state IN ({placeholders})"
        );
        let mut query = sqlx::query(&sql).bind(state.as_str()).bind(now()).bind(id);
        for s in from {
            query = query.bind(*s);
        }
        Ok(query.execute(&self.db).await?.rows_affected() > 0)
    }

    /// Mark a download failed with a user-facing message and publish it. Never fails.
    pub(crate) async fn fail(&self, id: &str, message: &str) {
        let result = sqlx::query(
            "UPDATE downloads SET state = 'failed', error = ?, down_bps = 0, up_bps = 0, eta_s = NULL, updated_at = ?
             WHERE id = ?",
        )
        .bind(message)
        .bind(now())
        .bind(id)
        .execute(&self.db)
        .await;
        match result {
            Ok(_) => {
                let _ = self.publish_id(id).await;
            }
            Err(e) => {
                tracing::warn!(error = %e, download = %id, "could not record a failed download")
            }
        }
    }

    /// The download's files are in `Completed/` at `rel`: mark it completed, queue the
    /// sort job, then mark it sorting.
    pub(crate) async fn complete_and_sort(
        &self,
        id: &str,
        rel: &str,
        total: Option<i64>,
    ) -> ApiResult<()> {
        let t = now();
        sqlx::query(
            "UPDATE downloads SET state = 'completed', rel_path = ?, completed_at = COALESCE(completed_at, ?),
                    progress = 1.0, total_bytes = COALESCE(?, total_bytes), done_bytes = COALESCE(?, total_bytes, done_bytes),
                    down_bps = 0, up_bps = 0, peers = NULL, eta_s = NULL, error = NULL, updated_at = ?
             WHERE id = ?",
        )
        .bind(rel)
        .bind(t)
        .bind(total)
        .bind(total)
        .bind(t)
        .bind(id)
        .execute(&self.db)
        .await?;
        self.publish_id(id).await?;
        if let Err(e) = self.library.enqueue_sort(rel, Some(id)).await {
            // The file waits in Completed; the next rescan sorts it.
            tracing::warn!(error = %e, download = %id, "could not queue the sort job");
            return Ok(());
        }
        self.set_state_if(id, DownloadState::Sorting, &["completed"])
            .await?;
        self.publish_id(id).await?;
        Ok(())
    }

    /// Move a file or folder into `Completed/` as `name` (a single component), or a free
    /// variant of it. Never overwrites. Returns the absolute and relative destination.
    pub(crate) async fn move_to_completed(
        &self,
        src: &Path,
        name: &str,
    ) -> stash_core::Result<(PathBuf, String)> {
        let name = paths::sanitize_filename(name);
        let completed = self.layout.completed();
        let mut last = None;
        for _ in 0..8 {
            let dest = paths::unique_path(&completed.join(&name));
            match paths::move_path(src, &dest).await {
                Ok(()) => {
                    let rel = self.layout.rel(&dest).ok_or_else(|| {
                        stash_core::CoreError::invalid("destination outside the stash")
                    })?;
                    return Ok((dest, rel));
                }
                // Someone took the name between the check and the move; pick another.
                Err(e @ stash_core::CoreError::Conflict(_)) => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| stash_core::CoreError::conflict("no free name in Completed")))
    }

    /// Claim a free file name in `In Progress` (checking files, `.part` files and other
    /// claims). Release it with [`Self::release_name`] once the file has left.
    pub(crate) fn reserve_name(&self, wanted: &str) -> String {
        let dir = self.layout.in_progress();
        let (stem, ext) = match wanted.rfind('.') {
            Some(i) if i > 0 => (&wanted[..i], &wanted[i..]),
            _ => (wanted, ""),
        };
        let mut reserved = lock(&self.reserved);
        let taken = |name: &str| {
            reserved.contains(name)
                || std::fs::symlink_metadata(dir.join(name)).is_ok()
                || std::fs::symlink_metadata(dir.join(format!("{name}.part"))).is_ok()
        };
        let mut n = 1u32;
        let name = loop {
            let candidate = if n == 1 {
                wanted.to_string()
            } else {
                format!("{stem} ({n}){ext}")
            };
            if !taken(&candidate) {
                break candidate;
            }
            n += 1;
        };
        reserved.insert(name.clone());
        name
    }

    pub(crate) fn release_name(&self, name: &str) {
        lock(&self.reserved).remove(name);
    }

    /// Where a torrent waiting for the engine keeps its add request across restarts.
    pub(crate) fn pending_path(&self, id: &str) -> PathBuf {
        self.config
            .pending_torrents_dir()
            .join(format!("{id}.json"))
    }

    /// Stop a running web download and wait (briefly) for its worker to exit.
    async fn stop_worker(&self, id: &str) {
        if let Some(token) = self.running.get(id) {
            token.cancel();
        }
        for _ in 0..200 {
            if !self.running.contains_key(id) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        tracing::warn!(download = %id, "web download worker did not stop in time");
    }

    /// Delete a download's file or folder. Only paths strictly inside
    /// `Downloads/In Progress` or `Downloads/Completed` are touched, never the library.
    async fn delete_download_files(&self, rel: &str) {
        if !is_deletable_download_path(rel) {
            tracing::warn!(
                rel,
                "refusing to delete a path outside the download folders"
            );
            return;
        }
        let Ok(abs) = self.layout.abs(rel) else {
            return;
        };
        let result = match tokio::fs::symlink_metadata(&abs).await {
            Ok(meta) if meta.is_dir() => tokio::fs::remove_dir_all(&abs).await,
            Ok(_) => tokio::fs::remove_file(&abs).await,
            Err(_) => Ok(()),
        };
        if let Err(e) = result {
            tracing::warn!(error = %e, rel, "could not delete download files");
        }
    }
}

/// True for paths strictly inside `In Progress` or `Completed` (not the folders
/// themselves, not partial browser uploads).
pub(crate) fn is_deletable_download_path(rel: &str) -> bool {
    let rel = rel.trim_matches('/');
    if rel
        .split('/')
        .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return false;
    }
    let inside =
        |prefix: &str| rel.len() > prefix.len() + 1 && rel.starts_with(&format!("{prefix}/"));
    let uploads = [paths::UPLOADS_TMP, paths::LEGACY_UPLOADS_TMP]
        .iter()
        .any(|dir| rel == *dir || rel.starts_with(&format!("{dir}/")));
    (inside(paths::IN_PROGRESS) || inside(paths::COMPLETED)) && !uploads
}

/// Map an engine failure to an API error.
pub(crate) fn engine_error(e: &TorrentdError) -> ApiError {
    e.api_error()
}

/// The VPN download proxy only connects to ports 80 and 443.
pub(crate) fn proxy_reaches(url: &Url) -> bool {
    matches!(url.port_or_known_default(), Some(80 | 443))
}

/// The engine may still hold this torrent: anything not yet handed to the sorter,
/// including a completed torrent whose files have not left `In Progress`.
fn torrent_in_engine(row: &DownloadRow) -> bool {
    match row.state {
        DownloadState::Sorting | DownloadState::Sorted => false,
        DownloadState::Completed => row
            .rel_path
            .as_deref()
            .is_some_and(|rel| StashLayout::zone(rel) == Zone::InProgress),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletable_paths() {
        assert!(is_deletable_download_path(
            "Downloads/In Progress/a.mkv.part"
        ));
        assert!(is_deletable_download_path("Downloads/Completed/Show S01"));
        assert!(!is_deletable_download_path("Downloads/Completed"));
        assert!(!is_deletable_download_path("Downloads/In Progress"));
        assert!(!is_deletable_download_path(
            "Downloads/In Progress/.uploads/x.part"
        ));
        assert!(!is_deletable_download_path(
            "Movies/Heat (1995)/Heat (1995).mkv"
        ));
        assert!(!is_deletable_download_path(
            "Downloads/Completed/../../Movies"
        ));
        assert!(!is_deletable_download_path("Downloads/Completedx/a"));
    }

    #[test]
    fn headers_debug_hides_values() {
        let headers = ForwardedHeaders {
            cookie: Some("sid=SECRET".into()),
            referer: None,
            user_agent: None,
        };
        let shown = format!("{headers:?}");
        assert!(!shown.contains("SECRET"));
        let request = AddDownloadRequest {
            url: "https://example.com/a.mkv?token=SECRET".into(),
            filename: None,
            headers: Some(headers),
        };
        assert!(!format!("{request:?}").contains("SECRET"));
    }
}
