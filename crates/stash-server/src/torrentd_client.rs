//! Client for the torrent engine's internal API in the VPN container (section 9).

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use stash_core::config::Config;
use stash_core::events::{Event, EventBus};
use stash_core::internal_api::{
    AddTorrentRequest, AddTorrentResponse, EngineHealth, EngineRunState, EngineTorrent,
    LimitsRequest, MSG_NOT_PROTECTED, TunnelState, TunnelStatus, VpnMeta,
};
use stash_core::models::{VpnState, VpnStatus};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::error::{ApiError, ErrorCode};

/// Request timeout for every engine call.
const TIMEOUT: Duration = Duration::from_secs(5);
/// `vpn_status()` answers from cache for this long.
const STATUS_TTL: Duration = Duration::from_secs(2);
const POLL_SLOW: Duration = Duration::from_secs(5);
const POLL_FAST: Duration = Duration::from_secs(1);
/// After a reload the watcher polls quickly for this long.
const FAST_WINDOW: Duration = Duration::from_secs(30);

pub const MSG_DISABLED: &str = "Torrents are turned off on this server.";

/// Why torrents are off: the supervisor's reason when it gave one, else [`MSG_DISABLED`].
pub fn disabled_message(config: &Config) -> &str {
    config
        .torrents_off_reason
        .as_deref()
        .unwrap_or(MSG_DISABLED)
}
pub const MSG_UNREACHABLE: &str = "The torrent container is not running or not reachable.";
const MSG_TOKEN_REJECTED: &str =
    "The torrent engine did not accept the internal token. Restart the PopcornStash container.";
const MSG_ENGINE_ERROR: &str = "The torrent container answered with an error. Check its logs.";
const MSG_BAD_RESPONSE: &str = "The torrent container sent an answer PopcornStash could not read.";
const MSG_BLOCKED_DEFAULT: &str = "Waiting for the VPN to connect.";
const MSG_ENGINE_STARTING: &str = "The VPN is connected. The torrent engine is starting.";
/// Largest answer read from the engine. It describes torrents and VPN state, never
/// files, so even thousands of torrents fit easily.
const MAX_ENGINE_RESPONSE: usize = 8 * 1024 * 1024;
/// Longest engine message passed on to people (for example why torrents are blocked).
const MAX_ENGINE_MESSAGE: usize = 300;
/// A supervisor `status.json` older than this is ignored (it is rewritten every 5 s).
const SUPERVISOR_STATUS_MAX_AGE_S: i64 = 20;

#[derive(Debug, thiserror::Error)]
pub enum TorrentdError {
    /// `STASH_TORRENTD_URL` is empty.
    #[error("Torrents are turned off on this server.")]
    Disabled,
    #[error("The torrent container is not reachable: {0}")]
    Unreachable(String),
    /// The engine refuses torrents because the VPN is not connected.
    #[error("{0}")]
    Blocked(String),
    #[error("Torrent engine error ({status}): {message}")]
    Api { status: u16, message: String },
}

impl TorrentdError {
    /// The API error to show a user for this failure. `Disabled`, `Unreachable` and
    /// `Blocked` become `vpn_unavailable` (503); engine errors are logged and become `internal`.
    pub fn api_error(&self) -> ApiError {
        match self {
            TorrentdError::Disabled => ApiError::new(ErrorCode::VpnUnavailable, MSG_DISABLED),
            TorrentdError::Unreachable(_) => {
                ApiError::new(ErrorCode::VpnUnavailable, MSG_UNREACHABLE)
            }
            TorrentdError::Blocked(message) => {
                ApiError::new(ErrorCode::VpnUnavailable, message.clone())
            }
            TorrentdError::Api { status, message } => {
                tracing::warn!(status, message = %message, "torrent engine error");
                if *status == 401 {
                    ApiError::new(ErrorCode::Internal, MSG_TOKEN_REJECTED)
                } else {
                    ApiError::new(ErrorCode::Internal, MSG_ENGINE_ERROR)
                }
            }
        }
    }
}

struct CachedStatus {
    at: Instant,
    status: VpnStatus,
}

