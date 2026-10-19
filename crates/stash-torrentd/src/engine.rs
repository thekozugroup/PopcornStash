//! librqbit session, public IP checks and the engine process main loop.
//!
//! The engine runs as its own user with no capabilities. It applies Landlock
//! to itself first, then runs librqbit with every socket bound to the tunnel
//! interface. librqbit binds sockets to the interface index that exists when
//! the session starts, so the session is created when the tunnel comes up,
//! stopped when it goes down and recreated when the interface changes (a
//! reconnect makes a new `psw0`). While no session runs the engine is
//! "blocked" while the tunnel is unusable. With a usable tunnel it is ready,
//! but leaves the peer/DHT session stopped until there is work.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use librqbit::api::TorrentIdOrHash;
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse as RqAddResponse, DhtSessionConfig,
    ListenerMode, ListenerOptions, Magnet, ManagedTorrent, Session, SessionOptions,
    SessionPersistenceConfig, TorrentStatsState,
};
use serde::{Deserialize, Serialize};
pub use stash_core::internal_api::MSG_NOT_PROTECTED;
use stash_core::internal_api::{
    AddTorrentResponse, EXIT_RELOAD, EngineHealth, EngineRunState, EngineState, EngineTorrent,
    EngineTorrentState, LimitsRequest, MIN_SPEED_LIMIT_KBPS, TorrentFile, TorrentFileKind,
    TorrentFiles, TunnelState, TunnelStatus, VpnProvider,
};
use tokio::net::TcpListener;
use tokio::sync::{Notify, RwLock, watch};
use tokio_util::sync::CancellationToken;

use crate::EngineOptions;
use crate::api::{self, BackendError, MSG_VPN_WAIT, NewTorrent, TorrentBackend};
use crate::proxy::{self, ProxyConfig};
use crate::public_trackers::{self, PublicTrackers};
use crate::status;
use crate::sys::{self, now_unix};
use crate::vpn::killswitch::{ENGINE_DHT_PORT, ENGINE_LISTEN_PORT};

#[path = "file_rules.rs"]
mod file_rules;

/// Public IP check interval while running (section 5, "Proof of protection").
pub const IP_CHECK_INTERVAL: Duration = Duration::from_secs(600);
const POLL: Duration = Duration::from_secs(1);
pub const NORDVPN_INSIGHTS: &str = "https://api.nordvpn.com/v1/helpers/ips/insights";
pub const IPIFY: &str = "https://api.ipify.org?format=json";
pub const MSG_NOT_CONFIGURED: &str = "The VPN is not set up yet.";
/// Between public-address checks while an earlier check said the address is not a VPN
/// address, so torrents start again soon after a check passes.
const UNPROTECTED_RECHECK: Duration = Duration::from_secs(60);
/// Wait between failed attempts to start the torrent session on the same interface.
const START_RETRY: Duration = Duration::from_secs(60);
const PENDING_FILE: &str = "pending.json";
const LIMITS_FILE: &str = "limits.json";
const PEERS_PER_TORRENT: usize = 50;
/// The last public-address answer while it says the address is not a VPN address, so a
/// restarted engine (Reconnect, a saved VPN, a container restart) keeps torrents stopped
/// until a check passes instead of starting them before its first check.
const NOT_PROTECTED_FILE: &str = "not-protected.json";
const SESSION_DIR: &str = "session";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where and how often the engine checks its public address through the tunnel (the
/// proof of protection). The services are fixed in production; tests point them at
/// local fakes with [`Engine::with_ip_check`].
#[derive(Debug, Clone)]
pub struct IpCheck {
    /// NordVPN's insights service: `{ip, country, protected}`.
    pub nordvpn_url: String,
    /// Any other provider: `{ip}`.
    pub other_url: String,
    /// Between checks while torrents run.
    pub interval: Duration,
    /// Between checks while torrents are stopped because the last one failed the proof.
    pub recheck_unprotected: Duration,
}

impl Default for IpCheck {
    fn default() -> Self {
        Self {
            nordvpn_url: NORDVPN_INSIGHTS.into(),
            other_url: IPIFY.into(),
            interval: IP_CHECK_INTERVAL,
            recheck_unprotected: UNPROTECTED_RECHECK,
        }
    }
}

impl IpCheck {
    fn url(&self, provider: Option<VpnProvider>) -> &str {
        if provider == Some(VpnProvider::Nordvpn) {
            &self.nordvpn_url
        } else {
            &self.other_url
        }
    }
}

/// Run the engine. Landlock restricts the calling thread and the threads it
/// creates, but `main` already started a tokio runtime, so the engine runs on
/// a dedicated thread that sandboxes itself and then builds its own runtime.
/// (On kernels with Landlock ABI 8 the sandbox also covers every other thread.)
pub async fn run(options: EngineOptions) -> anyhow::Result<i32> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("engine".into())
        .spawn(move || {
            let result = (|| {
                apply_landlock(&options);
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("engine-worker")
                    .build()?;
                runtime.block_on(run_sandboxed(options))
            })();
            let _ = tx.send(result);
        })?;
    rx.await
        .map_err(|_| anyhow::anyhow!("the engine thread stopped unexpectedly"))?
}

/// Read-only system paths and read-write download/state paths (best effort).
fn apply_landlock(options: &EngineOptions) {
    use landlock::{
        ABI, Access, AccessFs, CompatLevel, Compatible, RestrictSelfAttr, Ruleset, RulesetAttr,
        RulesetCreatedAttr, RulesetStatus, path_beneath_rules,
    };
    let abi = ABI::V6;
    let mut read_only: Vec<PathBuf> = [
        "/usr",
        "/lib",
        "/lib64",
        "/lib32",
        "/bin",
        "/etc",
        "/proc/self",
        "/sys/fs/cgroup",
        "/dev/null",
        "/dev/urandom",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if let Ok(exe) = std::env::current_exe() {
        read_only.push(exe);
    }
    if let Some(run_dir) = options.status_file.parent() {
        read_only.push(run_dir.to_path_buf());
    }
    let read_write = [options.downloads_dir.clone(), options.state_dir.clone()];
    let result = (|| -> Result<_, landlock::RulesetError> {
        Ruleset::default()
            .set_compatibility(CompatLevel::BestEffort)
            .handle_access(AccessFs::from_all(abi))?
            .create()?
            .add_rules(path_beneath_rules(&read_only, AccessFs::from_read(abi)))?
            .add_rules(path_beneath_rules(&read_write, AccessFs::from_all(abi)))?
            .all_threads(true)?
            .restrict_self()
    })();
    match result {
        Ok(status) => match status.ruleset {
            RulesetStatus::FullyEnforced => tracing::info!(
                all_threads = status.all_threads,
                "Landlock sandbox enforced"
            ),
            RulesetStatus::PartiallyEnforced => tracing::info!(
                all_threads = status.all_threads,
                "Landlock sandbox partially enforced (older kernel)"
            ),
            RulesetStatus::NotEnforced => tracing::warn!(
                "this kernel does not support Landlock; the engine runs without its filesystem sandbox"
            ),
        },
        Err(e) => {
            tracing::warn!(error = %e, "could not apply the Landlock sandbox; continuing without it")
        }
    }
}

async fn run_sandboxed(options: EngineOptions) -> anyhow::Result<i32> {
    let engine = Arc::new(Engine::new(&options));
    let shutdown = CancellationToken::new();
    {
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};
            let (Ok(mut term), Ok(mut int)) = (
                signal(SignalKind::terminate()),
                signal(SignalKind::interrupt()),
            ) else {
                return;
            };
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
            tracing::info!("stopping the torrent engine");
            shutdown.cancel();
        });
    }
    let api_listener = TcpListener::bind(options.api_bind).await?;
    let proxy_listener = TcpListener::bind(options.proxy_bind).await?;
    tracing::info!(api = %options.api_bind, proxy = %options.proxy_bind, tunnel = ?options.tunnel_if, "torrent engine listening");
    let app = api::router(engine.clone(), &options.internal_token);
    let api_shutdown = shutdown.clone();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(api_listener, app)
            .with_graceful_shutdown(api_shutdown.cancelled_owned())
            .await
        {
            tracing::error!(error = %e, "internal API stopped");
        }
    });
    let proxy_config = Arc::new(ProxyConfig::new(
        options.internal_token.clone(),
        options.tunnel_if.clone(),
    ));
    tokio::spawn(proxy::serve(
        proxy_listener,
        proxy_config,
        engine.tunnel_usable(),
        shutdown.clone(),
    ));
    let code = engine.clone().supervise(shutdown.clone()).await;
    shutdown.cancel();
    Ok(code)
}

/// Why the main loop ends.
enum Stop {
    Reload,
    Shutdown,
}

/// A magnet whose metadata is still being fetched.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingAdd {
    #[serde(default)]
    file_types: Option<Vec<TorrentFileKind>>,
    magnet: String,
    name: Option<String>,
    #[serde(default)]
    paused: bool,
    #[serde(skip)]
    error: Option<String>,
    #[serde(skip)]
    resolving: Option<Arc<CancellationToken>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IpInfo {
    ip: String,
    country: Option<String>,
    protected: Option<bool>,
    checked_at: i64,
}

struct Live {
    session: Arc<Session>,
    ifindex: Option<u32>,
}

struct Inner {
    tunnel: Option<TunnelStatus>,
    running: bool,
    reason: Option<String>,
    ip: Option<IpInfo>,
    /// The last public-address check said the address is not a VPN address: torrents
    /// stay stopped until a later check passes.
    unprotected: bool,
    limits: LimitsRequest,
    pending: BTreeMap<String, PendingAdd>,
    last_known: HashMap<String, EngineTorrent>,
    reserved_folders: HashSet<String>,
}

pub struct Engine {
    rules: Mutex<file_rules::RuleBook>,
    rules_load_failed: bool,
    downloads_dir: PathBuf,
    state_dir: PathBuf,
    status_file: PathBuf,
    tunnel_if: Option<String>,
    allow_no_vpn: bool,
    ip_check: IpCheck,
    public_trackers: Mutex<PublicTrackers>,
    restart_trackers: AtomicBool,
    session: RwLock<Option<Live>>,
    /// Serialize metadata handoff with pause, resume, remove and re-add.
    pending_ops: tokio::sync::Mutex<()>,
    inner: Mutex<Inner>,
    running_tx: watch::Sender<bool>,
    seed_tx: watch::Sender<bool>,
    /// The tunnel is up (or no VPN is required): the proxy may forward.
    tunnel_tx: watch::Sender<bool>,
    reload: Notify,
    reload_requested: AtomicBool,
    #[cfg(test)]
    offline_sessions: bool,
}