pub struct TorrentdClient {
    pub config: Arc<Config>,
    /// No proxy, 5 s timeout. `None` only if the TLS backend failed to initialise.
    http: Option<reqwest::Client>,
    token: Mutex<Option<String>>,
    status: Mutex<Option<CachedStatus>>,
    /// Last time the VPN configuration changed or a reload was requested.
    changed_at: Mutex<Option<Instant>>,
    /// Wakes the status watcher early.
    wake: Notify,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl TorrentdClient {
    pub fn new(config: Arc<Config>) -> Self {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(TIMEOUT)
            .connect_timeout(TIMEOUT)
            .user_agent(concat!("PopcornStash/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(
                |e| tracing::error!(error = %e, "could not build the torrent engine HTTP client"),
            )
            .ok();
        Self {
            config,
            http,
            token: Mutex::new(None),
            status: Mutex::new(None),
            changed_at: Mutex::new(None),
            wake: Notify::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.torrentd_url.is_some()
    }

    /// Create `<config>/torrent/` and `vpn/` and return the internal token. Under the
    /// supervisor the token comes from `STASH_INTERNAL_TOKEN` and is never written to disk
    /// (section 7); only a hand-started development setup uses the token file (0640).
    pub async fn ensure_internal_token(config: &Config) -> anyhow::Result<String> {
        let torrent_dir = config.torrent_dir();
        let vpn_dir = config.vpn_dir();
        let engine_dir = torrent_dir.join(stash_core::internal_api::ENGINE_DIR);
        for dir in [&torrent_dir, &vpn_dir, &engine_dir] {
            tokio::fs::create_dir_all(dir).await?;
        }
        // The engine (primary group PGID) must traverse torrent/ to reach engine/, but has
        // no business listing the VPN files; the root supervisor reads them regardless.
        set_mode(&torrent_dir, 0o750).await;
        set_mode(&vpn_dir, 0o700).await;

        if let Some(token) = env_token(config) {
            return Ok(token);
        }
        let path = config.internal_token_path();
        match tokio::fs::read_to_string(&path).await {
            Ok(existing) if !existing.trim().is_empty() => {
                set_mode(&path, 0o640).await;
                return Ok(existing.trim().to_string());
            }
            Ok(_) => tracing::warn!("the internal token file is empty; creating a new token"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(anyhow::anyhow!("could not read {}: {e}", path.display())),
        }

        let token = hex::encode(rand::random::<[u8; 32]>());
        write_private_file(&path, format!("{token}\n").as_bytes(), 0o640).await?;
        tracing::info!(path = %path.display(), "created the internal torrent engine token");
        Ok(token)
    }

    pub async fn health(&self) -> Result<EngineHealth, TorrentdError> {
        let response = self.request(Method::GET, &["health"], &[], None).await?;
        decode(response).await
    }

    pub async fn list(&self) -> Result<Vec<EngineTorrent>, TorrentdError> {
        let response = self.request(Method::GET, &["torrents"], &[], None).await?;
        decode(response).await
    }

    pub async fn add(
        &self,
        request: &AddTorrentRequest,
    ) -> Result<AddTorrentResponse, TorrentdError> {
        let body = serde_json::to_value(request).map_err(|e| TorrentdError::Api {
            status: 0,
            message: e.to_string(),
        })?;
        let response = self
            .request(Method::POST, &["torrents"], &[], Some(&body))
            .await?;
        decode(response).await
    }

    pub async fn pause(&self, id: &str) -> Result<(), TorrentdError> {
        self.request(Method::POST, &["torrents", id, "pause"], &[], None)
            .await
            .map(drop)
    }

    pub async fn resume(&self, id: &str) -> Result<(), TorrentdError> {
        self.request(Method::POST, &["torrents", id, "resume"], &[], None)
            .await
            .map(drop)
    }

    pub async fn remove(&self, id: &str, delete_files: bool) -> Result<(), TorrentdError> {
        let flag = if delete_files { "true" } else { "false" };
        self.request(
            Method::DELETE,
            &["torrents", id],
            &[("delete_files", flag)],
            None,
        )
        .await
        .map(drop)
    }

    pub async fn set_limits(&self, limits: LimitsRequest) -> Result<(), TorrentdError> {
        let body = serde_json::to_value(limits).map_err(|e| TorrentdError::Api {
            status: 0,
            message: e.to_string(),
        })?;
        self.request(Method::PUT, &["limits"], &[], Some(&body))
            .await
            .map(drop)
    }

    pub async fn reload_vpn(&self) -> Result<(), TorrentdError> {
        self.note_vpn_changed();
        let result = self
            .request(Method::POST, &["vpn", "reload"], &[], None)
            .await
            .map(drop);
        // The engine restarts now; make the next status read and the watcher look again.
        self.note_vpn_changed();
        result
    }

    /// Forget the cached status and poll quickly for a while. Call after changing
    /// files in the VPN directory.
    pub fn note_vpn_changed(&self) {
        *lock(&self.status) = None;
        *lock(&self.changed_at) = Some(Instant::now());
        self.wake.notify_one();
    }

    /// UI status: combines `vpn/vpn.json` with the engine's health.
    pub async fn vpn_status(&self) -> VpnStatus {
        if let Some(cached) = lock(&self.status).as_ref()
            && cached.at.elapsed() < STATUS_TTL
        {
            return cached.status.clone();
        }
        self.refresh_status().await
    }

    /// Fetch a fresh status and update the cache.
    async fn refresh_status(&self) -> VpnStatus {
        let meta = read_vpn_meta(&self.config).await;
        let health = self.health().await;
        let status = match health {
            // The engine only runs while the tunnel is up; while the supervisor is still
            // connecting or retrying, its status file says what is going on.
            Err(TorrentdError::Unreachable(_)) => {
                match read_supervisor_status(&self.config).await {
                    Some(tunnel) => status_from_supervisor(meta.as_ref(), tunnel),
                    None => combine_status(meta.as_ref(), health),
                }
            }
            health => combine_status(meta.as_ref(), health),
        };
        let mut status = status;
        if status.error.as_deref() == Some(MSG_DISABLED) {
            status.error = Some(disabled_message(&self.config).to_string());
        }
        *lock(&self.status) = Some(CachedStatus {
            at: Instant::now(),
            status: status.clone(),
        });
        status
    }

    /// Poll engine health and publish `Event::Vpn` whenever the UI-visible status changes.
    pub fn spawn_status_watch(self: &Arc<Self>, events: EventBus, cancel: CancellationToken) {
        let client = self.clone();
        tokio::spawn(async move {
            let mut last: Option<VpnStatus> = None;
            loop {
                let status = client.refresh_status().await;
                let comparable = without_timestamps(&status);
                if last.as_ref() != Some(&comparable) {
                    tracing::info!(state = ?status.state, reachable = status.reachable, "VPN status changed");
                    events.publish(Event::Vpn(status));
                    last = Some(comparable);
                }
                let wait = client.poll_interval();
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(wait) => {}
                    _ = client.wake.notified() => {}
                }
            }
            tracing::debug!("VPN status watch stopped");
        });
    }

    fn poll_interval(&self) -> Duration {
        match *lock(&self.changed_at) {
            Some(at) if at.elapsed() < FAST_WINDOW => POLL_FAST,
            _ => POLL_SLOW,
        }
    }

    /// reqwest proxy for web downloads through the VPN (CONNECT proxy with basic auth), when configured.
    pub async fn download_proxy(&self) -> Option<reqwest::Proxy> {
        let proxy_url = self.config.torrentd_proxy.as_ref()?;
        if !self.enabled() {
            return None;
        }
        let status = self.vpn_status().await;
        let usable =
            status.reachable && (status.state == VpnState::Connected || status.allow_no_vpn);
        if !usable {
            return None;
        }
        let token = self.token().await.ok()?;
        match reqwest::Proxy::all(proxy_url.as_str()) {
            Ok(proxy) => Some(proxy.basic_auth("stash", &token)),
            Err(e) => {
                tracing::warn!(error = %e, "STASH_TORRENTD_PROXY is not usable as a proxy");
                None
            }
        }
    }

    // --- internals ------------------------------------------------------------

    async fn token(&self) -> Result<String, TorrentdError> {
        if let Some(token) = env_token(&self.config) {
            return Ok(token);
        }
        if let Some(token) = lock(&self.token).clone() {
            return Ok(token);
        }
        let path = self.config.internal_token_path();
        let token = match tokio::fs::read_to_string(&path).await {
            Ok(text) if !text.trim().is_empty() => text.trim().to_string(),
            _ => Self::ensure_internal_token(&self.config)
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, "could not read or create the internal token");
                    TorrentdError::Unreachable("the internal token could not be read".into())
                })?,
        };
        *lock(&self.token) = Some(token.clone());
        Ok(token)
    }

    fn forget_token(&self) {
        *lock(&self.token) = None;
    }

    fn url(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<Url, TorrentdError> {
        let base = self
            .config
            .torrentd_url
            .as_ref()
            .ok_or(TorrentdError::Disabled)?;
        let mut url = base.clone();
        {
            let mut path = url.path_segments_mut().map_err(|_| {
                TorrentdError::Unreachable("STASH_TORRENTD_URL cannot have paths".into())
            })?;
            path.pop_if_empty();
            path.extend(segments);
        }
        url.set_query(None);
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        Ok(url)
    }

    /// Send an authenticated request. A 401 re-reads the token once and retries.
    async fn request(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, &str)],
        body: Option<&serde_json::Value>,
    ) -> Result<reqwest::Response, TorrentdError> {
        let url = self.url(segments, query)?;
        let http = self
            .http
            .as_ref()
            .ok_or_else(|| TorrentdError::Unreachable("the HTTP client is unavailable".into()))?;
        let mut retried = false;
        loop {
            let token = self.token().await?;
            let mut request = http
                .request(method.clone(), url.clone())
                .bearer_auth(&token);
            if let Some(body) = body {
                request = request.json(body);
            }
            let response = request.send().await.map_err(transport_error)?;
            let status = response.status();
            if status == StatusCode::UNAUTHORIZED && !retried {
                retried = true;
                self.forget_token();
                continue;
            }
            if status.is_success() {
                return Ok(response);
            }
            return Err(error_from_response(response).await);
        }
    }
}

/// The token the supervisor passed in `STASH_INTERNAL_TOKEN`, if any.
fn env_token(config: &Config) -> Option<String> {
    config
        .internal_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// Read `vpn/vpn.json`. Missing or unreadable files mean "not configured".
pub async fn read_vpn_meta(config: &Config) -> Option<VpnMeta> {
    let path = config
        .vpn_dir()
        .join(stash_core::internal_api::VPN_META_FILE);
    match tokio::fs::read(&path).await {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(meta) => Some(meta),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "vpn.json is not valid; treating the VPN as not configured");
                None
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "could not read vpn.json");
            None
        }
    }
}

/// The supervisor's `status.json` when it was written in the last 20 seconds. It lives in
/// the supervisor's run directory (`/run/popcornstash`, next to its socket); without a
/// supervisor there is no status file.
pub async fn read_supervisor_status(config: &Config) -> Option<TunnelStatus> {
    let path = config
        .supervisor_socket
        .as_deref()?
        .parent()?
        .join(stash_core::internal_api::STATUS_FILE);
    let bytes = tokio::fs::read(&path).await.ok()?;
    let status: TunnelStatus = match serde_json::from_slice(&bytes) {
        Ok(status) => status,
        Err(e) => {
            tracing::debug!(error = %e, "status.json is not valid");
            return None;
        }
    };
    let age = stash_core::now().saturating_sub(status.updated_at);
    (-SUPERVISOR_STATUS_MAX_AGE_S..=SUPERVISOR_STATUS_MAX_AGE_S)
        .contains(&age)
        .then_some(status)
}

/// A saved VPN that the supervisor has not applied yet is connecting, not missing:
/// right after a VPN is set up the engine restarts to apply it, and until then the
/// supervisor and the engine still report the state from before the change.
fn with_saved_vpn(configured: bool, state: VpnState) -> VpnState {
    if configured && state == VpnState::NotConfigured {
        VpnState::Connecting
    } else {
        state
    }
}

/// The state shown for a tunnel state. With STASH_ALLOW_NO_VPN no tunnel is ever used,
/// whatever `vpn.json` holds, so the state is always "not configured" and the UI shows
/// its permanent no-VPN warning instead of "connecting".
fn shown_state(configured: bool, tunnel: TunnelState, allow_no_vpn: bool) -> VpnState {
    if allow_no_vpn {
        VpnState::NotConfigured
    } else {
        with_saved_vpn(configured, vpn_state(tunnel))
    }
}

/// UI status while the engine is not answering but the supervisor is alive.
pub fn status_from_supervisor(meta: Option<&VpnMeta>, tunnel: TunnelStatus) -> VpnStatus {
    // Only STASH_ALLOW_NO_VPN turns the supervisor's kill switch off (it reports it on in
    // every other state), so this tells the mode apart while the engine restarts.
    let allow_no_vpn = !tunnel.kill_switch;
    let state = shown_state(meta.is_some(), tunnel.state, allow_no_vpn);
    let error = match (tunnel.error, state) {
        (Some(error), _) => Some(error),
        (None, VpnState::Connected) => Some(MSG_ENGINE_STARTING.to_string()),
        (None, _) => None,
    };
    VpnStatus {
        configured: meta.is_some(),
        reachable: false,
        state,
        provider: meta.map(|m| m.provider).or(tunnel.provider),
        protocol: meta.map(|m| m.protocol).or(tunnel.protocol),
        label: meta.and_then(|m| m.label.clone()).or(tunnel.label),
        server: meta.and_then(|m| m.server.clone()).or(tunnel.server),
        country: meta.and_then(|m| m.country.clone()).or(tunnel.country),
        public_ip: None,
        protected: None,
        since: tunnel.since,
        error,
        kill_switch: tunnel.kill_switch,
        allow_no_vpn,
        torrents_off: false,
    }
}

fn vpn_state(state: TunnelState) -> VpnState {
    match state {
        TunnelState::Connected => VpnState::Connected,
        TunnelState::Connecting => VpnState::Connecting,
        TunnelState::Disconnected => VpnState::Disconnected,
        TunnelState::Error => VpnState::Error,
        TunnelState::NotConfigured => VpnState::NotConfigured,
    }
}

/// The engine stopped torrents because the public address check found an address that
/// is not a VPN address (the VPN status's `protected: false`, "VPN not protecting").
pub fn blocked_not_protected(health: &EngineHealth) -> bool {
    !health.engine.allow_no_vpn
        && health.engine.state == EngineRunState::Blocked
        && health.engine.reason.as_deref() == Some(MSG_NOT_PROTECTED)
}