/// Name of the folder a torrent is downloaded into: the torrent name (without
/// the extension for single-file torrents), made safe as one path component.
pub fn folder_name(name: Option<&str>, single_file: bool, fallback: &str) -> String {
    let raw = name.unwrap_or("");
    let raw = if single_file {
        match raw.rfind('.') {
            Some(dot) if dot > 0 && raw.len() - dot <= 6 => &raw[..dot],
            _ => raw,
        }
    } else {
        raw
    };
    let mut cleaned: String = raw
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    cleaned = cleaned
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .to_string();
    if cleaned.len() > 200 {
        let mut cut = 200;
        while !cleaned.is_char_boundary(cut) {
            cut -= 1;
        }
        cleaned.truncate(cut);
    }
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

fn torrent_state(state: &TorrentStatsState, finished: bool) -> EngineTorrentState {
    match state {
        TorrentStatsState::Initializing { paused: true } => EngineTorrentState::Paused,
        TorrentStatsState::Initializing { .. } => EngineTorrentState::Initializing,
        TorrentStatsState::Live if finished => EngineTorrentState::Seeding,
        TorrentStatsState::Live => EngineTorrentState::Downloading,
        TorrentStatsState::Paused => EngineTorrentState::Paused,
        TorrentStatsState::Error => EngineTorrentState::Error,
    }
}

fn engine_torrent(handle: &ManagedTorrent) -> EngineTorrent {
    let stats = handle.stats();
    let id = handle.info_hash().as_string();
    let empty_selection = handle.only_files().is_some_and(|files| files.is_empty());
    let total = if empty_selection {
        0
    } else {
        stats.total_bytes
    };
    let done = if empty_selection || matches!(stats.state, TorrentStatsState::Initializing { .. }) {
        0
    } else {
        stats.progress_bytes.min(total)
    };
    let (down, up, peers) = stats
        .live
        .as_ref()
        .map(|live| {
            (
                live.download_speed.as_bytes(),
                live.upload_speed.as_bytes(),
                Some(live.snapshot.peer_stats.live),
            )
        })
        .unwrap_or((0, 0, None));
    let progress = if total > 0 {
        (done as f64 / total as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let eta_s = (down > 0 && total > done).then(|| (total - done) / down);
    EngineTorrent {
        id: id.clone(),
        info_hash: id,
        name: handle.name(),
        state: if empty_selection {
            EngineTorrentState::Paused
        } else {
            torrent_state(&stats.state, stats.finished)
        },
        progress,
        total_bytes: (total > 0).then_some(total),
        done_bytes: done,
        uploaded_bytes: stats.uploaded_bytes,
        down_bps: down,
        up_bps: up,
        peers,
        eta_s,
        error: stats.error.clone(),
        output_dir: Some(handle.output_folder().display().to_string()),
    }
}

fn selected_indices(
    bytes: &[u8],
    types: Option<&[TorrentFileKind]>,
) -> anyhow::Result<Option<Vec<usize>>> {
    let Some(types) = types else { return Ok(None) };
    let meta = librqbit::torrent_from_bytes(bytes)?;
    let info = meta.info.data.validate()?;
    let indices = info
        .iter_file_details()
        .enumerate()
        .filter_map(|(index, f)| {
            (!f.attrs().padding && types.contains(&TorrentFileKind::of(&f.filename.to_string())))
                .then_some(index)
        })
        .collect();
    Ok(Some(indices))
}

/// The sentence shown when the torrent session cannot start.
fn start_failure_reason(detail: &str) -> String {
    if detail.contains("Address family not supported") {
        // librqbit always opens an IPv6 UDP socket for trackers.
        "Torrents need a kernel with IPv6 support (it may be turned off with ipv6.disable=1). The VPN is connected, but downloads cannot start.".into()
    } else {
        "The torrent engine could not start. Check the logs.".into()
    }
}

fn parse_id(id: &str) -> Result<Id20, BackendError> {
    id.parse::<Id20>().map_err(|_| BackendError::NotFound)
}

/// A speed limit for librqbit: none for 0, and never below the 16 KiB/s it needs to
/// move a block at all (a lower limit would stall every transfer instead of slowing it).
fn kbps_to_bps(kbps: u64) -> Option<NonZeroU32> {
    if kbps == 0 {
        return None;
    }
    let kbps = kbps.max(MIN_SPEED_LIMIT_KBPS);
    NonZeroU32::new(u32::try_from(kbps.saturating_mul(1024)).unwrap_or(u32::MAX))
}

/// True when `ifname` has an IPv6 address (from /proc/net/if_inet6).
fn interface_has_ipv6(ifname: Option<&str>) -> bool {
    let Ok(text) = std::fs::read_to_string("/proc/net/if_inet6") else {
        return false;
    };
    match ifname {
        Some(name) => text
            .lines()
            .any(|l| l.split_whitespace().last() == Some(name)),
        None => true,
    }
}

fn if_index(name: &str) -> Option<u32> {
    nix::net::if_::if_nametoindex(name).ok().filter(|i| *i != 0)
}

impl Engine {
    pub fn new(options: &EngineOptions) -> Self {
        let (running_tx, _) = watch::channel(false);
        let (tunnel_tx, _) = watch::channel(false);
        let saved_rules = file_rules::load(&options.state_dir);
        let rules_load_failed = saved_rules.is_err();
        let engine = Self {
            rules: Mutex::new(saved_rules.unwrap_or_default()),
            rules_load_failed,
            downloads_dir: options.downloads_dir.clone(),
            state_dir: options.state_dir.clone(),
            status_file: options.status_file.clone(),
            tunnel_if: options.tunnel_if.clone(),
            allow_no_vpn: options.allow_no_vpn,
            ip_check: IpCheck::default(),
            public_trackers: Mutex::new(PublicTrackers::load(&options.state_dir)),
            restart_trackers: AtomicBool::new(false),
            session: RwLock::new(None),
            pending_ops: tokio::sync::Mutex::new(()),
            inner: Mutex::new(Inner {
                tunnel: None,
                running: false,
                reason: Some(MSG_VPN_WAIT.into()),
                ip: None,
                unprotected: false,
                limits: LimitsRequest::default(),
                pending: BTreeMap::new(),
                last_known: HashMap::new(),
                reserved_folders: HashSet::new(),
            }),
            running_tx,
            seed_tx: watch::channel(false).0,
            tunnel_tx,
            reload: Notify::new(),
            reload_requested: AtomicBool::new(false),
            #[cfg(test)]
            offline_sessions: false,
        };
        engine.load_saved_state();
        engine.seed_tx.send_replace(lock(&engine.inner).limits.seed);
        engine
    }

    /// Check the public address against other services (tests).
    pub fn with_ip_check(mut self, ip_check: IpCheck) -> Self {
        self.ip_check = ip_check;
        self
    }

    /// `true` while the engine is ready to accept torrents (also while idle).
    pub fn running(&self) -> watch::Receiver<bool> {
        self.running_tx.subscribe()
    }

    /// `true` while the tunnel is usable. The CONNECT proxy follows this rather
    /// than the torrent session, so web downloads through the VPN keep working
    /// even if librqbit could not start.
    pub fn tunnel_usable(&self) -> watch::Receiver<bool> {
        self.tunnel_tx.subscribe()
    }

    fn load_saved_state(&self) {
        let mut inner = lock(&self.inner);
        if let Ok(bytes) = std::fs::read(self.state_dir.join(PENDING_FILE))
            && let Ok(pending) = serde_json::from_slice::<BTreeMap<String, PendingAdd>>(&bytes)
        {
            inner.pending = pending;
        }
        if let Ok(bytes) = std::fs::read(self.state_dir.join(LIMITS_FILE))
            && let Ok(limits) = serde_json::from_slice::<LimitsRequest>(&bytes)
        {
            inner.limits = limits;
        }
        // A "not protected" answer from before the restart: torrents wait for a check
        // that passes, and the status keeps showing the answer meanwhile.
        if !self.allow_no_vpn
            && let Ok(bytes) = std::fs::read(self.state_dir.join(NOT_PROTECTED_FILE))
            && let Ok(info) = serde_json::from_slice::<IpInfo>(&bytes)
            && info.protected == Some(false)
        {
            inner.unprotected = true;
            inner.ip = Some(info);
        }
    }

    /// Keep the "not protected" answer across engine restarts, or forget it once a check
    /// no longer says so.
    fn save_not_protected(&self, info: Option<&IpInfo>) {
        if self.allow_no_vpn {
            // Torrents do not wait for the check in this mode.
            return;
        }
        let path = self.state_dir.join(NOT_PROTECTED_FILE);
        let result = match info {
            Some(info) => {
                let bytes = serde_json::to_vec(info).unwrap_or_default();
                sys::write_atomic(&path, &bytes, 0o600)
            }
            None => match std::fs::remove_file(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
        };
        if let Err(e) = result {
            tracing::warn!(error = %e, "could not save the public-address answer");
        }
    }

    fn save_pending(&self, pending: &BTreeMap<String, PendingAdd>) {
        let bytes = serde_json::to_vec(pending).unwrap_or_default();
        if let Err(e) = sys::write_atomic(&self.state_dir.join(PENDING_FILE), &bytes, 0o600) {
            tracing::warn!(error = %e, "could not save pending magnet links");
        }
    }

    fn session_db(&self) -> PathBuf {
        self.state_dir.join(SESSION_DIR).join("session.json")
    }

    /// Decide whether torrents may run: `Ok(interface index)` or `Err(reason)`.
    fn decide(&self, status: Option<&TunnelStatus>, now: i64) -> Result<Option<u32>, String> {
        if self.allow_no_vpn {
            return Ok(None);
        }
        if !status::tunnel_connected(status, now) {
            return Err(match status {
                Some(s) if now.saturating_sub(s.updated_at) > status::STALE_AFTER_SECS => {
                    MSG_VPN_WAIT.to_string()
                }
                Some(s) => match s.state {
                    TunnelState::NotConfigured => MSG_NOT_CONFIGURED.to_string(),
                    TunnelState::Error => {
                        s.error.clone().unwrap_or_else(|| MSG_VPN_WAIT.to_string())
                    }
                    _ => MSG_VPN_WAIT.to_string(),
                },
                None => MSG_VPN_WAIT.to_string(),
            });
        }
        let Some(ifname) = self.tunnel_if.as_deref() else {
            return Err("The torrent engine was started without a tunnel interface.".into());
        };
        if_index(ifname)
            .map(Some)
            .ok_or_else(|| MSG_VPN_WAIT.to_string())
    }

    fn set_running(&self, running: bool, reason: Option<String>) {
        let changed = {
            let mut inner = lock(&self.inner);
            let changed = inner.running != running || inner.reason != reason;
            inner.running = running;
            inner.reason = reason.clone();
            // A "not protected" answer stays until a check no longer says so, also while
            // the tunnel is down, so the status never reads as protected meanwhile.
            if !running && !inner.unprotected {
                inner.ip = None;
            }
            changed
        };
        if changed {
            if running {
                tracing::info!("torrent engine is ready");
            } else {
                tracing::info!(
                    reason = reason.as_deref().unwrap_or(""),
                    "torrents are blocked"
                );
            }
        }
        self.running_tx.send_replace(running);
    }

    /// Stop torrents because the public address is not a VPN address. The check's answer
    /// stays, so the status shows the address and `protected: false`.
    fn block_unprotected(&self) {
        let changed = {
            let mut inner = lock(&self.inner);
            let changed = inner.running || inner.reason.as_deref() != Some(MSG_NOT_PROTECTED);
            inner.running = false;
            inner.reason = Some(MSG_NOT_PROTECTED.into());
            changed
        };
        if changed {
            tracing::warn!("the public address is not a VPN address; torrents are blocked");
        }
        self.running_tx.send_replace(false);
    }

    /// Main loop: follow the tunnel state until shutdown or reload.
    pub async fn supervise(self: Arc<Self>, shutdown: CancellationToken) -> i32 {
        let mut last_ip_check: Option<Instant> = None;
        // After a failed start, wait before trying the same interface again.
        let mut failed_start: Option<(Option<u32>, Instant)> = None;
        loop {
            let now = now_unix();
            let status = status::read(&self.status_file);
            let provider = status.as_ref().and_then(|s| s.provider);
            lock(&self.inner).tunnel = status.clone();
            let decision = self.decide(status.as_ref(), now);
            self.tunnel_tx.send_replace(decision.is_ok());
            match decision {
                Ok(ifindex) => {
                    // The tunnel is up but the last check said the address is not a VPN
                    // address: no torrents until a later check passes.
                    let unprotected = !self.allow_no_vpn && lock(&self.inner).unprotected;
                    if !unprotected {
                        self.refresh_public_trackers();
                        if self.restart_trackers.swap(false, Ordering::SeqCst) {
                            // Flush and restore all torrents with the new session-wide list.
                            // Original tracker metadata is untouched; private torrents ignore it.
                            self.stop_session().await;
                            failed_start = None;
                        }
                    }
                    let current = self.session.read().await.as_ref().map(|live| live.ifindex);
                    let wait_retry = failed_start.is_some_and(|(failed, at)| {
                        failed == ifindex && at.elapsed() < START_RETRY
                    });
                    let has_work = self.has_session_work().await;
                    if unprotected {
                        if current.is_some() {
                            tracing::warn!(
                                "the public address is not a VPN address; stopping torrents"
                            );
                            self.stop_session().await;
                        }
                        self.block_unprotected();
                    } else if current != Some(ifindex) && !wait_retry && has_work {
                        self.stop_session().await;
                        // Starting can take several seconds; a reload or shutdown must not wait for it.
                        let started = tokio::select! {
                            r = self.start_session(ifindex) => r,
                            _ = self.reload.notified() => return self.finish(Stop::Reload).await,
                            _ = shutdown.cancelled() => return self.finish(Stop::Shutdown).await,
                        };
                        match started {
                            Ok(()) => {
                                failed_start = None;
                                self.set_running(true, None);
                                last_ip_check = None;
                            }
                            Err(e) => {
                                let detail = format!("{e:#}");
                                tracing::error!(error = %detail, retry_in_s = START_RETRY.as_secs(), "could not start the torrent session");
                                failed_start = Some((ifindex, Instant::now()));
                                self.set_running(false, Some(start_failure_reason(&detail)));
                            }
                        }
                    } else if !has_work && !wait_retry {
                        // Ready to accept work; no DHT or peer listener is needed yet.
                        self.set_running(true, None);
                    }
                    // Magnets added since the last tick (or left over from a stopped session).
                    self.spawn_resolutions().await;
                    if !unprotected {
                        self.stop_idle_session().await;
                    }
                    let interval = if unprotected {
                        self.ip_check.recheck_unprotected
                    } else {
                        self.ip_check.interval
                    };
                    let due = last_ip_check.is_none_or(|t| t.elapsed() >= interval);
                    if due && (unprotected || lock(&self.inner).running) {
                        last_ip_check = Some(Instant::now());
                        let engine = self.clone();
                        tokio::spawn(async move { engine.check_public_ip(provider).await });
                    }
                }
                Err(reason) => {
                    if self.session.read().await.is_some() {
                        tracing::info!(reason = %reason, "the tunnel is not usable; stopping torrents");
                        self.stop_session().await;
                    }
                    self.set_running(false, Some(reason));
                    // Check again as soon as the tunnel is back, so a "not protected"
                    // answer from before the drop is confirmed or cleared at once.
                    last_ip_check = None;
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(POLL) => {}
                _ = self.reload.notified() => return self.finish(Stop::Reload).await,
                _ = shutdown.cancelled() => return self.finish(Stop::Shutdown).await,
            }
        }
    }

    /// Stop torrents (librqbit flushes its state) and return the exit code.
    async fn finish(&self, stop: Stop) -> i32 {
        self.tunnel_tx.send_replace(false);
        self.set_running(false, Some(MSG_VPN_WAIT.into()));
        self.stop_session().await;
        match stop {
            Stop::Reload => EXIT_RELOAD,
            Stop::Shutdown => 0,
        }
    }

    fn session_options(&self, ifindex: Option<u32>, fixed_ports: bool) -> SessionOptions {
        let limits = lock(&self.inner).limits;
        let bind_device_name = ifindex.and(self.tunnel_if.clone());
        let ipv4_only = !interface_has_ipv6(bind_device_name.as_deref());
        let listen_port = if fixed_ports { ENGINE_LISTEN_PORT } else { 0 };
        let listen_addr: SocketAddr = (Ipv4Addr::UNSPECIFIED, listen_port).into();
        let options = SessionOptions {
            dht: Some(DhtSessionConfig {
                bootstrap_addrs: None,
                port: fixed_ports.then_some(ENGINE_DHT_PORT),
                persistence: Some(DhtPersistenceConfig {
                    dump_interval: None,
                    config_filename: Some(self.state_dir.join("dht.json")),
                }),
            }),
            bind_device_name,
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json {
                folder: Some(self.state_dir.join(SESSION_DIR)),
            }),
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpAndUtp,
                listen_addr,
                ipv4_only,
                ..Default::default()
            }),
            disable_local_service_discovery: true,
            ipv4_only,
            ratelimits: LimitsConfig {
                download_bps: kbps_to_bps(limits.down_kbps),
                upload_bps: kbps_to_bps(limits.up_kbps),
            },
            trackers: if limits.add_public_trackers {
                lock(&self.public_trackers).trackers()
            } else {
                HashSet::new()
            },
            peer_limit: Some(PEERS_PER_TORRENT),
            ..Default::default()
        };
        #[cfg(test)]
        let options = if self.offline_sessions {
            SessionOptions {
                dht: None,
                disable_trackers: true,
                listen: None,
                ..options
            }
        } else {
            options
        };
        options
    }

    async fn start_session(&self, ifindex: Option<u32>) -> anyhow::Result<()> {
        let _operation = self.pending_ops.lock().await;
        if self
            .session
            .read()
            .await
            .as_ref()
            .is_some_and(|live| live.ifindex == ifindex)
        {
            return Ok(());
        }
        self.start_session_locked(ifindex).await
    }

    async fn start_session_locked(&self, ifindex: Option<u32>) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.rules_load_failed,
            "Saved file rules could not be read. Repair file-rules.json before restarting downloads."
        );
        self.reconcile_rules()?;
        let mut last_error = None;
        // Fixed ports first (the kill switch opens them on the tunnel); a port
        // still held by the previous session is retried, then any port is used.
        for attempt in 0..4 {
            let options = self.session_options(ifindex, attempt < 3);
            match Session::new_with_opts(self.downloads_dir.clone(), options).await {
                Ok(session) => {
                    let restored = session.with_torrents(|iter| {
                        iter.map(|(_, handle)| handle.clone()).collect::<Vec<_>>()
                    });
                    for handle in restored {
                        self.watch_completion(&session, handle);
                    }
                    tracing::info!(interface = ?self.tunnel_if, ifindex = ?ifindex, "torrent session started");
                    *self.session.write().await = Some(Live { session, ifindex });
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(attempt, error = %format!("{e:#}"), "torrent session did not start");
                    last_error = Some(e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("unknown error")))
    }

    async fn stop_session(&self) {
        let _pending_op = self.pending_ops.lock().await;
        self.stop_session_locked().await;
    }

    async fn stop_session_locked(&self) {
        let live = self.session.write().await.take();
        if let Some(live) = live {
            let snapshot: Vec<EngineTorrent> = live
                .session
                .with_torrents(|iter| iter.map(|(_, handle)| engine_torrent(handle)).collect());
            {
                let mut inner = lock(&self.inner);
                inner.last_known = snapshot.into_iter().map(|t| (t.id.clone(), t)).collect();
                for pending in inner.pending.values_mut() {
                    if let Some(cancel) = pending.resolving.take() {
                        cancel.cancel();
                    }
                }
            }
            live.session.stop().await;
            tracing::info!("torrent session stopped");
        }
    }

    async fn has_session_work(&self) -> bool {
        if !lock(&self.inner).pending.is_empty() {
            return true;
        }
        match self.live_session().await {
            Some(session) => session.with_torrents(|iter| iter.next().is_some()),
            None => !self.persisted_torrents().is_empty(),
        }
    }

    async fn stop_idle_session(&self) {
        // The emptiness check and stop share the add/control lock: a just-added
        // torrent cannot be stopped from an earlier, empty snapshot.
        let _operation = self.pending_ops.lock().await;
        if !self.has_session_work().await {
            self.stop_session_locked().await;
        }
    }

    fn watch_completion(&self, session: &Arc<Session>, handle: Arc<ManagedTorrent>) {
        let session = session.clone();
        let mut seed = self.seed_tx.subscribe();
        tokio::spawn(async move {
            let cancellation = session.cancellation_token();
            loop {
                let current = session.get(TorrentIdOrHash::Hash(handle.info_hash()));
                if !current.is_some_and(|current| Arc::ptr_eq(&current, &handle)) {
                    return;
                }
                let stats = handle.stats();
                let seed_allowed = *seed.borrow_and_update();
                if stats.finished
                    && !seed_allowed
                    && !handle.is_paused()
                    && let Err(error) = session.pause(&handle).await
                {
                    tracing::warn!(%error, "could not pause a completed torrent; will retry");
                }
                tokio::select! {
                    _ = cancellation.cancelled() => return,
                    changed = seed.changed() => if changed.is_err() { return; },
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {},
                    _ = handle.wait_until_completed(), if !stats.finished && stats.error.is_none() => {},
                }
            }
        });
    }

    async fn live_session(&self) -> Option<Arc<Session>> {
        self.session
            .read()
            .await
            .as_ref()
            .map(|live| live.session.clone())
    }

    /// Pick a folder under `downloads_dir` that no other torrent uses.
    fn unique_folder(&self, base: &str) -> String {
        let mut inner = lock(&self.inner);
        for n in 1..1000 {
            let candidate = if n == 1 {
                base.to_string()
            } else {
                format!("{base} ({n})")
            };
            if !inner.reserved_folders.contains(&candidate)
                && std::fs::symlink_metadata(self.downloads_dir.join(&candidate)).is_err()
            {
                inner.reserved_folders.insert(candidate.clone());
                return candidate;
            }
        }
        format!("{base} {}", hex::encode(rand::random::<[u8; 4]>()))
    }

    async fn spawn_resolutions(self: &Arc<Self>) {
        let Some(session) = self.live_session().await else {
            return;
        };
        let todo: Vec<_> = {
            let mut inner = lock(&self.inner);
            inner
                .pending
                .iter_mut()
                .filter(|(_, p)| !p.paused && p.resolving.is_none() && p.error.is_none())
                .map(|(id, p)| {
                    let cancel = Arc::new(CancellationToken::new());
                    p.resolving = Some(cancel.clone());
                    (id.clone(), p.magnet.clone(), cancel)
                })
                .collect()
        };
        for (id, magnet, cancel) in todo {
            let engine = self.clone();
            let session = session.clone();
            tokio::spawn(async move {
                let session_cancel = session.cancellation_token();
                let result = tokio::select! {
                    _ = session_cancel.cancelled() => None,
                    _ = cancel.cancelled() => None,
                    r = session.add_torrent(AddTorrent::from_url(&magnet), Some(AddTorrentOptions {
                        list_only: true,
                        ..Default::default()
                    })) => Some(r),
                };
                engine
                    .finish_resolution(&session, &id, &magnet, &cancel, result)
                    .await;
            });
        }
    }

    async fn finish_resolution(
        &self,
        session: &Arc<Session>,
        id: &str,
        magnet: &str,
        cancel: &Arc<CancellationToken>,
        result: Option<anyhow::Result<RqAddResponse>>,
    ) {
        let _pending_op = self.pending_ops.lock().await;
        {
            let mut inner = lock(&self.inner);
            let Some(pending) = inner.pending.get_mut(id) else {
                return;
            };
            if !pending
                .resolving
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, cancel))
            {
                return;
            }
            if pending.paused
                || cancel.is_cancelled()
                || session.cancellation_token().is_cancelled()
                || result.is_none()
            {
                pending.resolving = None;
                return;
            }
        }
        let Some(result) = result else {
            return;
        };
        let result = match result {
            Ok(RqAddResponse::ListOnly(list)) => self.add_resolved(session, id, magnet, list).await,
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        let mut inner = lock(&self.inner);
        match result {
            Ok(()) => {
                inner.pending.remove(id);
                self.save_pending(&inner.pending);
            }
            Err(e) => {
                tracing::warn!(id, error = %format!("{e:#}"), "could not add a magnet link");
                if let Some(pending) = inner.pending.get_mut(id) {
                    pending.resolving = None;
                    pending.error = Some("The torrent's details could not be fetched.".into());
                }
            }
        }
    }

    async fn add_resolved(
        &self,
        session: &Arc<Session>,
        id: &str,
        magnet: &str,
        list: librqbit::ListOnlyResponse,
    ) -> anyhow::Result<()> {
        let trackers = Magnet::parse(magnet)
            .map(|m| m.trackers)
            .unwrap_or_default();
        let name = list.info.name().map(|n| n.into_owned());
        let single = list.info.info().files.is_none();
        let folder = self.unique_folder(&folder_name(name.as_deref(), single, id));
        let file_types = lock(&self.inner)
            .pending
            .get(id)
            .and_then(|p| p.file_types.clone());
        let only_files = self.rule_indices(&list.torrent_bytes, id, file_types.as_deref())?;
        let options = AddTorrentOptions {
            paused: only_files.as_ref().is_some_and(Vec::is_empty),
            only_files,
            sub_folder: Some(folder),
            initial_peers: Some(list.seen_peers.clone()),
            trackers: (!trackers.is_empty()).then_some(trackers),
            ..Default::default()
        };
        let added = session
            .add_torrent(
                AddTorrent::from_bytes(list.torrent_bytes.clone()),
                Some(options),
            )
            .await?;
        if let Some(handle) = added.into_handle() {
            self.watch_completion(session, handle);
        }
        Ok(())
    }

    /// Never ask the feed while opted out, blocked or outside the engine's tunnel policy.
    fn refresh_public_trackers(self: &Arc<Self>) {
        if !lock(&self.inner).limits.add_public_trackers
            || !lock(&self.public_trackers).begin_refresh(now_unix())
        {
            return;
        }
        let engine = self.clone();
        tokio::spawn(async move {
            let result =
                public_trackers::fetch(engine.tunnel_if.as_deref(), public_trackers::SOURCE).await;
            let changed = lock(&engine.public_trackers).finish_refresh(&engine.state_dir, result);
            if changed && lock(&engine.inner).limits.add_public_trackers {
                engine.restart_trackers.store(true, Ordering::SeqCst);
            }
        });
    }

    async fn check_public_ip(&self, provider: Option<VpnProvider>) {
        let url = self.ip_check.url(provider);
        match fetch_public_ip(self.tunnel_if.as_deref(), url).await {
            Ok(info) => {
                tracing::info!(ip = %info.ip, country = ?info.country, protected = ?info.protected, "public IP through the tunnel");
                // Only a clear "not protected" blocks; no answer (ipify) or an error does not.
                let unprotected = info.protected == Some(false);
                let was_unprotected = {
                    let mut inner = lock(&self.inner);
                    let was = inner.unprotected;
                    inner.unprotected = unprotected;
                    // A passing answer replaces the failing one at once, before the
                    // session has started again.
                    if inner.running || unprotected || was {
                        inner.ip = Some(info.clone());
                    }
                    was
                };
                if unprotected {
                    self.save_not_protected(Some(&info));
                } else if was_unprotected {
                    self.save_not_protected(None);
                }
            }
            Err(e) => tracing::warn!(error = %e, "public IP check failed"),
        }
    }

    // --- blocked-state operations on the persisted session -------------------

    fn read_session_db(&self) -> serde_json::Value {
        std::fs::read(self.session_db())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| serde_json::json!({"torrents": {}}))
    }

    fn write_session_db(&self, db: &serde_json::Value) -> Result<(), BackendError> {
        let bytes = serde_json::to_vec(db).map_err(|e| BackendError::Internal(e.to_string()))?;
        sys::write_atomic(&self.session_db(), &bytes, 0o600)
            .map_err(|e| BackendError::Internal(e.to_string()))
    }

    /// Torrents remembered by librqbit while no session runs.
    fn persisted_torrents(&self) -> Vec<EngineTorrent> {
        let db = self.read_session_db();
        let last_known = lock(&self.inner).last_known.clone();
        let Some(torrents) = db.get("torrents").and_then(|t| t.as_object()) else {
            return Vec::new();
        };
        torrents
            .values()
            .filter_map(|entry| {
                let hash = entry.get("info_hash")?.as_str()?.to_ascii_lowercase();
                let output = entry
                    .get("output_folder")
                    .and_then(|o| o.as_str())
                    .map(str::to_string);
                let mut torrent = last_known
                    .get(&hash)
                    .cloned()
                    .unwrap_or_else(|| EngineTorrent {
                        id: hash.clone(),
                        info_hash: hash.clone(),
                        name: output
                            .as_deref()
                            .and_then(|o| Path::new(o).file_name())
                            .map(|n| n.to_string_lossy().into_owned()),
                        state: EngineTorrentState::Paused,
                        progress: 0.0,
                        total_bytes: None,
                        done_bytes: 0,
                        uploaded_bytes: 0,
                        down_bps: 0,
                        up_bps: 0,
                        peers: None,
                        eta_s: None,
                        error: None,
                        output_dir: output.clone(),
                    });
                torrent.state = EngineTorrentState::Paused;
                torrent.down_bps = 0;
                torrent.up_bps = 0;
                torrent.peers = None;
                torrent.eta_s = None;
                Some(torrent)
            })
            .collect()
    }

    /// Edit one persisted torrent; returns its output folder.
    fn edit_persisted(
        &self,
        id: &str,
        edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>) -> bool,
    ) -> Result<Option<String>, BackendError> {
        let mut db = self.read_session_db();
        let torrents = db
            .get_mut("torrents")
            .and_then(|t| t.as_object_mut())
            .ok_or(BackendError::NotFound)?;
        let key = torrents
            .iter()
            .find(|(_, v)| {
                v.get("info_hash")
                    .and_then(|h| h.as_str())
                    .is_some_and(|h| h.eq_ignore_ascii_case(id))
            })
            .map(|(k, _)| k.clone())
            .ok_or(BackendError::NotFound)?;
        let output = torrents
            .get(&key)
            .and_then(|v| v.get("output_folder"))
            .and_then(|o| o.as_str())
            .map(str::to_string);
        let remove = match torrents.get_mut(&key).and_then(|v| v.as_object_mut()) {
            Some(entry) => edit(entry),
            None => return Err(BackendError::NotFound),
        };
        if remove {
            torrents.remove(&key);
        }
        self.write_session_db(&db)?;
        Ok(output)
    }

    /// Remove the files of a torrent folder, only when it is strictly inside the downloads folder.
    fn delete_output(&self, output: &str) {
        let path = Path::new(output);
        let inside = path.parent().is_some_and(|p| p == self.downloads_dir)
            && path.file_name().is_some_and(|n| n != "." && n != "..");
        if !inside {
            tracing::warn!(path = %output, "not deleting files outside a torrent folder");
            return;
        }
        let result = match std::fs::symlink_metadata(path) {
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
            Ok(_) => std::fs::remove_file(path),
            Err(_) => Ok(()),
        };
        if let Err(e) = result {
            tracing::warn!(path = %output, error = %e, "could not delete torrent files");
        }
    }

    fn set_pending_paused(&self, id: &str, paused: bool) -> bool {
        let mut inner = lock(&self.inner);
        let Some(pending) = inner.pending.get_mut(id) else {
            return false;
        };
        pending.paused = paused;
        if paused {
            if let Some(cancel) = pending.resolving.take() {
                cancel.cancel();
            }
        } else {
            pending.error = None;
        }
        self.save_pending(&inner.pending);
        true
    }

    fn pending_entries(&self, known: &HashSet<String>) -> Vec<EngineTorrent> {
        let inner = lock(&self.inner);
        inner
            .pending
            .iter()
            .filter(|(id, _)| !known.contains(*id))
            .map(|(id, p)| EngineTorrent {
                id: id.clone(),
                info_hash: id.clone(),
                name: p.name.clone(),
                state: if p.paused {
                    EngineTorrentState::Paused
                } else if p.error.is_some() {
                    EngineTorrentState::Error
                } else {
                    EngineTorrentState::Initializing
                },
                progress: 0.0,
                total_bytes: None,
                done_bytes: 0,
                uploaded_bytes: 0,
                down_bps: 0,
                up_bps: 0,
                peers: None,
                eta_s: None,
                error: p.error.clone(),
                output_dir: None,
            })
            .collect()
    }
}