/// Build the UI status from the saved VPN choice and the engine's health answer.
pub fn combine_status(
    meta: Option<&VpnMeta>,
    health: Result<EngineHealth, TorrentdError>,
) -> VpnStatus {
    let configured = meta.is_some();
    let with_meta = |mut status: VpnStatus| {
        if let Some(meta) = meta {
            status.provider = Some(meta.provider);
            status.protocol = Some(meta.protocol);
            status.label = meta.label.clone();
            status.server = meta.server.clone();
            status.country = meta.country.clone();
        }
        status
    };
    match health {
        Ok(health) => {
            // Torrents are stopped because the address is not a VPN address: never show
            // the connection as protected, whatever the engine's last answer field says.
            let protected = if blocked_not_protected(&health) {
                Some(false)
            } else {
                health.protected
            };
            let tunnel = health.vpn;
            let allow_no_vpn = health.engine.allow_no_vpn;
            let state = shown_state(configured, tunnel.state, allow_no_vpn);
            VpnStatus {
                configured,
                reachable: true,
                state,
                provider: meta.map(|m| m.provider).or(tunnel.provider),
                protocol: meta.map(|m| m.protocol).or(tunnel.protocol),
                label: meta.and_then(|m| m.label.clone()).or(tunnel.label),
                server: meta.and_then(|m| m.server.clone()).or(tunnel.server),
                country: meta
                    .and_then(|m| m.country.clone())
                    .or(tunnel.country)
                    .or(health.country),
                public_ip: health.public_ip,
                protected,
                since: tunnel.since,
                // No tunnel is used in this mode, so a tunnel problem is not one.
                error: tunnel.error.filter(|_| !allow_no_vpn),
                kill_switch: tunnel.kill_switch,
                allow_no_vpn,
                torrents_off: false,
            }
        }
        Err(TorrentdError::Disabled) => {
            let mut status = with_meta(VpnStatus::unreachable(configured, MSG_DISABLED));
            status.torrents_off = true;
            status
        }
        Err(TorrentdError::Unreachable(_)) => {
            with_meta(VpnStatus::unreachable(configured, MSG_UNREACHABLE))
        }
        Err(TorrentdError::Api { status, message }) => {
            tracing::warn!(status, message = %message, "torrent engine health check failed");
            let error = if status == 401 {
                MSG_TOKEN_REJECTED
            } else {
                MSG_ENGINE_ERROR
            };
            let mut status = with_meta(VpnStatus::unreachable(configured, error));
            status.reachable = true;
            status.state = VpnState::Error;
            status
        }
        Err(TorrentdError::Blocked(message)) => {
            let mut status = with_meta(VpnStatus::unreachable(configured, message));
            status.reachable = true;
            status.state = VpnState::Disconnected;
            status
        }
    }
}

fn without_timestamps(status: &VpnStatus) -> VpnStatus {
    VpnStatus {
        since: None,
        ..status.clone()
    }
}

fn transport_error(error: reqwest::Error) -> TorrentdError {
    let error = error.without_url();
    tracing::debug!(error = %error, "torrent engine request failed");
    TorrentdError::Unreachable(error.to_string())
}