#[derive(Deserialize)]
struct IpAnswer {
    ip: String,
    country: Option<String>,
    protected: Option<bool>,
}

/// Ask `url` (an [`IpCheck`] service) for the public address, through `interface` when set.
async fn fetch_public_ip(interface: Option<&str>, url: &str) -> anyhow::Result<IpInfo> {
    #[allow(unused_mut)]
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("PopcornStash/", env!("CARGO_PKG_VERSION")));
    if let Some(interface) = interface {
        builder = builder.interface(interface);
    }
    let client = builder.build()?;
    let answer: IpAnswer = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if answer.ip.parse::<std::net::IpAddr>().is_err() {
        anyhow::bail!("the IP service answered with something that is not an address");
    }
    Ok(IpInfo {
        ip: answer.ip,
        country: answer.country,
        protected: answer.protected,
        checked_at: now_unix(),
    })
}

impl TorrentBackend for Engine {
    async fn health(&self) -> EngineHealth {
        let now = now_unix();
        let inner = lock(&self.inner);
        let mut vpn = inner.tunnel.clone().unwrap_or_else(|| status::unknown(now));
        if now.saturating_sub(vpn.updated_at) > status::STALE_AFTER_SECS
            && vpn.state == TunnelState::Connected
        {
            vpn.state = TunnelState::Disconnected;
            vpn.error = Some("The VPN supervisor stopped reporting.".into());
        }
        if self.allow_no_vpn {
            vpn.kill_switch = false;
        }
        EngineHealth {
            vpn,
            engine: EngineState {
                state: if inner.running {
                    EngineRunState::Running
                } else {
                    EngineRunState::Blocked
                },
                reason: if inner.running {
                    None
                } else {
                    inner.reason.clone()
                },
                allow_no_vpn: self.allow_no_vpn,
            },
            public_ip: inner.ip.as_ref().map(|i| i.ip.clone()),
            country: inner.ip.as_ref().and_then(|i| i.country.clone()),
            protected: inner.ip.as_ref().and_then(|i| i.protected),
            checked_at: inner.ip.as_ref().map(|i| i.checked_at),
        }
    }

    async fn list(&self) -> Vec<EngineTorrent> {
        let mut torrents = match self.live_session().await {
            Some(session) => {
                let list: Vec<EngineTorrent> = session
                    .with_torrents(|iter| iter.map(|(_, handle)| engine_torrent(handle)).collect());
                let mut inner = lock(&self.inner);
                for t in &list {
                    inner.last_known.insert(t.id.clone(), t.clone());
                }
                list
            }
            None => self.persisted_torrents(),
        };
        for torrent in &mut torrents {
            if lock(&self.rules).contains_key(&torrent.id)
                && torrent.total_bytes.is_none()
                && torrent.state == EngineTorrentState::Paused
            {
                torrent.error =
                    Some("No files match these rules. Edit the rules or choose files.".into());
            }
        }
        let known: HashSet<String> = torrents.iter().map(|t| t.id.clone()).collect();
        torrents.extend(self.pending_entries(&known));
        torrents.sort_by(|a, b| a.id.cmp(&b.id));
        torrents
    }

    async fn files(&self, id: String) -> Result<TorrentFiles, BackendError> {
        let hash = id.parse().map_err(|_| BackendError::NotFound)?;
        if let Some(session) = self.live_session().await
            && let Some(handle) = session.get(TorrentIdOrHash::Hash(hash))
        {
            let selected = handle.only_files();
            let rule = lock(&self.rules).get(&id).cloned();
            return handle
                .with_metadata(|meta| TorrentFiles {
                    ready: true,
                    rules: rule.clone(),
                    files: meta
                        .file_infos
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| !f.attrs.padding)
                        .map(|(index, f)| {
                            let path = f.relative_filename.to_string_lossy().into_owned();
                            TorrentFile {
                                index,
                                kind: TorrentFileKind::of(&path),
                                path,
                                size: f.len,
                                selected: selected.as_ref().is_none_or(|ids| ids.contains(&index)),
                            }
                        })
                        .collect(),
                })
                .map_err(|_| {
                    BackendError::BadRequest(
                        "Fetching the torrent's file list. Try again shortly.".into(),
                    )
                });
        }
        if lock(&self.inner).pending.contains_key(&id) {
            return Ok(TorrentFiles::default());
        }
        if self
            .persisted_torrents()
            .iter()
            .any(|torrent| torrent.id == id)
        {
            // A stopped VPN session still owns a persisted selection. Reporting
            // 404 would make the server mistake this for an older, whole-file
            // engine and sort unselected boundary fragments without its manifest.
            return Err(BackendError::Blocked(MSG_VPN_WAIT.into()));
        }
        Err(BackendError::NotFound)
    }

    async fn select_files(&self, id: String, indices: Vec<usize>) -> Result<(), BackendError> {
        let _operation = self.pending_ops.lock().await;
        let session = self
            .live_session()
            .await
            .ok_or_else(|| BackendError::Blocked(MSG_VPN_WAIT.into()))?;
        let hash = id.parse().map_err(|_| BackendError::NotFound)?;
        let handle = session
            .get(TorrentIdOrHash::Hash(hash))
            .ok_or(BackendError::NotFound)?;
        let files = self.files(id.clone()).await?;
        let valid: HashSet<_> = files.files.iter().map(|f| f.index).collect();
        if indices.is_empty() || indices.iter().any(|i| !valid.contains(i)) {
            return Err(BackendError::BadRequest(
                "Choose at least one valid file.".into(),
            ));
        }
        session
            .update_only_files(&handle, &indices.into_iter().collect())
            .await
            .map_err(|_| {
                BackendError::BadRequest(
                    "Files cannot be changed while this torrent is checking. Try again shortly."
                        .into(),
                )
            })?;
        self.forget_rules(&id)
    }

    async fn add(&self, torrent: NewTorrent) -> Result<AddTorrentResponse, BackendError> {
        self.add_with_selection(torrent, None).await
    }

    async fn add_with_selection(
        &self,
        torrent: NewTorrent,
        file_types: Option<Vec<TorrentFileKind>>,
    ) -> Result<AddTorrentResponse, BackendError> {
        self.add_with_rules(torrent, file_types, None).await
    }

    async fn set_rules(
        &self,
        id: String,
        rules: stash_core::internal_api::TorrentFileRules,
    ) -> Result<(), BackendError> {
        self.apply_rules(id, rules).await
    }

    async fn add_with_rules(
        &self,
        torrent: NewTorrent,
        file_types: Option<Vec<TorrentFileKind>>,
        mut rules: Option<stash_core::internal_api::TorrentFileRules>,
    ) -> Result<AddTorrentResponse, BackendError> {
        if let Some(rules) = rules.as_mut() {
            rules.validate().map_err(BackendError::BadRequest)?;
            if rules.file_types.is_none() {
                rules.file_types = file_types.clone();
            }
        }
        let _pending_op = self.pending_ops.lock().await;
        let session = match self.live_session().await {
            Some(session) => session,
            None => {
                let blocked = {
                    let inner = lock(&self.inner);
                    !inner.running || inner.unprotected
                };
                if blocked {
                    return Err(BackendError::Blocked(MSG_VPN_WAIT.into()));
                }
                let status = status::read(&self.status_file);
                let ifindex = self
                    .decide(status.as_ref(), now_unix())
                    .map_err(BackendError::Blocked)?;
                self.start_session_locked(ifindex)
                    .await
                    .map_err(|e| BackendError::Internal(format!("{e:#}")))?;
                self.live_session().await.ok_or_else(|| {
                    BackendError::Internal("The torrent session did not start.".into())
                })?
            }
        };
        match torrent {
            NewTorrent::File(bytes) => {
                let (hash, name, single) = {
                    let meta = librqbit::torrent_from_bytes(&bytes).map_err(|_| {
                        BackendError::BadRequest("That is not a valid .torrent file.".into())
                    })?;
                    let name = meta
                        .info
                        .data
                        .name
                        .as_ref()
                        .map(|n| String::from_utf8_lossy(n.as_ref()).into_owned());
                    (meta.info_hash, name, meta.info.data.files.is_none())
                };
                let id = hash.as_string();
                if let Some(existing) = session.get(TorrentIdOrHash::Hash(hash)) {
                    return Ok(AddTorrentResponse {
                        id: id.clone(),
                        info_hash: id,
                        name: existing.name(),
                    });
                }
                let folder = self.unique_folder(&folder_name(name.as_deref(), single, &id));
                self.remember_rules(&id, rules.clone())?;
                let only_files = self
                    .rule_indices(&bytes, &id, file_types.as_deref())
                    .map_err(|_| {
                        BackendError::BadRequest("The torrent file list is not valid.".into())
                    })?;
                let options = AddTorrentOptions {
                    paused: only_files.as_ref().is_some_and(Vec::is_empty),
                    only_files,
                    sub_folder: Some(folder),
                    ..Default::default()
                };
                let response = session
                    .add_torrent(AddTorrent::from_bytes(bytes), Some(options))
                    .await
                    .map_err(|e| BackendError::Internal(format!("{e:#}")))?;
                let name = response
                    .into_handle()
                    .and_then(|h| {
                        let name = h.name();
                        self.watch_completion(&session, h);
                        name
                    })
                    .or(name);
                tracing::info!(id = %id, "torrent added");
                Ok(AddTorrentResponse {
                    id: id.clone(),
                    info_hash: id,
                    name,
                })
            }
            NewTorrent::Magnet(link) => {
                let magnet = Magnet::parse(&link).map_err(|_| {
                    BackendError::BadRequest("That is not a valid magnet link.".into())
                })?;
                let hash = magnet.as_id20().ok_or_else(|| {
                    BackendError::BadRequest(
                        "Only BitTorrent v1 magnet links are supported.".into(),
                    )
                })?;
                let id = hash.as_string();
                if let Some(existing) = session.get(TorrentIdOrHash::Hash(hash)) {
                    return Ok(AddTorrentResponse {
                        id: id.clone(),
                        info_hash: id,
                        name: existing.name(),
                    });
                }
                let name = magnet.name.clone();
                if !lock(&self.inner).pending.contains_key(&id) {
                    self.remember_rules(&id, rules)?;
                }
                let pending = {
                    let mut inner = lock(&self.inner);
                    let entry = inner
                        .pending
                        .entry(id.clone())
                        .or_insert_with(|| PendingAdd {
                            file_types: file_types.clone(),
                            magnet: link.clone(),
                            name: name.clone(),
                            paused: false,
                            error: None,
                            resolving: None,
                        });
                    // Retrying a failed magnet starts over.
                    entry.error = None;
                    inner.pending.clone()
                };
                self.save_pending(&pending);
                tracing::info!(id = %id, "magnet link added; fetching its details");
                Ok(AddTorrentResponse {
                    id: id.clone(),
                    info_hash: id,
                    name,
                })
            }
        }
    }

    async fn pause(&self, id: String) -> Result<(), BackendError> {
        let hash = parse_id(&id)?;
        let _pending_op = self.pending_ops.lock().await;
        if self.set_pending_paused(&id, true) {
            return Ok(());
        }
        match self.live_session().await {
            Some(session) => {
                let handle = session
                    .get(TorrentIdOrHash::Hash(hash))
                    .ok_or(BackendError::NotFound)?;
                if handle.is_paused() {
                    return Ok(());
                }
                session
                    .pause(&handle)
                    .await
                    .map_err(|e| BackendError::Internal(format!("{e:#}")))
            }
            None => self
                .edit_persisted(&id, |entry| {
                    entry.insert("is_paused".into(), serde_json::Value::Bool(true));
                    false
                })
                .map(drop),
        }
    }

    async fn resume(&self, id: String) -> Result<(), BackendError> {
        let hash = parse_id(&id)?;
        let _pending_op = self.pending_ops.lock().await;
        if self.set_pending_paused(&id, false) {
            return Ok(());
        }
        match self.live_session().await {
            Some(session) => {
                let handle = session
                    .get(TorrentIdOrHash::Hash(hash))
                    .ok_or(BackendError::NotFound)?;
                if !handle.is_paused() && !matches!(handle.stats().state, TorrentStatsState::Error)
                {
                    return Ok(());
                }
                session
                    .unpause(&handle)
                    .await
                    .map_err(|e| BackendError::Internal(format!("{e:#}")))
            }
            None => self
                .edit_persisted(&id, |entry| {
                    entry.insert("is_paused".into(), serde_json::Value::Bool(false));
                    false
                })
                .map(drop),
        }
    }

    async fn remove(&self, id: String, delete_files: bool) -> Result<(), BackendError> {
        let hash = parse_id(&id)?;
        let _pending_op = self.pending_ops.lock().await;
        self.forget_rules(&id)?;
        let was_pending = {
            let mut inner = lock(&self.inner);
            let removed = inner
                .pending
                .remove(&id)
                .map(|pending| {
                    if let Some(cancel) = pending.resolving {
                        cancel.cancel();
                    }
                })
                .is_some();
            if removed {
                let pending = inner.pending.clone();
                drop(inner);
                self.save_pending(&pending);
            }
            removed
        };
        match self.live_session().await {
            Some(session) => {
                if session.get(TorrentIdOrHash::Hash(hash)).is_none() {
                    return if was_pending {
                        Ok(())
                    } else {
                        Err(BackendError::NotFound)
                    };
                }
                session
                    .delete(TorrentIdOrHash::Hash(hash), delete_files)
                    .await
                    .map_err(|e| BackendError::Internal(format!("{e:#}")))?;
            }
            None => match self.edit_persisted(&id, |_| true) {
                Ok(output) => {
                    for ext in ["torrent", "bitv"] {
                        let _ = std::fs::remove_file(
                            self.state_dir.join(SESSION_DIR).join(format!("{id}.{ext}")),
                        );
                    }
                    if delete_files && let Some(output) = output {
                        self.delete_output(&output);
                    }
                }
                Err(BackendError::NotFound) if was_pending => {}
                Err(e) => return Err(e),
            },
        }
        lock(&self.inner).last_known.remove(&id);
        tracing::info!(id = %id, delete_files, "torrent removed");
        Ok(())
    }

    async fn set_limits(&self, limits: LimitsRequest) -> Result<(), BackendError> {
        let changed_trackers = {
            let mut inner = lock(&self.inner);
            let changed = inner.limits.add_public_trackers != limits.add_public_trackers;
            inner.limits = limits;
            changed
        };
        if changed_trackers {
            self.restart_trackers.store(true, Ordering::SeqCst);
        }
        self.seed_tx.send_replace(limits.seed);
        if let Ok(bytes) = serde_json::to_vec(&limits) {
            let _ = sys::write_atomic(&self.state_dir.join(LIMITS_FILE), &bytes, 0o600);
        }
        if let Some(session) = self.live_session().await {
            session
                .ratelimits
                .set_download_bps(kbps_to_bps(limits.down_kbps));
            session
                .ratelimits
                .set_upload_bps(kbps_to_bps(limits.up_kbps));
        }
        Ok(())
    }

    fn request_reload(&self) {
        if !self.reload_requested.swap(true, Ordering::SeqCst) {
            self.reload.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[tokio::test]
    async fn file_rules_select_before_transfer_persist_and_allow_manual_override() {
        use stash_core::internal_api::TorrentFileRules;
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir_all(source.join("Jane.Doe")).unwrap();
        for (path, size) in [
            ("Jane.Doe/film.mkv", 100),
            ("Jane.Doe/trailer.mkv", 100),
            ("Jane.Doe/photo.jpg", 100),
            ("Jane.Doe/tiny.mp4", 10),
            ("Other.mp4", 100),
        ] {
            std::fs::write(source.join(path), vec![0u8; size]).unwrap();
        }
        let bytes = librqbit::create_torrent(
            &source,
            Default::default(),
            &librqbit::spawn_utils::BlockingSpawner::new(1),
        )
        .await
        .unwrap()
        .as_bytes()
        .unwrap()
        .to_vec();
        let opts = options(dir.path());
        let engine = Engine::new(&opts);
        let session = selection_session(&opts).await;
        *engine.session.write().await = Some(Live {
            session: session.clone(),
            ifindex: None,
        });
        let rules = TorrentFileRules {
            file_types: Some(vec![TorrentFileKind::Video]),
            name_contains: vec!["jane doe".into()],
            name_excludes: vec!["trailer".into()],
            min_size: Some(50),
            max_size: Some(100),
        };
        let added = engine
            .add_with_rules(
                NewTorrent::File(bytes.clone()),
                Some(TorrentFileKind::media_default()),
                Some(rules.clone()),
            )
            .await
            .unwrap();
        let handle = session
            .get(TorrentIdOrHash::Hash(added.id.parse().unwrap()))
            .unwrap();
        wait_until("rule torrent checked", async || {
            !matches!(handle.stats().state, TorrentStatsState::Initializing { .. })
        })
        .await;
        let files = engine.files(added.id.clone()).await.unwrap();
        assert_eq!(files.rules, Some(rules.clone()));
        let chosen: Vec<_> = files.files.iter().filter(|f| f.selected).collect();
        assert_eq!(chosen.len(), 1);
        assert!(chosen[0].path.ends_with("film.mkv"));
        assert_eq!(
            Engine::new(&opts)
                .rule_indices(&bytes, &added.id, None)
                .unwrap(),
            handle.only_files()
        );
        assert!(
            engine
                .select_files(added.id.clone(), vec![usize::MAX])
                .await
                .is_err()
        );
        assert!(
            engine
                .files(added.id.clone())
                .await
                .unwrap()
                .rules
                .is_some()
        );
        let no_matches = TorrentFileRules {
            name_contains: vec!["nobody".into()],
            ..rules.clone()
        };
        engine
            .set_rules(added.id.clone(), no_matches.clone())
            .await
            .unwrap();
        assert!(handle.is_paused());
        assert_eq!(handle.only_files(), Some(vec![]));
        assert_eq!(
            engine.list().await[0].error.as_deref(),
            Some("No files match these rules. Edit the rules or choose files.")
        );
        session.stop().await;
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(engine.session_db()).unwrap()).unwrap();
        for t in persisted["torrents"].as_object_mut().unwrap().values_mut() {
            t["only_files"] = serde_json::Value::Null;
            t["is_paused"] = false.into();
        }
        std::fs::write(engine.session_db(), serde_json::to_vec(&persisted).unwrap()).unwrap();
        let restored = Engine::new(&opts);
        restored.reconcile_rules().unwrap();
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(engine.session_db()).unwrap()).unwrap();
        for t in persisted["torrents"].as_object().unwrap().values() {
            assert_eq!(t["only_files"], serde_json::json!([]));
            assert_eq!(t["is_paused"], true);
        }
        let session = selection_session(&opts).await;
        *restored.session.write().await = Some(Live {
            session: session.clone(),
            ifindex: None,
        });
        let handle = session
            .get(TorrentIdOrHash::Hash(added.id.parse().unwrap()))
            .unwrap();
        wait_until("restored rule torrent checked", async || {
            !matches!(handle.stats().state, TorrentStatsState::Initializing { .. })
        })
        .await;
        restored
            .select_files(added.id.clone(), vec![chosen[0].index])
            .await
            .unwrap();
        assert!(restored.files(added.id).await.unwrap().rules.is_none());
        assert!(handle.is_paused(), "manual selection preserves user pause");
        session.stop().await;
    }

    #[tokio::test]
    async fn unreadable_rules_block_session_instead_of_downloading_everything() {
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path());
        std::fs::create_dir_all(&opts.state_dir).unwrap();
        std::fs::write(opts.state_dir.join("file-rules.json"), b"invalid").unwrap();
        let engine = Engine::new(&opts);
        assert!(
            engine
                .start_session(None)
                .await
                .unwrap_err()
                .to_string()
                .contains("Saved file rules")
        );
        assert!(engine.live_session().await.is_none());
    }

    #[tokio::test]
    async fn selecting_nested_files_persists_without_restarting_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir_all(source.join("Season 1/Subs")).unwrap();
        for (path, data) in [
            ("Season 1/Episode.MKV", b"video".as_slice()),
            ("Season 1/Subs/Episode.srt", b"subtitle"),
            ("poster.jpg", b"image"),
            ("Instructions.txt", b"do not run me"),
        ] {
            std::fs::write(source.join(path), data).unwrap();
        }
        let bytes = librqbit::create_torrent(
            &source,
            Default::default(),
            &librqbit::spawn_utils::BlockingSpawner::new(1),
        )
        .await
        .unwrap()
        .as_bytes()
        .unwrap()
        .to_vec();
        let opts = options(dir.path());
        let engine = Engine::new(&opts);
        let session = selection_session(&opts).await;
        *engine.session.write().await = Some(Live {
            session: session.clone(),
            ifindex: None,
        });
        let added = engine
            .add_with_selection(
                NewTorrent::File(bytes.clone()),
                Some(TorrentFileKind::media_default()),
            )
            .await
            .unwrap();
        let handle = session
            .get(TorrentIdOrHash::Hash(added.id.parse().unwrap()))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while matches!(handle.stats().state, TorrentStatsState::Initializing { .. }) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let files = engine.files(added.id.clone()).await.unwrap();
        assert_eq!(files.files.iter().filter(|f| f.selected).count(), 2);
        assert!(
            files.files.iter().all(|f| f.selected
                == matches!(f.kind, TorrentFileKind::Video | TorrentFileKind::Subtitle))
        );
        let video = files
            .files
            .iter()
            .find(|f| f.kind == TorrentFileKind::Video)
            .unwrap()
            .index;
        engine
            .select_files(added.id.clone(), vec![video])
            .await
            .unwrap();
        assert_eq!(handle.only_files(), Some(vec![video]));
        let files = engine.files(added.id.clone()).await.unwrap();
        assert_eq!(files.files.iter().filter(|f| f.selected).count(), 1);
        assert!(
            engine
                .select_files(added.id, vec![usize::MAX])
                .await
                .is_err()
        );
        let saved =
            std::fs::read_to_string(opts.state_dir.join(SESSION_DIR).join("session.json")).unwrap();
        assert!(saved.contains("only_files"));
        session.stop().await;
        assert_eq!(
            selected_indices(&bytes, None).unwrap(),
            None,
            "old requests still select all files"
        );
    }

    #[tokio::test]
    async fn a_torrent_without_matching_types_waits_for_a_file_choice() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("instructions.txt");
        std::fs::write(&source, b"not a video").unwrap();
        let bytes = librqbit::create_torrent(
            &source,
            Default::default(),
            &librqbit::spawn_utils::BlockingSpawner::new(1),
        )
        .await
        .unwrap()
        .as_bytes()
        .unwrap()
        .to_vec();
        let opts = options(dir.path());
        let engine = Engine::new(&opts);
        let session = offline_session(&opts).await;
        *engine.session.write().await = Some(Live {
            session: session.clone(),
            ifindex: None,
        });
        let added = engine
            .add_with_selection(
                NewTorrent::File(bytes),
                Some(TorrentFileKind::media_default()),
            )
            .await
            .unwrap();
        let files = engine.files(added.id).await.unwrap();
        assert!(files.ready);
        assert!(files.files.iter().all(|f| !f.selected));
        let listed = engine.list().await;
        assert_eq!(listed[0].state, EngineTorrentState::Paused);
        assert_eq!(listed[0].done_bytes, 0);
        assert_eq!(listed[0].progress, 0.0);
        session.stop().await;
    }

    fn options(dir: &Path) -> EngineOptions {
        let downloads = dir.join("In Progress");
        let state = dir.join("engine");
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::create_dir_all(state.join(SESSION_DIR)).unwrap();
        EngineOptions {
            state_dir: state,
            downloads_dir: downloads,
            status_file: dir.join("status.json"),
            tunnel_if: Some("psw-test0".into()),
            allow_no_vpn: false,
            internal_token: "t".into(),
            api_bind: "127.0.0.1:0".parse().unwrap(),
            proxy_bind: "127.0.0.1:0".parse().unwrap(),
        }
    }

    const PENDING_HASH: &str = "1111111111111111111111111111111111111111";

    #[test]
    fn paused_file_check_is_reported_as_paused() {
        assert_eq!(
            torrent_state(&TorrentStatsState::Initializing { paused: true }, false),
            EngineTorrentState::Paused
        );
        assert_eq!(
            torrent_state(&TorrentStatsState::Initializing { paused: false }, false),
            EngineTorrentState::Initializing
        );
    }

    fn pending_fixture(options: &EngineOptions) {
        // Pre-existing pending.json files have no paused field.
        let pending = serde_json::json!({PENDING_HASH: {
            "magnet": format!("magnet:?xt=urn:btih:{PENDING_HASH}"),
            "name": "Synthetic metadata fixture"
        }});
        std::fs::write(
            options.state_dir.join(PENDING_FILE),
            serde_json::to_vec(&pending).unwrap(),
        )
        .unwrap();
    }

    async fn offline_session(options: &EngineOptions) -> Arc<Session> {
        Session::new_with_opts(
            options.downloads_dir.clone(),
            SessionOptions {
                dht: None,
                disable_trackers: true,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    /// Synthetic peers and trackers only; no DHT, LSD, public trackers or listeners.
    async fn selection_session(options: &EngineOptions) -> Arc<Session> {
        Session::new_with_opts(
            options.downloads_dir.clone(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ipv4_only: true,
                fastresume: true,
                persistence: Some(SessionPersistenceConfig::Json {
                    folder: Some(options.state_dir.join(SESSION_DIR)),
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn selected_nested_payload_resumes_after_restart_without_fetching_omitted_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("seed/pack");
        std::fs::create_dir_all(source.join("Season 1/Subs")).unwrap();
        let payload = (0..512 * 1024 + 137)
            .map(|i| (i % 251) as u8)
            .collect::<Vec<_>>();
        for (path, data) in [
            ("Season 1/Episode.MKV", payload.as_slice()),
            ("Season 1/Subs/Episode.srt", &[3_u8; 48 * 1024 + 11]),
            ("poster.jpg", &[4_u8; 128 * 1024 + 23]),
            ("Instructions.txt", &[5_u8; 128 * 1024 + 37]),
        ] {
            std::fs::write(source.join(path), data).unwrap();
        }
        let seeder = Session::new_with_opts(
            source.parent().unwrap().to_path_buf(),
            SessionOptions {
                dht: None,
                disable_trackers: true,
                disable_local_service_discovery: true,
                ipv4_only: true,
                listen: Some(ListenerOptions {
                    mode: ListenerMode::TcpOnly,
                    listen_addr: (Ipv4Addr::LOCALHOST, 0).into(),
                    ipv4_only: true,
                    ..Default::default()
                }),
                ratelimits: LimitsConfig {
                    download_bps: None,
                    upload_bps: kbps_to_bps(64),
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let tracker = one_peer_tracker(seeder.listen_addr().unwrap()).await;
        let bytes = librqbit::create_torrent(
            &source,
            librqbit::CreateTorrentOptions {
                name: None,
                trackers: vec![tracker],
                piece_length: Some(16 * 1024),
            },
            &librqbit::spawn_utils::BlockingSpawner::new(1),
        )
        .await
        .unwrap()
        .as_bytes()
        .unwrap();
        let seeded = seeder
            .add_torrent(
                AddTorrent::from_bytes(bytes.clone()),
                Some(AddTorrentOptions {
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_handle()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), seeded.wait_until_completed())
            .await
            .unwrap()
            .unwrap();
        let opts = options(&dir.path().join("client"));
        let engine = Engine::new(&opts);
        let session = selection_session(&opts).await;
        *engine.session.write().await = Some(Live {
            session: session.clone(),
            ifindex: None,
        });
        let added = engine
            .add_with_selection(
                NewTorrent::File(bytes.to_vec()),
                Some(TorrentFileKind::media_default()),
            )
            .await
            .unwrap();
        let hash = TorrentIdOrHash::Hash(added.id.parse().unwrap());
        let handle = session.get(hash).unwrap();
        wait_until("a verified selected piece", async || {
            handle.stats().progress_bytes > 0
                && !matches!(handle.stats().state, TorrentStatsState::Initializing { .. })
        })
        .await;
        engine.pause(added.id.clone()).await.unwrap();
        assert!(
            !handle.stats().finished,
            "the transfer was interrupted before completion"
        );
        let before = handle.stats().progress_bytes;
        let files = engine.files(added.id.clone()).await.unwrap();
        let video = files
            .files
            .iter()
            .find(|f| f.kind == TorrentFileKind::Video)
            .unwrap()
            .index;
        engine
            .select_files(added.id.clone(), vec![video])
            .await
            .unwrap();
        assert!(handle.is_paused(), "editing files preserves a user's pause");
        engine.stop_session().await;
        assert!(matches!(
            engine.files(added.id.clone()).await,
            Err(BackendError::Blocked(_))
        ));
        drop(handle);
        drop(session);

        let restored = Engine::new(&opts);
        let session = selection_session(&opts).await;
        *restored.session.write().await = Some(Live {
            session: session.clone(),
            ifindex: None,
        });
        let handle = session
            .get(TorrentIdOrHash::Hash(added.id.parse().unwrap()))
            .unwrap();
        wait_until("restored files to be checked", async || {
            !matches!(handle.stats().state, TorrentStatsState::Initializing { .. })
        })
        .await;
        assert!(handle.is_paused());
        assert_eq!(handle.only_files(), Some(vec![video]));
        assert!(
            handle.stats().progress_bytes > 0,
            "verified pieces survive a restart ({before} bytes before stopping)"
        );
        let selected = restored.files(added.id.clone()).await.unwrap();
        assert_eq!(selected.files.iter().filter(|f| f.selected).count(), 1);
        restored.resume(added.id).await.unwrap();
        seeder.ratelimits.set_upload_bps(None);
        wait_until("selected payload to finish", async || {
            handle.stats().finished
        })
        .await;
        assert_eq!(
            std::fs::read(handle.output_folder().join("Season 1/Episode.MKV")).unwrap(),
            payload
        );
        for file in selected
            .files
            .iter()
            .filter(|f| matches!(f.kind, TorrentFileKind::Image | TorrentFileKind::Other))
        {
            assert!(
                handle.stats().file_progress[file.index] < file.size,
                "omitted file {} must not transfer in full",
                file.path
            );
        }
        assert!(engine_torrent(&handle).done_bytes < seeded.stats().total_bytes);
        restored.stop_session().await;
        seeder.stop().await;
    }

    #[tokio::test]
    async fn pending_magnet_pause_survives_restart_and_blocked_resume() {
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path());
        pending_fixture(&opts);
        let engine = Arc::new(Engine::new(&opts));
        let session = offline_session(&opts).await;
        *engine.session.write().await = Some(Live {
            session,
            ifindex: None,
        });
        let cancel = Arc::new(CancellationToken::new());
        lock(&engine.inner)
            .pending
            .get_mut(PENDING_HASH)
            .unwrap()
            .resolving = Some(cancel.clone());

        engine.pause(PENDING_HASH.into()).await.unwrap();
        engine.pause(PENDING_HASH.into()).await.unwrap();
        assert!(cancel.is_cancelled());
        assert_eq!(engine.list().await[0].state, EngineTorrentState::Paused);
        engine.spawn_resolutions().await;
        assert!(
            lock(&engine.inner).pending[PENDING_HASH]
                .resolving
                .is_none()
        );
        engine.stop_session().await;

        let restarted = Engine::new(&opts);
        assert_eq!(restarted.list().await[0].state, EngineTorrentState::Paused);
        lock(&restarted.inner)
            .pending
            .get_mut(PENDING_HASH)
            .unwrap()
            .error = Some("old failure".into());
        restarted.resume(PENDING_HASH.into()).await.unwrap();
        restarted.resume(PENDING_HASH.into()).await.unwrap();
        assert!(lock(&restarted.inner).pending[PENDING_HASH].error.is_none());
        assert_eq!(
            Engine::new(&opts).list().await[0].state,
            EngineTorrentState::Initializing
        );
        assert_eq!(
            restarted
                .resume("0000000000000000000000000000000000000000".into())
                .await,
            Err(BackendError::NotFound)
        );
    }

    #[tokio::test]
    async fn pending_magnet_stale_resolution_cannot_replace_a_resumed_or_readded_entry() {
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path());
        pending_fixture(&opts);
        let engine = Engine::new(&opts);
        let session = offline_session(&opts).await;
        let old = Arc::new(CancellationToken::new());
        lock(&engine.inner)
            .pending
            .get_mut(PENDING_HASH)
            .unwrap()
            .resolving = Some(old.clone());
        engine.pause(PENDING_HASH.into()).await.unwrap();
        engine.resume(PENDING_HASH.into()).await.unwrap();
        let current = Arc::new(CancellationToken::new());
        lock(&engine.inner)
            .pending
            .get_mut(PENDING_HASH)
            .unwrap()
            .resolving = Some(current.clone());
        engine
            .finish_resolution(
                &session,
                PENDING_HASH,
                "",
                &old,
                Some(Err(anyhow::anyhow!("late failure"))),
            )
            .await;
        {
            let inner = lock(&engine.inner);
            let pending = &inner.pending[PENDING_HASH];
            assert!(pending.error.is_none());
            assert!(Arc::ptr_eq(pending.resolving.as_ref().unwrap(), &current));
        }
        engine.remove(PENDING_HASH.into(), false).await.unwrap();
        assert!(current.is_cancelled());
        pending_fixture(&opts);
        engine.load_saved_state();
        engine
            .finish_resolution(
                &session,
                PENDING_HASH,
                "",
                &current,
                Some(Err(anyhow::anyhow!("late failure"))),
            )
            .await;
        assert!(lock(&engine.inner).pending[PENDING_HASH].error.is_none());
        session.stop().await;
    }

    #[tokio::test]
    async fn magnet_rules_survive_metadata_wait_and_filter_before_payload() {
        use stash_core::internal_api::TorrentFileRules;
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path());
        let engine = Engine::new(&opts);
        let session = offline_session(&opts).await;
        *engine.session.write().await = Some(Live {
            session: session.clone(),
            ifindex: None,
        });
        let bytes = b"d4:infod6:lengthi4e4:name11:fixture.bin12:piece lengthi16384e6:pieces20:11111111111111111111ee";
        let hash = librqbit::torrent_from_bytes(bytes)
            .unwrap()
            .info_hash
            .as_string();
        let magnet = format!("magnet:?xt=urn:btih:{hash}");
        let rules = TorrentFileRules {
            min_size: Some(5),
            ..Default::default()
        };
        engine
            .add_with_rules(
                NewTorrent::Magnet(magnet.clone()),
                None,
                Some(rules.clone()),
            )
            .await
            .unwrap();
        assert!(
            engine.list().await[0].error.is_none(),
            "pending metadata is not a no-match error"
        );
        assert_eq!(file_rules::load(&opts.state_dir).unwrap()[&hash], rules);
        let current = Arc::new(CancellationToken::new());
        lock(&engine.inner)
            .pending
            .get_mut(&hash)
            .unwrap()
            .resolving = Some(current.clone());
        let metadata = session
            .add_torrent(
                AddTorrent::from_bytes(bytes.as_slice()),
                Some(AddTorrentOptions {
                    list_only: true,
                    ..Default::default()
                }),
            )
            .await;
        engine
            .finish_resolution(&session, &hash, &magnet, &current, Some(metadata))
            .await;
        let handle = session
            .get(TorrentIdOrHash::Hash(parse_id(&hash).unwrap()))
            .unwrap();
        assert_eq!(handle.only_files(), Some(vec![]));
        assert!(handle.is_paused());
        assert_eq!(engine.files(hash).await.unwrap().rules, Some(rules));
        engine.stop_session().await;
    }

    #[tokio::test]
    async fn pending_magnet_pause_before_and_after_metadata_handoff() {
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path());
        let engine = Engine::new(&opts);
        let session = offline_session(&opts).await;
        *engine.session.write().await = Some(Live {
            session: session.clone(),
            ifindex: None,
        });
        let bytes = b"d4:infod6:lengthi4e4:name11:fixture.bin12:piece lengthi16384e6:pieces20:11111111111111111111ee";
        let metadata = || {
            session.add_torrent(
                AddTorrent::from_bytes(bytes.as_slice()),
                Some(AddTorrentOptions {
                    list_only: true,
                    ..Default::default()
                }),
            )
        };
        let hash = librqbit::torrent_from_bytes(bytes)
            .unwrap()
            .info_hash
            .as_string();
        let magnet = format!("magnet:?xt=urn:btih:{hash}");
        engine
            .add(NewTorrent::Magnet(magnet.clone()))
            .await
            .unwrap();
        let old = Arc::new(CancellationToken::new());
        lock(&engine.inner)
            .pending
            .get_mut(&hash)
            .unwrap()
            .resolving = Some(old.clone());
        let resolved = metadata().await;
        engine.pause(hash.clone()).await.unwrap();
        engine
            .finish_resolution(&session, &hash, &magnet, &old, Some(resolved))
            .await;
        assert!(
            session
                .get(TorrentIdOrHash::Hash(parse_id(&hash).unwrap()))
                .is_none()
        );
        assert_eq!(std::fs::read_dir(&opts.downloads_dir).unwrap().count(), 0);

        engine.resume(hash.clone()).await.unwrap();
        let current = Arc::new(CancellationToken::new());
        lock(&engine.inner)
            .pending
            .get_mut(&hash)
            .unwrap()
            .resolving = Some(current.clone());
        engine
            .finish_resolution(&session, &hash, &magnet, &current, Some(metadata().await))
            .await;
        assert!(!lock(&engine.inner).pending.contains_key(&hash));
        engine.pause(hash).await.unwrap();
        assert_eq!(engine.list().await[0].state, EngineTorrentState::Paused);
        engine.stop_session().await;
    }

    #[tokio::test]
    async fn idle_session_stops_and_the_next_add_starts_it_without_losing_ready_health() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options(dir.path());
        opts.allow_no_vpn = true;
        let mut engine = Engine::new(&opts);
        engine.offline_sessions = true;
        engine.set_running(true, None);
        assert!(engine.live_session().await.is_none());
        let bytes = b"d4:infod6:lengthi4e4:name11:fixture.bin12:piece lengthi16384e6:pieces20:11111111111111111111ee";
        let first = engine.add(NewTorrent::File(bytes.to_vec())).await.unwrap();
        let session = engine.live_session().await.unwrap();
        assert_eq!(engine.session_options(None, false).peer_limit, Some(50));
        engine.stop_idle_session().await;
        assert!(engine.live_session().await.is_some());
        engine.remove(first.id.clone(), false).await.unwrap();
        engine.stop_idle_session().await;
        assert!(engine.live_session().await.is_none());
        assert!(engine.list().await.is_empty());
        assert_eq!(engine.health().await.engine.state, EngineRunState::Running);
        assert!(Engine::new(&opts).list().await.is_empty());
        let second = engine.add(NewTorrent::File(bytes.to_vec())).await.unwrap();
        assert_eq!(first.id, second.id);
        assert!(!Arc::ptr_eq(
            &session,
            &engine.live_session().await.unwrap()
        ));
        engine.stop_session().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn completed_loopback_download_pauses_unless_seeding_is_allowed() {
        for seed in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let seed_dir = dir.path().join("seed");
            std::fs::create_dir_all(&seed_dir).unwrap();
            let file = seed_dir.join("fixture.bin");
            std::fs::write(&file, vec![7_u8; 64 * 1024]).unwrap();
            let seeder = Session::new_with_opts(
                seed_dir,
                SessionOptions {
                    dht: None,
                    disable_trackers: true,
                    disable_local_service_discovery: true,
                    ipv4_only: true,
                    listen: Some(ListenerOptions {
                        mode: ListenerMode::TcpOnly,
                        listen_addr: (Ipv4Addr::LOCALHOST, 0).into(),
                        ipv4_only: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            let bytes = librqbit::create_torrent(
                &file,
                librqbit::CreateTorrentOptions {
                    name: None,
                    trackers: vec![],
                    piece_length: Some(16 * 1024),
                },
                &librqbit::spawn_utils::BlockingSpawner::new(1),
            )
            .await
            .unwrap()
            .as_bytes()
            .unwrap();
            let seeded = seeder
                .add_torrent(
                    AddTorrent::from_bytes(bytes.clone()),
                    Some(AddTorrentOptions {
                        overwrite: true,
                        ..Default::default()
                    }),
                )
                .await
                .unwrap()
                .into_handle()
                .unwrap();
            tokio::time::timeout(Duration::from_secs(10), seeded.wait_until_completed())
                .await
                .unwrap()
                .unwrap();
            let opts = options(&dir.path().join("engine"));
            let engine = Engine::new(&opts);
            engine
                .set_limits(LimitsRequest {
                    seed,
                    ..Default::default()
                })
                .await
                .unwrap();
            let leecher = offline_session(&opts).await;
            let handle = leecher
                .add_torrent(
                    AddTorrent::from_bytes(bytes),
                    Some(AddTorrentOptions {
                        initial_peers: Some(vec![seeder.listen_addr().unwrap()]),
                        ..Default::default()
                    }),
                )
                .await
                .unwrap()
                .into_handle()
                .unwrap();
            engine.watch_completion(&leecher, handle.clone());
            wait_until("the loopback download to finish", async || {
                handle.stats().finished
            })
            .await;
            if seed {
                assert!(
                    !handle.is_paused(),
                    "seeding remains available when enabled"
                );
                engine.set_limits(LimitsRequest::default()).await.unwrap();
            }
            wait_until("completion to stop uploading", async || handle.is_paused()).await;
            assert_eq!(engine_torrent(&handle).up_bps, 0);
            leecher.stop().await;
            seeder.stop().await;
        }
    }

    #[test]
    fn start_failures_explain_missing_ipv6() {
        let detail = "error creating UDP tracker client: error creating UDP socket at [::]:0: error creating socket: Address family not supported by protocol (os error 97)";
        assert!(start_failure_reason(detail).contains("IPv6"));
        assert!(start_failure_reason("boom").contains("Check the logs"));
    }

    #[test]
    fn folder_names_are_single_safe_components() {
        assert_eq!(
            folder_name(Some("Big.Movie.2019.1080p.mkv"), true, "h"),
            "Big.Movie.2019.1080p"
        );
        assert_eq!(folder_name(Some("Show S01"), false, "h"), "Show S01");
        assert_eq!(folder_name(Some("../../etc"), false, "h"), "_.._etc");
        assert_eq!(folder_name(Some("..."), false, "h"), "h");
        assert_eq!(folder_name(Some(".hidden"), false, "h"), "hidden");
        assert_eq!(folder_name(Some("a\nb\\c"), false, "h"), "a_b_c");
        assert_eq!(folder_name(None, false, "abc"), "abc");
        assert_eq!(
            folder_name(Some("ubuntu-24.04.iso"), true, "h"),
            "ubuntu-24.04"
        );
        assert!(folder_name(Some(&"é".repeat(300)), false, "h").len() <= 200);
    }

    #[tokio::test]
    async fn decide_follows_status_file() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::new(&options(dir.path()));
        let now = now_unix();
        assert_eq!(engine.decide(None, now), Err(MSG_VPN_WAIT.to_string()));
        let mut status = TunnelStatus::not_configured(now);
        assert_eq!(
            engine.decide(Some(&status), now),
            Err(MSG_NOT_CONFIGURED.to_string())
        );
        status.state = TunnelState::Error;
        status.error = Some("The VPN server rejected the username or password.".into());
        assert_eq!(
            engine.decide(Some(&status), now).unwrap_err(),
            "The VPN server rejected the username or password."
        );
        // Connected, but the interface does not exist here: still blocked.
        status.state = TunnelState::Connected;
        assert_eq!(
            engine.decide(Some(&status), now),
            Err(MSG_VPN_WAIT.to_string())
        );
        // Connected on an interface that exists.
        let mut opts = options(dir.path());
        opts.tunnel_if = Some("lo".into());
        let engine = Engine::new(&opts);
        assert!(matches!(engine.decide(Some(&status), now), Ok(Some(_))));
        // Stale status: blocked.
        status.updated_at = now - 120;
        assert!(engine.decide(Some(&status), now).is_err());
        // allow_no_vpn ignores the tunnel.
        opts.allow_no_vpn = true;
        let engine = Engine::new(&opts);
        assert_eq!(engine.decide(None, now), Ok(None));
    }

    #[tokio::test]
    async fn blocked_engine_refuses_adds_and_edits_persisted_state() {
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path());
        let hash = "c9e15763f722f23e98a29decdfae341b98d53056";
        let folder = opts.downloads_dir.join("ubuntu");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("ubuntu.iso"), b"x").unwrap();
        let db = serde_json::json!({"torrents": {"0": {"info_hash": hash, "trackers": [], "output_folder": folder, "only_files": null, "is_paused": false}}});
        std::fs::write(
            opts.state_dir.join(SESSION_DIR).join("session.json"),
            serde_json::to_vec(&db).unwrap(),
        )
        .unwrap();
        let engine = Engine::new(&opts);

        let err = engine
            .add(NewTorrent::Magnet(format!("magnet:?xt=urn:btih:{hash}")))
            .await
            .unwrap_err();
        assert_eq!(err, BackendError::Blocked(MSG_VPN_WAIT.into()));
        let health = engine.health().await;
        assert_eq!(health.engine.state, EngineRunState::Blocked);

        let list = engine.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, hash);
        assert_eq!(list[0].state, EngineTorrentState::Paused);
        assert_eq!(list[0].name.as_deref(), Some("ubuntu"));

        engine.pause(hash.into()).await.unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(engine.session_db()).unwrap()).unwrap();
        assert_eq!(saved["torrents"]["0"]["is_paused"], true);
        engine.resume(hash.into()).await.unwrap();
        assert_eq!(
            engine
                .pause("0000000000000000000000000000000000000000".into())
                .await,
            Err(BackendError::NotFound)
        );

        engine.remove(hash.into(), true).await.unwrap();
        assert!(engine.list().await.is_empty());
        assert!(!folder.exists());
        assert!(opts.downloads_dir.exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reload_ends_the_loop_promptly_with_exit_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options(dir.path());
        opts.tunnel_if = Some("lo".into());
        let mut status = TunnelStatus::not_configured(now_unix());
        status.state = TunnelState::Connected;
        crate::status::write(&opts.status_file, &status).unwrap();
        let engine = Arc::new(Engine::new(&opts));
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(engine.clone().supervise(shutdown.clone()));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            *engine.tunnel_usable().borrow(),
            "the proxy follows the tunnel state"
        );
        let asked = Instant::now();
        engine.request_reload();
        let code = tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("the loop ended")
            .unwrap();
        assert_eq!(code, EXIT_RELOAD);
        assert!(
            asked.elapsed() < Duration::from_secs(5),
            "took {:?}",
            asked.elapsed()
        );
        assert!(!*engine.tunnel_usable().borrow());
        assert_eq!(engine.health().await.engine.state, EngineRunState::Blocked);
    }

    #[tokio::test]
    async fn limits_persist() {
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path());
        let engine = Engine::new(&opts);
        engine
            .set_limits(LimitsRequest {
                down_kbps: 100,
                up_kbps: 5,
                seed: false,
                add_public_trackers: false,
            })
            .await
            .unwrap();
        let engine = Engine::new(&opts);
        assert_eq!(
            lock(&engine.inner).limits,
            LimitsRequest {
                down_kbps: 100,
                up_kbps: 5,
                seed: false,
                add_public_trackers: false,
            }
        );
        assert_eq!(kbps_to_bps(0), None);
        assert_eq!(kbps_to_bps(100).map(NonZeroU32::get), Some(100 * 1024));
        assert_eq!(kbps_to_bps(u64::MAX).map(NonZeroU32::get), Some(u32::MAX));
    }

    /// librqbit waits for one second's allowance to cover a whole 16 KiB block; with a
    /// smaller limit the wait fails and the transfer stops (a 5 KiB/s upload limit used
    /// to stop all uploads). Limits below the block size count as 16 KiB/s.
    #[tokio::test]
    async fn limits_below_one_block_still_move_blocks() {
        let block = NonZeroU32::new(16 * 1024).unwrap();
        for kbps in [1, 5, 15, 16, 100] {
            let limits = librqbit::limits::Limits::new(LimitsConfig {
                download_bps: kbps_to_bps(kbps),
                upload_bps: kbps_to_bps(kbps),
            });
            let both = async {
                limits.prepare_for_upload(block).await?;
                limits.prepare_for_download(block).await
            };
            tokio::time::timeout(Duration::from_secs(5), both)
                .await
                .unwrap_or_else(|_| panic!("{kbps} KiB/s: a block did not get through in time"))
                .unwrap_or_else(|e| panic!("{kbps} KiB/s: a block was refused: {e:#}"));
        }
        assert_eq!(kbps_to_bps(1).map(NonZeroU32::get), Some(16 * 1024));
        assert_eq!(kbps_to_bps(15).map(NonZeroU32::get), Some(16 * 1024));
        assert_eq!(kbps_to_bps(16).map(NonZeroU32::get), Some(16 * 1024));
    }

    /// An HTTP tracker that answers every announce with `peer` and asks for the next
    /// announce in one second.
    async fn one_peer_tracker(peer: SocketAddr) -> String {
        let SocketAddr::V4(peer) = peer else {
            panic!("the test tracker lists IPv4 peers only")
        };
        let mut body = b"d8:intervali1e5:peers6:".to_vec();
        body.extend_from_slice(&peer.ip().octets());
        body.extend_from_slice(&peer.port().to_be_bytes());
        body.push(b'e');
        let app = axum::Router::new().route(
            "/announce",
            axum::routing::get(move || std::future::ready(body.clone())),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        format!("http://{addr}/announce")
    }

    /// Metadata lookup must identify itself as a downloader, and must retry a peer
    /// that was unavailable when the tracker first listed it.
    async fn metadata_tracker_fixture(require_left: bool, start_paused: bool, global_only: bool) {
        let dir = tempfile::tempdir().unwrap();
        let seed_dir = dir.path().join("seed");
        let leech_dir = dir.path().join("leech");
        std::fs::create_dir_all(&seed_dir).unwrap();
        std::fs::create_dir_all(&leech_dir).unwrap();
        let film = seed_dir.join("metadata-fixture.bin");
        std::fs::write(&film, vec![42u8; 32 * 1024]).unwrap();
        let offline = || SessionOptions {
            dht: None,
            disable_local_service_discovery: true,
            ipv4_only: true,
            ..Default::default()
        };
        let seeder = Session::new_with_opts(
            seed_dir,
            SessionOptions {
                disable_trackers: true,
                listen: Some(ListenerOptions {
                    mode: ListenerMode::TcpOnly,
                    listen_addr: (Ipv4Addr::LOCALHOST, 0).into(),
                    ipv4_only: true,
                    ..Default::default()
                }),
                ..offline()
            },
        )
        .await
        .unwrap();
        let SocketAddr::V4(peer) = seeder.listen_addr().unwrap() else {
            panic!("IPv4 fixture")
        };
        let announces = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let observed = announces.clone();
        let mut body = b"d8:intervali1e5:peers6:".to_vec();
        body.extend_from_slice(&peer.ip().octets());
        body.extend_from_slice(&peer.port().to_be_bytes());
        body.push(b'e');
        let app = axum::Router::new().route(
            "/announce",
            axum::routing::get(
                move |axum::extract::Query(query): axum::extract::Query<
                    HashMap<String, String>,
                >| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    let response =
                        if require_left && query.get("left").is_none_or(|left| left == "0") {
                            b"d8:intervali1e5:peers0:e".to_vec()
                        } else {
                            body.clone()
                        };
                    std::future::ready(response)
                },
            ),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tracker = format!("http://{}/announce", listener.local_addr().unwrap());
        let tracker_task = tokio::spawn(async move { axum::serve(listener, app).await });
        let torrent = librqbit::create_torrent(
            &film,
            librqbit::CreateTorrentOptions {
                name: None,
                trackers: vec![tracker.clone()],
                piece_length: Some(16 * 1024),
            },
            &librqbit::spawn_utils::BlockingSpawner::new(1),
        )
        .await
        .unwrap()
        .as_bytes()
        .unwrap();
        let seeded = seeder
            .add_torrent(
                AddTorrent::from_bytes(torrent),
                Some(AddTorrentOptions {
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_handle()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), seeded.wait_until_completed())
            .await
            .unwrap()
            .unwrap();
        if start_paused {
            seeder.pause(&seeded).await.unwrap();
        }
        let leecher = Session::new_with_opts(
            leech_dir,
            SessionOptions {
                listen: None,
                trackers: if global_only {
                    [url::Url::parse(&tracker).unwrap()].into()
                } else {
                    HashSet::new()
                },
                ..offline()
            },
        )
        .await
        .unwrap();
        let magnet = if global_only {
            format!("magnet:?xt=urn:btih:{}", seeded.info_hash().as_string())
        } else {
            format!(
                "magnet:?xt=urn:btih:{}&tr={tracker}",
                seeded.info_hash().as_string()
            )
        };
        let resolving = {
            let leecher = leecher.clone();
            tokio::spawn(async move {
                leecher
                    .add_torrent(
                        AddTorrent::from_url(&magnet),
                        Some(AddTorrentOptions {
                            list_only: true,
                            ..Default::default()
                        }),
                    )
                    .await
            })
        };
        if start_paused {
            tokio::time::timeout(Duration::from_secs(10), async {
                while announces.load(Ordering::SeqCst) < 3 {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await
            .expect("the paused peer was listed repeatedly");
            assert!(
                !resolving.is_finished(),
                "a paused peer cannot serve metadata"
            );
            seeder.unpause(&seeded).await.unwrap();
        }
        let result = tokio::time::timeout(Duration::from_secs(15), resolving).await;
        // Stop the fixture even when the regression fails.
        leecher.stop().await;
        seeder.stop().await;
        tracker_task.abort();
        let result = result
            .expect("metadata did not recover from the tracker")
            .unwrap()
            .unwrap();
        assert!(matches!(result, RqAddResponse::ListOnly(_)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn metadata_lookup_announces_missing_bytes_to_trackers() {
        metadata_tracker_fixture(true, false, false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn metadata_lookup_retries_a_peer_that_comes_back() {
        metadata_tracker_fixture(false, true, false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn extra_tracker_resolves_a_magnet_without_its_own_tracker_or_dht() {
        metadata_tracker_fixture(true, false, true).await;
    }

    #[tokio::test]
    async fn public_tracker_policy_is_opt_in_and_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path());
        let engine = Engine::new(&opts);
        lock(&engine.public_trackers).finish_refresh(
            &opts.state_dir,
            Ok(vec!["https://tracker.example/announce".into()]),
        );
        assert!(engine.session_options(None, false).trackers.is_empty());
        engine
            .set_limits(LimitsRequest {
                add_public_trackers: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(engine.restart_trackers.load(Ordering::SeqCst));
        assert_eq!(engine.session_options(None, false).trackers.len(), 1);
        let restored = Engine::new(&opts);
        assert_eq!(restored.session_options(None, false).trackers.len(), 1);
        restored.set_limits(LimitsRequest::default()).await.unwrap();
        assert!(restored.session_options(None, false).trackers.is_empty());
    }

    #[tokio::test]
    async fn tracker_reconnect_preserves_paused_files_and_pending_magnets() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options(dir.path());
        opts.allow_no_vpn = true;
        let mut engine = Engine::new(&opts);
        engine.offline_sessions = true;
        engine.set_running(true, None);
        let bytes = b"d4:infod6:lengthi4e4:name11:fixture.bin12:piece lengthi16384e6:pieces20:11111111111111111111ee";
        let file = engine.add(NewTorrent::File(bytes.to_vec())).await.unwrap();
        engine.pause(file.id.clone()).await.unwrap();
        let magnet = engine
            .add(NewTorrent::Magnet(format!(
                "magnet:?xt=urn:btih:{PENDING_HASH}"
            )))
            .await
            .unwrap();
        engine.pause(magnet.id.clone()).await.unwrap();
        for enabled in [true, false] {
            engine
                .set_limits(LimitsRequest {
                    add_public_trackers: enabled,
                    ..Default::default()
                })
                .await
                .unwrap();
            assert!(engine.restart_trackers.swap(false, Ordering::SeqCst));
            engine.stop_session().await;
            engine.start_session(None).await.unwrap();
            let rows = engine.list().await;
            assert_eq!(rows.len(), 2);
            for id in [&file.id, &magnet.id] {
                let row = rows.iter().find(|row| &row.id == id).unwrap();
                assert_eq!(row.state, EngineTorrentState::Paused);
                assert_eq!(row.done_bytes, 0);
            }
        }
        engine.stop_session().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn private_torrents_never_announce_to_the_session_public_tracker() {
        for private in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let original = Arc::new(AtomicU32::new(0));
            let extra = Arc::new(AtomicU32::new(0));
            let (a, b) = (original.clone(), extra.clone());
            let app = axum::Router::new()
                .route(
                    "/original",
                    axum::routing::get(move || {
                        a.fetch_add(1, Ordering::SeqCst);
                        std::future::ready("d8:intervali1e5:peers0:e")
                    }),
                )
                .route(
                    "/extra",
                    axum::routing::get(move || {
                        b.fetch_add(1, Ordering::SeqCst);
                        std::future::ready("d8:intervali1e5:peers0:e")
                    }),
                );
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let task = tokio::spawn(async {
                axum::serve(listener, app).await.unwrap();
            });
            let tracker = format!("{base}/original");
            let mut bytes = format!("d8:announce{}:{tracker}4:infod6:lengthi16384e4:name11:fixture.bin12:piece lengthi16384e6:pieces20:", tracker.len()).into_bytes();
            bytes.extend_from_slice(&[0; 20]);
            if private {
                bytes.extend_from_slice(b"7:privatei1e");
            }
            bytes.extend_from_slice(b"ee");
            let session = Session::new_with_opts(
                dir.path().to_path_buf(),
                SessionOptions {
                    dht: None,
                    listen: None,
                    disable_local_service_discovery: true,
                    ipv4_only: true,
                    trackers: [url::Url::parse(&format!("{base}/extra")).unwrap()].into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            session
                .add_torrent(AddTorrent::from_bytes(bytes), None)
                .await
                .unwrap();
            let announced = tokio::time::timeout(Duration::from_secs(10), async {
                while original.load(Ordering::SeqCst) == 0
                    || (!private && extra.load(Ordering::SeqCst) == 0)
                {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            })
            .await;
            session.stop().await;
            task.abort();
            announced.expect("original tracker (and public extra when allowed) must be contacted");
            assert_eq!(extra.load(Ordering::SeqCst) > 0, !private);
        }
    }

    /// A lone seeder that goes away and comes back is reconnected as soon as the tracker
    /// lists it again (the vendored librqbit patch, vendor/librqbit/PATCHED.md). Upstream
    /// librqbit ignores addresses it already knows and waits out a reconnect backoff that
    /// grows to an hour, so the download sat at zero peers. The leecher has no listener,
    /// like the engine behind the VPN's NAT: only it can open the connection.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_seeder_that_comes_back_is_reconnected_while_the_tracker_lists_it() {
        let dir = tempfile::tempdir().unwrap();
        let (seed_dir, leech_dir) = (dir.path().join("seed"), dir.path().join("leech"));
        std::fs::create_dir_all(&seed_dir).unwrap();
        std::fs::create_dir_all(&leech_dir).unwrap();
        let film = seed_dir.join("film.bin");
        // 64 pieces at 16 KiB/s: the download is far from done when the seeder pauses.
        std::fs::write(
            &film,
            (0..1024 * 1024)
                .map(|i| (i % 251) as u8)
                .collect::<Vec<_>>(),
        )
        .unwrap();

        let seeder = Session::new_with_opts(
            seed_dir.clone(),
            SessionOptions {
                dht: None,
                disable_trackers: true,
                listen: Some(ListenerOptions {
                    mode: ListenerMode::TcpOnly,
                    listen_addr: (Ipv4Addr::LOCALHOST, 0).into(),
                    ipv4_only: true,
                    ..Default::default()
                }),
                disable_local_service_discovery: true,
                ipv4_only: true,
                ratelimits: LimitsConfig {
                    download_bps: None,
                    upload_bps: kbps_to_bps(16),
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let seeder_addr = seeder.listen_addr().expect("the seeder listens");
        let tracker = one_peer_tracker(seeder_addr).await;
        let torrent = librqbit::create_torrent(
            &film,
            librqbit::CreateTorrentOptions {
                name: None,
                trackers: vec![tracker],
                piece_length: Some(16 * 1024),
            },
            &librqbit::spawn_utils::BlockingSpawner::new(1),
        )
        .await
        .unwrap()
        .as_bytes()
        .unwrap();
        let seeded = seeder
            .add_torrent(
                AddTorrent::from_bytes(torrent.clone()),
                Some(AddTorrentOptions {
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_handle()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(30), seeded.wait_until_completed())
            .await
            .expect("the seeder checked its file in time")
            .unwrap();

        let leecher = Session::new_with_opts(
            leech_dir,
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ipv4_only: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let leeching = leecher
            .add_torrent(AddTorrent::from_bytes(torrent), None)
            .await
            .unwrap()
            .into_handle()
            .unwrap();
        let done = || leeching.stats().progress_bytes;
        // The seeder's entry in the leecher's peer table.
        let seeder_entry = || {
            let snapshot = leeching
                .live()
                .map(|live| {
                    live.per_peer_stats_snapshot(
                        serde_json::from_value(serde_json::json!({"state": "all"})).unwrap(),
                    )
                })
                .map(|s| serde_json::to_value(&s).unwrap());
            snapshot
                .and_then(|s| s["peers"].get(seeder_addr.to_string()).cloned())
                .unwrap_or_default()
        };
        async fn wait_for(what: &str, limit: Duration, mut ready: impl FnMut() -> bool) {
            let deadline = Instant::now() + limit;
            while !ready() {
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        wait_for("the first piece", Duration::from_secs(30), || done() > 0).await;

        // The seeder goes away: the connection drops, and the paused seeder turns the
        // leecher's first retry away. Upstream would now wait 60 to 120 s before the
        // next try, then 6 to 12 minutes, while the tracker keeps listing the seeder.
        seeder.pause(&seeded).await.unwrap();
        wait_for(
            "the leecher to fail twice on the paused seeder",
            Duration::from_secs(60),
            || seeder_entry()["counters"]["errors"].as_u64() >= Some(2),
        )
        .await;
        let before = done();
        seeder.unpause(&seeded).await.unwrap();
        let resumed = Instant::now();
        let deadline = resumed + Duration::from_secs(40);
        while done() <= before {
            assert!(
                Instant::now() < deadline,
                "no new data 40 s after the seeder came back; its entry in the leecher: {}",
                seeder_entry()
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        eprintln!(
            "reconnected {:?} after the seeder came back",
            resumed.elapsed()
        );
        leecher.stop().await;
        seeder.stop().await;
    }

    /// A fake public-address service on 127.0.0.1 that answers `/nordvpn` like NordVPN's
    /// insights and `/ipify` like ipify, with whatever `answer` currently holds.
    async fn fake_ip_service(answer: Arc<Mutex<serde_json::Value>>) -> (String, Arc<AtomicU32>) {
        let hits = Arc::new(AtomicU32::new(0));
        let reply = |answer: Arc<Mutex<serde_json::Value>>, hits: Arc<AtomicU32>| {
            move || {
                hits.fetch_add(1, Ordering::SeqCst);
                let body = lock(&answer).clone();
                std::future::ready(axum::Json(body))
            }
        };
        let app = axum::Router::new()
            .route(
                "/nordvpn",
                axum::routing::get(reply(answer.clone(), hits.clone())),
            )
            .route("/ipify", axum::routing::get(reply(answer, hits.clone())))
            .route(
                "/broken",
                axum::routing::get(|| async { (axum::http::StatusCode::BAD_GATEWAY, "no") }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        (format!("http://{addr}"), hits)
    }

    /// The proof of protection reads NordVPN's insights and ipify's answers, and refuses
    /// an answer that is not an address or a service that fails.
    #[tokio::test]
    async fn public_ip_answers_are_read_and_checked() {
        let answer = Arc::new(Mutex::new(serde_json::json!({
            "ip": "185.153.177.12", "country": "Netherlands", "protected": true, "isp": "NordVPN"
        })));
        let (base, _) = fake_ip_service(answer.clone()).await;
        let check = IpCheck {
            nordvpn_url: format!("{base}/nordvpn"),
            other_url: format!("{base}/ipify"),
            ..IpCheck::default()
        };
        assert_eq!(check.url(Some(VpnProvider::Nordvpn)), check.nordvpn_url);
        assert_eq!(check.url(Some(VpnProvider::Wireguard)), check.other_url);
        assert_eq!(check.url(None), check.other_url);
        assert_eq!(IpCheck::default().nordvpn_url, NORDVPN_INSIGHTS);
        assert_eq!(IpCheck::default().other_url, IPIFY);

        let nord = fetch_public_ip(None, check.url(Some(VpnProvider::Nordvpn)))
            .await
            .unwrap();
        assert_eq!(nord.ip, "185.153.177.12");
        assert_eq!(nord.country.as_deref(), Some("Netherlands"));
        assert_eq!(nord.protected, Some(true));
        assert!(nord.checked_at > 0);

        *lock(&answer) = serde_json::json!({"ip": "2001:db8::7"});
        let ipify = fetch_public_ip(None, check.url(None)).await.unwrap();
        assert_eq!(ipify.ip, "2001:db8::7");
        assert_eq!((ipify.country, ipify.protected), (None, None));

        *lock(&answer) = serde_json::json!({"ip": "<script>"});
        assert!(fetch_public_ip(None, &check.other_url).await.is_err());
        *lock(&answer) = serde_json::json!({"address": "203.0.113.5"});
        assert!(fetch_public_ip(None, &check.other_url).await.is_err());
        assert!(
            fetch_public_ip(None, &format!("{base}/broken"))
                .await
                .is_err()
        );
    }

    /// When the check says the address is not a VPN address (NordVPN `protected: false`),
    /// torrents stop and stay stopped until a later check passes, as the UI says.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unprotected_address_blocks_torrents_until_a_check_passes() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options(dir.path());
        opts.tunnel_if = Some("lo".into());
        let answer = Arc::new(Mutex::new(serde_json::json!({
            "ip": "198.51.100.23", "country": "Netherlands", "protected": false
        })));
        let (base, hits) = fake_ip_service(answer.clone()).await;
        let engine = Arc::new(Engine::new(&opts).with_ip_check(IpCheck {
            nordvpn_url: format!("{base}/nordvpn"),
            other_url: format!("{base}/ipify"),
            interval: Duration::from_secs(600),
            recheck_unprotected: Duration::from_millis(300),
        }));
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(engine.clone().supervise(shutdown.clone()));
        // The status file is rewritten like the supervisor does, so it never goes stale.
        let status_file = opts.status_file.clone();
        let writer_stop = shutdown.clone();
        tokio::spawn(async move {
            while !writer_stop.is_cancelled() {
                let mut status = TunnelStatus::not_configured(now_unix());
                status.state = TunnelState::Connected;
                status.provider = Some(VpnProvider::Nordvpn);
                crate::status::write(&status_file, &status).unwrap();
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });
        async fn wait_for(what: &str, mut ready: impl AsyncFnMut() -> bool) {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !ready().await {
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        wait_for("the not-protected answer to block torrents", async || {
            let health = engine.health().await;
            health.protected == Some(false) && health.engine.state == EngineRunState::Blocked
        })
        .await;
        let health = engine.health().await;
        assert_eq!(health.engine.reason.as_deref(), Some(MSG_NOT_PROTECTED));
        assert_eq!(health.public_ip.as_deref(), Some("198.51.100.23"));
        assert_eq!(health.vpn.state, TunnelState::Connected);
        assert!(engine.live_session().await.is_none(), "the session stopped");
        assert!(matches!(
            engine
                .add(NewTorrent::Magnet(
                    "magnet:?xt=urn:btih:c9e15763f722f23e98a29decdfae341b98d53056".into()
                ))
                .await,
            Err(BackendError::Blocked(_))
        ));
        // It keeps checking while blocked, and stays blocked while the answer stays.
        let seen = hits.load(Ordering::SeqCst);
        wait_for("another check", async || hits.load(Ordering::SeqCst) > seen).await;
        assert_eq!(engine.health().await.engine.state, EngineRunState::Blocked);

        // A later check passes: torrents run again.
        *lock(&answer) = serde_json::json!({
            "ip": "185.153.177.12", "country": "Netherlands", "protected": true
        });
        wait_for("torrents to run again", async || {
            engine.health().await.engine.state == EngineRunState::Running
        })
        .await;
        wait_for("the passing answer", async || {
            engine.health().await.protected == Some(true)
        })
        .await;
        assert!(
            engine.live_session().await.is_none(),
            "ready but idle: no listeners are needed"
        );
        shutdown.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
    }

    /// Writes the supervisor's status file every 200 ms with whatever state the test sets,
    /// so it never goes stale.
    fn fake_supervisor(status_file: PathBuf, stop: CancellationToken) -> Arc<Mutex<TunnelState>> {
        let state = Arc::new(Mutex::new(TunnelState::Connected));
        let shared = state.clone();
        tokio::spawn(async move {
            while !stop.is_cancelled() {
                let mut status = TunnelStatus::not_configured(now_unix());
                status.state = *lock(&shared);
                status.provider = Some(VpnProvider::Nordvpn);
                crate::status::write(&status_file, &status).unwrap();
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });
        state
    }

    async fn wait_until(what: &str, mut ready: impl AsyncFnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !ready().await {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// A "not protected" answer survives a short tunnel drop: while the tunnel is back but
    /// torrents are still stopped, the status keeps saying not protected (never protected
    /// with no answer), and the address is checked again as soon as the tunnel is back
    /// rather than after the one-minute wait.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_tunnel_drop_keeps_the_not_protected_answer_and_checks_again_on_reconnect() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options(dir.path());
        opts.tunnel_if = Some("lo".into());
        let answer = Arc::new(Mutex::new(serde_json::json!({
            "ip": "198.51.100.23", "country": "Netherlands", "protected": false
        })));
        let (base, hits) = fake_ip_service(answer.clone()).await;
        // The production waits: a recheck would not come within the test.
        let engine = Arc::new(Engine::new(&opts).with_ip_check(IpCheck {
            nordvpn_url: format!("{base}/nordvpn"),
            other_url: format!("{base}/ipify"),
            ..IpCheck::default()
        }));
        let shutdown = CancellationToken::new();
        let tunnel = fake_supervisor(opts.status_file.clone(), shutdown.clone());
        let task = tokio::spawn(engine.clone().supervise(shutdown.clone()));

        wait_until("the not-protected answer to block torrents", async || {
            let health = engine.health().await;
            health.protected == Some(false) && health.engine.state == EngineRunState::Blocked
        })
        .await;
        let checks = hits.load(Ordering::SeqCst);

        // The tunnel drops for a moment, then comes back.
        *lock(&tunnel) = TunnelState::Connecting;
        wait_until("the engine to see the drop", async || {
            engine.health().await.engine.reason.as_deref() == Some(MSG_VPN_WAIT)
        })
        .await;
        let health = engine.health().await;
        assert_eq!(
            health.protected,
            Some(false),
            "kept while the tunnel is down"
        );
        assert_eq!(health.public_ip.as_deref(), Some("198.51.100.23"));
        *lock(&tunnel) = TunnelState::Connected;
        wait_until("the tunnel to be back", async || {
            engine.health().await.engine.reason.as_deref() == Some(MSG_NOT_PROTECTED)
        })
        .await;
        let health = engine.health().await;
        assert_eq!(health.vpn.state, TunnelState::Connected);
        assert_eq!(health.engine.state, EngineRunState::Blocked);
        assert_eq!(
            health.protected,
            Some(false),
            "never protected while blocked"
        );
        assert_eq!(health.public_ip.as_deref(), Some("198.51.100.23"));
        wait_until("a check right after the reconnect", async || {
            hits.load(Ordering::SeqCst) > checks
        })
        .await;
        assert!(
            engine.live_session().await.is_none(),
            "torrents stay stopped"
        );

        // That check passes: torrents run, and the status says protected at once.
        *lock(&answer) = serde_json::json!({
            "ip": "185.153.177.12", "country": "Netherlands", "protected": true
        });
        *lock(&tunnel) = TunnelState::Connecting;
        wait_until("the second drop", async || {
            engine.health().await.engine.reason.as_deref() == Some(MSG_VPN_WAIT)
        })
        .await;
        *lock(&tunnel) = TunnelState::Connected;
        wait_until("torrents to run again", async || {
            let health = engine.health().await;
            health.engine.state == EngineRunState::Running && health.protected == Some(true)
        })
        .await;
        shutdown.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
    }

    /// A restarted engine (Reconnect, a newly saved VPN, a container restart) remembers a
    /// "not protected" answer: it starts no torrents before a check passes, and shows the
    /// answer meanwhile.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_restarted_engine_keeps_torrents_stopped_until_a_check_passes() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options(dir.path());
        opts.tunnel_if = Some("lo".into());
        let answer = Arc::new(Mutex::new(serde_json::json!({
            "ip": "198.51.100.23", "country": "Netherlands", "protected": false
        })));
        let (base, hits) = fake_ip_service(answer.clone()).await;
        let check = IpCheck {
            nordvpn_url: format!("{base}/nordvpn"),
            other_url: format!("{base}/ipify"),
            interval: Duration::from_secs(600),
            recheck_unprotected: Duration::from_millis(300),
        };
        let stop_writer = CancellationToken::new();
        let _tunnel = fake_supervisor(opts.status_file.clone(), stop_writer.clone());

        let first = Arc::new(Engine::new(&opts).with_ip_check(check.clone()));
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(first.clone().supervise(shutdown.clone()));
        wait_until("the not-protected answer to block torrents", async || {
            first.health().await.protected == Some(false)
        })
        .await;
        shutdown.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
        drop(first);

        // The new engine knows the answer before it checks anything.
        let engine = Arc::new(Engine::new(&opts).with_ip_check(check));
        let health = engine.health().await;
        assert_eq!(health.protected, Some(false));
        assert_eq!(health.public_ip.as_deref(), Some("198.51.100.23"));
        let shutdown = CancellationToken::new();
        let mut running = engine.running();
        let task = tokio::spawn(engine.clone().supervise(shutdown.clone()));
        // Several checks still say not protected: torrents never start meanwhile.
        let checks = hits.load(Ordering::SeqCst);
        let watch = async {
            loop {
                running.changed().await.unwrap();
                assert!(!*running.borrow(), "torrents started before a check passed");
            }
        };
        tokio::select! {
            _ = watch => {}
            _ = wait_until("three more checks", async || hits.load(Ordering::SeqCst) >= checks + 3) => {}
        }
        let health = engine.health().await;
        assert_eq!(health.engine.state, EngineRunState::Blocked);
        assert_eq!(health.engine.reason.as_deref(), Some(MSG_NOT_PROTECTED));
        assert!(engine.live_session().await.is_none());

        // A check passes: torrents run, and the answer is forgotten for the next restart.
        *lock(&answer) = serde_json::json!({
            "ip": "185.153.177.12", "country": "Netherlands", "protected": true
        });
        wait_until("torrents to run again", async || {
            engine.health().await.engine.state == EngineRunState::Running
        })
        .await;
        assert!(!opts.state_dir.join(NOT_PROTECTED_FILE).exists());
        shutdown.cancel();
        stop_writer.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
        assert_eq!(Engine::new(&opts).health().await.protected, None);
    }

    #[test]
    fn delete_output_stays_inside_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path());
        let engine = Engine::new(&opts);
        let outside = dir.path().join("keep");
        std::fs::create_dir(&outside).unwrap();
        engine.delete_output(&outside.display().to_string());
        assert!(outside.exists());
        engine.delete_output(&opts.downloads_dir.display().to_string());
        assert!(opts.downloads_dir.exists());
        engine.delete_output(&opts.downloads_dir.join("..").display().to_string());
        assert!(dir.path().exists());
    }
}