#[derive(Deserialize)]
struct ErrorBody {
    error: Option<ErrorField>,
    code: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ErrorField {
    Object {
        code: Option<String>,
        message: Option<String>,
    },
    Text(String),
}

async fn error_from_response(response: reqwest::Response) -> TorrentdError {
    let status = response.status().as_u16();
    let text = match read_capped(response).await {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => return e,
    };
    let (code, message) = match serde_json::from_str::<ErrorBody>(&text) {
        Ok(ErrorBody {
            error: Some(ErrorField::Object { code, message }),
            ..
        }) => (code, message),
        Ok(ErrorBody {
            error: Some(ErrorField::Text(message)),
            code,
            ..
        }) => (code, Some(message)),
        Ok(ErrorBody {
            error: None,
            code,
            message,
        }) => (code, message),
        Err(_) => (
            None,
            Some(text.trim().chars().take(300).collect::<String>()).filter(|m| !m.is_empty()),
        ),
    };
    let message = message
        .map(|m| engine_message(&m))
        .filter(|m| !m.is_empty());
    if status == 409 && code.as_deref() == Some("vpn_unavailable") {
        return TorrentdError::Blocked(message.unwrap_or_else(|| MSG_BLOCKED_DEFAULT.to_string()));
    }
    TorrentdError::Api {
        status,
        message: message.unwrap_or_else(|| format!("HTTP {status}")),
    }
}

/// The body of an engine answer, refused past [`MAX_ENGINE_RESPONSE`] bytes.
async fn read_capped(mut response: reqwest::Response) -> Result<Vec<u8>, TorrentdError> {
    let status = response.status().as_u16();
    let too_large = || {
        tracing::warn!("the torrent engine sent an answer larger than PopcornStash accepts");
        TorrentdError::Api {
            status,
            message: MSG_BAD_RESPONSE.to_string(),
        }
    };
    if response
        .content_length()
        .is_some_and(|len| len > MAX_ENGINE_RESPONSE as u64)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        if body.len() + chunk.len() > MAX_ENGINE_RESPONSE {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// An engine message shown to people: one line, at most [`MAX_ENGINE_MESSAGE`] characters.
fn engine_message(text: &str) -> String {
    let line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let line = line.trim();
    if line.chars().count() <= MAX_ENGINE_MESSAGE {
        return line.to_string();
    }
    let mut short: String = line.chars().take(MAX_ENGINE_MESSAGE - 1).collect();
    short.push('…');
    short
}

async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, TorrentdError> {
    let status = response.status().as_u16();
    let bytes = read_capped(response).await?;
    serde_json::from_slice(&bytes).map_err(|e| {
        tracing::warn!(error = %e, "torrent engine response did not match the internal API");
        TorrentdError::Api {
            status,
            message: MSG_BAD_RESPONSE.to_string(),
        }
    })
}

async fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).await {
        tracing::warn!(path = %path.display(), error = %e, "could not set file permissions");
    }
}

/// Write `bytes` to `path` atomically (temp file in the same directory, then rename)
/// with the given mode. Used for the internal token and the VPN files.
pub async fn write_private_file(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::AsyncWriteExt;

    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("path has no parent directory"))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(
        ".{name}.{}.tmp",
        hex::encode(rand::random::<[u8; 6]>())
    ));
    let result = async {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)
            .await?;
        // The mode passed to open() is filtered by the umask; set it exactly.
        file.set_permissions(std::fs::Permissions::from_mode(mode))
            .await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&tmp, path).await
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::extract::{Path as AxumPath, Query, State};
    use axum::http::{HeaderMap, StatusCode as AxumStatus};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{delete, get, post, put};
    use axum::{Json, Router};
    use stash_core::internal_api::{
        EngineRunState, EngineState, EngineTorrentState, TunnelStatus, VpnProtocol, VpnProvider,
    };

    use super::*;

    #[derive(Clone)]
    struct Fake {
        token: Arc<Mutex<String>>,
        health_hits: Arc<AtomicUsize>,
        reloads: Arc<AtomicUsize>,
        limits: Arc<Mutex<Option<LimitsRequest>>>,
        removed: Arc<Mutex<Option<(String, String)>>>,
        tunnel: Arc<Mutex<TunnelState>>,
    }

    impl Fake {
        fn new(token: &str) -> Self {
            Self {
                token: Arc::new(Mutex::new(token.to_string())),
                health_hits: Arc::default(),
                reloads: Arc::default(),
                limits: Arc::default(),
                removed: Arc::default(),
                tunnel: Arc::new(Mutex::new(TunnelState::Connected)),
            }
        }

        fn authorized(&self, headers: &HeaderMap) -> bool {
            let expected = format!("Bearer {}", lock(&self.token));
            headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(expected.as_str())
        }
    }

    fn unauthorized() -> Response {
        (
            AxumStatus::UNAUTHORIZED,
            Json(serde_json::json!({"error": {"code": "unauthorized", "message": "no"}})),
        )
            .into_response()
    }

    fn sample_health(state: TunnelState) -> EngineHealth {
        EngineHealth {
            vpn: TunnelStatus {
                state,
                provider: Some(VpnProvider::Nordvpn),
                protocol: Some(VpnProtocol::Wireguard),
                label: Some("tunnel label".into()),
                server: Some("nl812.nordvpn.com".into()),
                country: Some("Netherlands".into()),
                interface: Some("psw0".into()),
                since: Some(1_700_000_000),
                error: None,
                kill_switch: true,
                updated_at: 1_700_000_100,
            },
            engine: EngineState {
                state: EngineRunState::Running,
                reason: None,
                allow_no_vpn: false,
            },
            public_ip: Some("185.153.177.12".into()),
            country: Some("Netherlands".into()),
            protected: Some(true),
            checked_at: Some(1_700_000_050),
        }
    }

    async fn start_fake(fake: Fake) -> Url {
        let app = Router::new()
            .route("/livez", get(|| async { "ok" }))
            .route(
                "/health",
                get(|State(f): State<Fake>, headers: HeaderMap| async move {
                    if !f.authorized(&headers) {
                        return unauthorized();
                    }
                    f.health_hits.fetch_add(1, Ordering::SeqCst);
                    Json(sample_health(*lock(&f.tunnel))).into_response()
                }),
            )
            .route(
                "/torrents",
                get(|State(f): State<Fake>, headers: HeaderMap| async move {
                    if !f.authorized(&headers) {
                        return unauthorized();
                    }
                    Json(vec![EngineTorrent {
                        id: "7".into(),
                        info_hash: "aa".repeat(20),
                        name: Some("Big Buck Bunny".into()),
                        state: EngineTorrentState::Downloading,
                        progress: 0.5,
                        total_bytes: Some(1000),
                        done_bytes: 500,
                        uploaded_bytes: 0,
                        down_bps: 100,
                        up_bps: 0,
                        peers: Some(3),
                        eta_s: Some(5),
                        error: None,
                        output_dir: Some("/stash/Downloads/In Progress/Big Buck Bunny".into()),
                    }])
                    .into_response()
                })
                .post(|State(f): State<Fake>, headers: HeaderMap, Json(req): Json<AddTorrentRequest>| async move {
                    if !f.authorized(&headers) {
                        return unauthorized();
                    }
                    if *lock(&f.tunnel) != TunnelState::Connected {
                        return (
                            AxumStatus::CONFLICT,
                            Json(serde_json::json!({"error": {"code": "vpn_unavailable", "message": "Waiting for the VPN to connect."}})),
                        )
                            .into_response();
                    }
                    if req.magnet.as_deref() == Some("magnet:?bad") {
                        return (
                            AxumStatus::BAD_REQUEST,
                            Json(serde_json::json!({"error": {"code": "bad_request", "message": "Not a magnet link."}})),
                        )
                            .into_response();
                    }
                    Json(AddTorrentResponse { id: "8".into(), info_hash: "bb".repeat(20), name: None }).into_response()
                }),
            )
            .route(
                "/torrents/{id}/pause",
                post(|State(f): State<Fake>, headers: HeaderMap| async move {
                    if !f.authorized(&headers) {
                        return unauthorized();
                    }
                    AxumStatus::NO_CONTENT.into_response()
                }),
            )
            .route(
                "/torrents/{id}",
                delete(
                    |State(f): State<Fake>,
                     headers: HeaderMap,
                     AxumPath(id): AxumPath<String>,
                     Query(q): Query<std::collections::HashMap<String, String>>| async move {
                        if !f.authorized(&headers) {
                            return unauthorized();
                        }
                        *lock(&f.removed) = Some((id, q.get("delete_files").cloned().unwrap_or_default()));
                        AxumStatus::NO_CONTENT.into_response()
                    },
                ),
            )
            .route(
                "/limits",
                put(|State(f): State<Fake>, headers: HeaderMap, Json(limits): Json<LimitsRequest>| async move {
                    if !f.authorized(&headers) {
                        return unauthorized();
                    }
                    *lock(&f.limits) = Some(limits);
                    AxumStatus::NO_CONTENT.into_response()
                }),
            )
            .route(
                "/vpn/reload",
                post(|State(f): State<Fake>, headers: HeaderMap| async move {
                    if !f.authorized(&headers) {
                        return unauthorized();
                    }
                    f.reloads.fetch_add(1, Ordering::SeqCst);
                    AxumStatus::ACCEPTED.into_response()
                }),
            )
            .with_state(fake);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Url::parse(&format!("http://{addr}")).unwrap()
    }

    fn test_config(dir: &Path, torrentd_url: Option<Url>) -> Config {
        let mut config = Config::for_tests(&dir.join("stash"), &dir.join("config"));
        config.torrentd_url = torrentd_url;
        config
    }

    async fn client_for(dir: &Path, url: Option<Url>) -> (Arc<TorrentdClient>, String) {
        let config = test_config(dir, url);
        let token = TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        (Arc::new(TorrentdClient::new(Arc::new(config))), token)
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[tokio::test]
    async fn internal_token_created_once() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path(), None);
        let token = TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        let path = config.internal_token_path();
        assert_eq!(mode(&path), 0o640);
        assert_eq!(mode(&config.torrent_dir()), 0o750);
        assert_eq!(mode(&config.vpn_dir()), 0o700);
        assert!(config.torrent_dir().join("engine").is_dir());
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), token);
        // Second call keeps the token.
        let again = TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        assert_eq!(again, token);
        // No temp files left behind.
        let leftovers: Vec<_> = std::fs::read_dir(config.torrent_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        // A world-readable token is tightened.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        assert_eq!(mode(&path), 0o640);
    }

    #[tokio::test]
    async fn bearer_required_and_methods_work() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path(), None);
        let token = TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        let fake = Fake::new(&token);
        let url = start_fake(fake.clone()).await;
        let (client, _) = client_for(dir.path(), Some(url)).await;

        let health = client.health().await.unwrap();
        assert_eq!(health.vpn.state, TunnelState::Connected);
        let list = client.list().await.unwrap();
        assert_eq!(list[0].name.as_deref(), Some("Big Buck Bunny"));
        let added = client
            .add(&AddTorrentRequest {
                magnet: Some("magnet:?xt=urn:btih:abc".into()),
                torrent_base64: None,
            })
            .await
            .unwrap();
        assert_eq!(added.id, "8");
        client.pause("7").await.unwrap();
        client.remove("7", true).await.unwrap();
        assert_eq!(
            lock(&fake.removed).clone(),
            Some(("7".to_string(), "true".to_string()))
        );
        client
            .set_limits(LimitsRequest {
                down_kbps: 100,
                up_kbps: 50,
            })
            .await
            .unwrap();
        assert_eq!(
            *lock(&fake.limits),
            Some(LimitsRequest {
                down_kbps: 100,
                up_kbps: 50
            })
        );
        client.reload_vpn().await.unwrap();
        assert_eq!(fake.reloads.load(Ordering::SeqCst), 1);

        // Other errors keep the status and message.
        match client
            .add(&AddTorrentRequest {
                magnet: Some("magnet:?bad".into()),
                torrent_base64: None,
            })
            .await
        {
            Err(TorrentdError::Api {
                status: 400,
                message,
            }) => assert_eq!(message, "Not a magnet link."),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn wrong_token_is_rejected_then_reread() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new("engine-has-another-token");
        let url = start_fake(fake.clone()).await;
        let (client, token) = client_for(dir.path(), Some(url)).await;
        match client.health().await {
            Err(TorrentdError::Api { status: 401, .. }) => {}
            other => panic!("unexpected {other:?}"),
        }
        let status = client.vpn_status().await;
        assert_eq!(status.state, VpnState::Error);
        assert!(status.error.unwrap().contains("internal token"));
        // Token rotated on disk (e.g. the appdata was restored): the client re-reads it once on 401.
        *lock(&fake.token) = "rotated-token".into();
        std::fs::write(client.config.internal_token_path(), "rotated-token\n").unwrap();
        assert_ne!(token, "rotated-token");
        assert!(client.health().await.is_ok());
    }

    #[tokio::test]
    async fn blocked_when_vpn_down() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path(), None);
        let token = TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        let fake = Fake::new(&token);
        *lock(&fake.tunnel) = TunnelState::Connecting;
        let url = start_fake(fake).await;
        let (client, _) = client_for(dir.path(), Some(url)).await;
        let err = client
            .add(&AddTorrentRequest {
                magnet: Some("magnet:?xt=urn:btih:abc".into()),
                torrent_base64: None,
            })
            .await
            .unwrap_err();
        match &err {
            TorrentdError::Blocked(message) => {
                assert_eq!(message, "Waiting for the VPN to connect.")
            }
            other => panic!("unexpected {other:?}"),
        }
        let api = err.api_error();
        assert_eq!(api.code, ErrorCode::VpnUnavailable);
        assert_eq!(api.message, "Waiting for the VPN to connect.");
    }

    #[tokio::test]
    async fn unreachable_and_disabled() {
        let dir = tempfile::tempdir().unwrap();
        // Grab a free port, then close it.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let url = Url::parse(&format!("http://127.0.0.1:{port}")).unwrap();
        let (client, _) = client_for(dir.path(), Some(url)).await;
        assert!(matches!(
            client.health().await,
            Err(TorrentdError::Unreachable(_))
        ));
        let status = client.vpn_status().await;
        assert_eq!(status.state, VpnState::Unreachable);
        assert!(!status.reachable && !status.configured);
        assert_eq!(status.error.as_deref(), Some(MSG_UNREACHABLE));
        assert!(client.download_proxy().await.is_none());

        let dir2 = tempfile::tempdir().unwrap();
        let (disabled, _) = client_for(dir2.path(), None).await;
        assert!(!disabled.enabled());
        assert!(matches!(
            disabled.list().await,
            Err(TorrentdError::Disabled)
        ));
        let status = disabled.vpn_status().await;
        assert_eq!(status.state, VpnState::Unreachable);
        assert_eq!(status.error.as_deref(), Some(MSG_DISABLED));
        assert!(status.torrents_off);
        // An engine that is only unreachable is not "off": torrents wait for it.
        assert!(!client.vpn_status().await.torrents_off);
        assert_eq!(
            TorrentdError::Disabled.api_error().code,
            ErrorCode::VpnUnavailable
        );
    }

    #[tokio::test]
    async fn status_mapping_and_cache() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path(), None);
        let token = TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        let fake = Fake::new(&token);
        let url = start_fake(fake.clone()).await;
        let (client, _) = client_for(dir.path(), Some(url)).await;

        // Not configured: engine fields still come through, configured is false.
        let status = client.vpn_status().await;
        assert!(!status.configured && status.reachable);
        assert_eq!(status.state, VpnState::Connected);
        assert_eq!(status.public_ip.as_deref(), Some("185.153.177.12"));
        assert_eq!(status.protected, Some(true));
        assert_eq!(status.since, Some(1_700_000_000));
        assert!(status.kill_switch && !status.allow_no_vpn);
        assert_eq!(fake.health_hits.load(Ordering::SeqCst), 1);

        // Cached for 2 s.
        let _ = client.vpn_status().await;
        assert_eq!(fake.health_hits.load(Ordering::SeqCst), 1);

        // vpn.json wins for the labels; a config change drops the cache.
        let meta = VpnMeta {
            provider: VpnProvider::Wireguard,
            protocol: VpnProtocol::Wireguard,
            label: Some("WireGuard · home".into()),
            server: Some("vpn.example.com".into()),
            country: None,
            created_at: 1,
        };
        std::fs::write(
            client.config.vpn_dir().join("vpn.json"),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();
        client.note_vpn_changed();
        let status = client.vpn_status().await;
        assert_eq!(fake.health_hits.load(Ordering::SeqCst), 2);
        assert!(status.configured);
        assert_eq!(status.provider, Some(VpnProvider::Wireguard));
        assert_eq!(status.label.as_deref(), Some("WireGuard · home"));
        assert_eq!(status.server.as_deref(), Some("vpn.example.com"));
        assert_eq!(status.country.as_deref(), Some("Netherlands"));

        // Tunnel states map one to one, except that a saved VPN the supervisor has not
        // applied yet (it was just set up) is connecting, never "not configured".
        for (tunnel, expected) in [
            (TunnelState::Connecting, VpnState::Connecting),
            (TunnelState::Disconnected, VpnState::Disconnected),
            (TunnelState::Error, VpnState::Error),
            (TunnelState::NotConfigured, VpnState::Connecting),
        ] {
            assert_eq!(
                combine_status(Some(&meta), Ok(sample_health(tunnel))).state,
                expected
            );
        }
        assert_eq!(
            combine_status(None, Ok(sample_health(TunnelState::NotConfigured))).state,
            VpnState::NotConfigured
        );
        let mut before = sample_health(TunnelState::NotConfigured).vpn;
        before.updated_at = stash_core::now();
        assert_eq!(
            status_from_supervisor(Some(&meta), before.clone()).state,
            VpnState::Connecting
        );
        assert_eq!(
            status_from_supervisor(None, before).state,
            VpnState::NotConfigured
        );
        let unreachable = combine_status(Some(&meta), Err(TorrentdError::Unreachable("x".into())));
        assert!(unreachable.configured && !unreachable.reachable);
        assert_eq!(unreachable.label.as_deref(), Some("WireGuard · home"));
    }

    /// An engine stopped because the address is not a VPN address is never shown as
    /// protected, also when its last answer is missing (for example just after the
    /// tunnel came back), so the UI never says "Protected" while torrents are stopped.
    #[test]
    fn an_engine_blocked_for_not_protected_is_never_shown_as_protected() {
        let blocked = |protected: Option<bool>| {
            let mut health = sample_health(TunnelState::Connected);
            health.engine.state = EngineRunState::Blocked;
            health.engine.reason = Some(MSG_NOT_PROTECTED.into());
            health.protected = protected;
            if protected.is_none() {
                health.public_ip = None;
            }
            health
        };
        for protected in [None, Some(true), Some(false)] {
            let status = combine_status(None, Ok(blocked(protected)));
            assert_eq!(status.state, VpnState::Connected, "{protected:?}");
            assert_eq!(status.protected, Some(false), "{protected:?}");
        }
        // Blocked for another reason, or running: the engine's answer is passed on.
        let mut waiting = sample_health(TunnelState::Connected);
        waiting.engine.state = EngineRunState::Blocked;
        waiting.engine.reason = Some("Waiting for the VPN to connect.".into());
        waiting.protected = None;
        assert_eq!(combine_status(None, Ok(waiting)).protected, None);
        let running = combine_status(None, Ok(sample_health(TunnelState::Connected)));
        assert_eq!(running.protected, Some(true));
    }

    /// With STASH_ALLOW_NO_VPN the supervisor never applies a VPN, so a saved one stays
    /// unused and torrents run on this network. The status must say exactly that (no VPN,
    /// allowed) and never "connecting" because a vpn.json is lying around.
    #[test]
    fn allow_no_vpn_with_a_saved_vpn_reports_no_vpn() {
        let meta = VpnMeta {
            provider: VpnProvider::Nordvpn,
            protocol: VpnProtocol::Wireguard,
            label: None,
            server: Some("nl812.nordvpn.com".into()),
            country: Some("Netherlands".into()),
            created_at: 1,
        };
        let no_vpn = |tunnel: TunnelState| {
            let mut health = sample_health(tunnel);
            health.engine.allow_no_vpn = true;
            health.vpn.kill_switch = false;
            health.vpn.interface = None;
            health.protected = None;
            health
        };
        // The supervisor's own status in this mode is "not configured" (kill switch off);
        // an engine that has not read it yet reports its stand-in "disconnected".
        for tunnel in [TunnelState::NotConfigured, TunnelState::Disconnected] {
            for saved in [Some(&meta), None] {
                let status = combine_status(saved, Ok(no_vpn(tunnel)));
                assert_eq!(
                    status.state,
                    VpnState::NotConfigured,
                    "{tunnel:?} {saved:?}"
                );
                assert!(status.allow_no_vpn && status.reachable && !status.kill_switch);
                assert_eq!(status.configured, saved.is_some());
            }
        }
        // While the engine restarts, the supervisor's status file still tells this mode
        // apart: its kill switch is off, which only STASH_ALLOW_NO_VPN does.
        let mut tunnel = no_vpn(TunnelState::NotConfigured).vpn;
        tunnel.updated_at = stash_core::now();
        let status = status_from_supervisor(Some(&meta), tunnel);
        assert_eq!(status.state, VpnState::NotConfigured);
        assert!(status.allow_no_vpn && status.configured && !status.reachable);
        assert_eq!(status.error, None);
        // A normal supervisor (kill switch on) keeps the saved-VPN rule.
        let mut tunnel = sample_health(TunnelState::NotConfigured).vpn;
        tunnel.updated_at = stash_core::now();
        let status = status_from_supervisor(Some(&meta), tunnel);
        assert_eq!(status.state, VpnState::Connecting);
        assert!(!status.allow_no_vpn);
    }

    /// Under the supervisor the token comes from `STASH_INTERNAL_TOKEN` and must never be
    /// written to disk (section 7); a different file token would make every call a 401.
    #[tokio::test]
    async fn supervisor_token_from_env_is_used_and_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new("token-from-the-supervisor");
        let url = start_fake(fake.clone()).await;
        let mut config = test_config(dir.path(), Some(url));
        config.internal_token = Some("token-from-the-supervisor".into());
        let token = TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        assert_eq!(token, "token-from-the-supervisor");
        assert!(!config.internal_token_path().exists());
        assert!(config.vpn_dir().is_dir());
        let client = TorrentdClient::new(Arc::new(config));
        let health = client.health().await.unwrap();
        assert_eq!(health.vpn.state, TunnelState::Connected);
        assert!(
            client.download_proxy().await.is_none(),
            "no proxy configured"
        );
        assert!(!client.config.internal_token_path().exists());
    }

    #[tokio::test]
    async fn supervisor_status_when_engine_is_down() {
        let dir = tempfile::tempdir().unwrap();
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let url = Url::parse(&format!("http://127.0.0.1:{port}")).unwrap();
        // The supervisor writes status.json next to its socket (/run/popcornstash).
        let run_dir = dir.path().join("run");
        std::fs::create_dir_all(&run_dir).unwrap();
        let mut config = test_config(dir.path(), Some(url));
        config.supervisor_socket = Some(run_dir.join("supervisor.sock"));
        TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        let client = Arc::new(TorrentdClient::new(Arc::new(config)));
        let mut tunnel = sample_health(TunnelState::Error).vpn;
        tunnel.error = Some("No WireGuard handshake for 3 minutes. Retrying in 20 seconds.".into());
        tunnel.updated_at = stash_core::now();
        let status_path = run_dir.join("status.json");
        std::fs::write(&status_path, serde_json::to_vec(&tunnel).unwrap()).unwrap();
        let status = client.vpn_status().await;
        assert!(!status.reachable);
        assert_eq!(status.state, VpnState::Error);
        assert_eq!(
            status.error.as_deref(),
            Some("No WireGuard handshake for 3 minutes. Retrying in 20 seconds.")
        );
        assert_eq!(status.server.as_deref(), Some("nl812.nordvpn.com"));
        assert!(status.public_ip.is_none());

        // A stale file is ignored.
        tunnel.updated_at = stash_core::now() - 600;
        std::fs::write(&status_path, serde_json::to_vec(&tunnel).unwrap()).unwrap();
        client.note_vpn_changed();
        let status = client.vpn_status().await;
        assert_eq!(status.state, VpnState::Unreachable);
        assert_eq!(status.error.as_deref(), Some(MSG_UNREACHABLE));

        // Connected tunnel without an engine: say the engine is starting.
        let mut up = sample_health(TunnelState::Connected).vpn;
        up.updated_at = stash_core::now();
        let status = status_from_supervisor(None, up);
        assert_eq!(status.state, VpnState::Connected);
        assert_eq!(status.error.as_deref(), Some(MSG_ENGINE_STARTING));
    }

    #[tokio::test]
    async fn download_proxy_needs_connected_tunnel() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path(), None);
        let token = TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        let fake = Fake::new(&token);
        let url = start_fake(fake.clone()).await;
        let mut config = test_config(dir.path(), Some(url));
        config.torrentd_proxy = Some(Url::parse("http://127.0.0.1:9").unwrap());
        let client = TorrentdClient::new(Arc::new(config));
        assert!(client.download_proxy().await.is_some());
        *lock(&fake.tunnel) = TunnelState::Connecting;
        client.note_vpn_changed();
        assert!(client.download_proxy().await.is_none());
    }

    #[tokio::test]
    async fn status_watch_publishes_changes() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path(), None);
        let token = TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        let fake = Fake::new(&token);
        let url = start_fake(fake.clone()).await;
        let (client, _) = client_for(dir.path(), Some(url)).await;
        let events = EventBus::new();
        let mut rx = events.subscribe();
        let cancel = CancellationToken::new();
        client.spawn_status_watch(events.clone(), cancel.clone());
        let first = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(first, Event::Vpn(ref s) if s.state == VpnState::Connected));
        *lock(&fake.tunnel) = TunnelState::Disconnected;
        client.reload_vpn().await.unwrap();
        let second = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(second, Event::Vpn(ref s) if s.state == VpnState::Disconnected));
        cancel.cancel();
    }

    /// The engine parses data from strangers on the internet; if it were taken over it
    /// must not be able to exhaust the web server's memory with an endless answer, or
    /// put a long story in front of the family as a "VPN" message.
    #[tokio::test]
    async fn engine_answers_are_bounded() {
        let app = Router::new().route(
            "/torrents",
            get(|| async {
                // A valid (empty) list, padded past the limit.
                let mut body = vec![b' '; MAX_ENGINE_RESPONSE + 2];
                body[0] = b'[';
                *body.last_mut().unwrap() = b']';
                body
            })
            .post(|| async {
                let long = "Enter your NordVPN password at https://example.invalid ".repeat(40);
                (
                    AxumStatus::CONFLICT,
                    Json(
                        serde_json::json!({"error": {"code": "vpn_unavailable", "message": long}}),
                    ),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let dir = tempfile::tempdir().unwrap();
        let url = Url::parse(&format!("http://{addr}")).unwrap();
        let (client, _) = client_for(dir.path(), Some(url)).await;
        match client.list().await {
            Err(TorrentdError::Api { message, .. }) => assert_eq!(message, MSG_BAD_RESPONSE),
            other => panic!("an oversized answer must be refused: {other:?}"),
        }
        let request = AddTorrentRequest {
            magnet: Some("magnet:?xt=urn:btih:abc".into()),
            torrent_base64: None,
        };
        match client.add(&request).await {
            Err(TorrentdError::Blocked(message)) => {
                assert!(message.chars().count() <= MAX_ENGINE_MESSAGE, "{message}")
            }
            other => panic!("expected the engine's refusal: {other:?}"),
        }
    }

    #[tokio::test]
    async fn private_file_is_atomic_and_0600() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.conf");
        write_private_file(&path, b"one", 0o600).await.unwrap();
        write_private_file(&path, b"two", 0o600).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
