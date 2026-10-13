//! librqbit session, public IP checks and the engine process main loop.
//!
//! The engine runs as its own user with no capabilities. It applies Landlock
//! to itself first, then runs librqbit with every socket bound to the tunnel
//! interface. librqbit binds sockets to the interface index that exists when
//! the session starts, so the session is created when the tunnel comes up,
//! stopped when it goes down and recreated when the interface changes (a
//! reconnect makes a new `psw0`). While no session runs the engine is
//! "blocked": it refuses new torrents and answers from its persisted state.

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
use stash_core::internal_api::{
    AddTorrentResponse, EXIT_RELOAD, EngineHealth, EngineRunState, EngineState, EngineTorrent,
    EngineTorrentState, LimitsRequest, TunnelState, TunnelStatus, VpnProvider,
};
use tokio::net::TcpListener;
use tokio::sync::{Notify, RwLock, watch};
use tokio_util::sync::CancellationToken;

use crate::EngineOptions;
use crate::api::{self, BackendError, MSG_VPN_WAIT, NewTorrent, TorrentBackend};
use crate::proxy::{self, ProxyConfig};
use crate::status;
use crate::sys::{self, now_unix};
use crate::vpn::killswitch::{ENGINE_DHT_PORT, ENGINE_LISTEN_PORT};

/// Public IP check interval while running (section 5, "Proof of protection").
pub const IP_CHECK_INTERVAL: Duration = Duration::from_secs(600);
const POLL: Duration = Duration::from_secs(1);
const NORDVPN_INSIGHTS: &str = "https://api.nordvpn.com/v1/helpers/ips/insights";
const IPIFY: &str = "https://api.ipify.org?format=json";
pub const MSG_NOT_CONFIGURED: &str = "The VPN is not set up yet.";
/// Wait between failed attempts to start the torrent session on the same interface.
const START_RETRY: Duration = Duration::from_secs(60);
const PENDING_FILE: &str = "pending.json";
const LIMITS_FILE: &str = "limits.json";
const SESSION_DIR: &str = "session";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
    magnet: String,
    name: Option<String>,
    #[serde(skip)]
    error: Option<String>,
    #[serde(skip)]
    resolving: bool,
}

#[derive(Debug, Clone)]
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
    limits: LimitsRequest,
    pending: BTreeMap<String, PendingAdd>,
    last_known: HashMap<String, EngineTorrent>,
    reserved_folders: HashSet<String>,
}

pub struct Engine {
    downloads_dir: PathBuf,
    state_dir: PathBuf,
    status_file: PathBuf,
    tunnel_if: Option<String>,
    allow_no_vpn: bool,
    session: RwLock<Option<Live>>,
    inner: Mutex<Inner>,
    running_tx: watch::Sender<bool>,
    /// The tunnel is up (or no VPN is required): the proxy may forward.
    tunnel_tx: watch::Sender<bool>,
    reload: Notify,
    reload_requested: AtomicBool,
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
    let total = stats.total_bytes;
    let done = stats.progress_bytes.min(total.max(stats.progress_bytes));
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
        state: torrent_state(&stats.state, stats.finished),
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

fn kbps_to_bps(kbps: u64) -> Option<NonZeroU32> {
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
        let engine = Self {
            downloads_dir: options.downloads_dir.clone(),
            state_dir: options.state_dir.clone(),
            status_file: options.status_file.clone(),
            tunnel_if: options.tunnel_if.clone(),
            allow_no_vpn: options.allow_no_vpn,
            session: RwLock::new(None),
            inner: Mutex::new(Inner {
                tunnel: None,
                running: false,
                reason: Some(MSG_VPN_WAIT.into()),
                ip: None,
                limits: LimitsRequest::default(),
                pending: BTreeMap::new(),
                last_known: HashMap::new(),
                reserved_folders: HashSet::new(),
            }),
            running_tx,
            tunnel_tx,
            reload: Notify::new(),
            reload_requested: AtomicBool::new(false),
        };
        engine.load_saved_state();
        engine
    }

    /// `true` while the torrent session runs.
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
            if !running {
                inner.ip = None;
            }
            changed
        };
        if changed {
            if running {
                tracing::info!("torrents are running");
            } else {
                tracing::info!(
                    reason = reason.as_deref().unwrap_or(""),
                    "torrents are blocked"
                );
            }
        }
        self.running_tx.send_replace(running);
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
                    let current = self.session.read().await.as_ref().map(|live| live.ifindex);
                    let wait_retry = failed_start.is_some_and(|(failed, at)| {
                        failed == ifindex && at.elapsed() < START_RETRY
                    });
                    if current != Some(ifindex) && !wait_retry {
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
                    }
                    // Magnets added since the last tick (or left over from a stopped session).
                    self.spawn_resolutions().await;
                    let due = last_ip_check.is_none_or(|t| t.elapsed() >= IP_CHECK_INTERVAL);
                    if due && lock(&self.inner).running {
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
        SessionOptions {
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
            ..Default::default()
        }
    }

    async fn start_session(&self, ifindex: Option<u32>) -> anyhow::Result<()> {
        let mut last_error = None;
        // Fixed ports first (the kill switch opens them on the tunnel); a port
        // still held by the previous session is retried, then any port is used.
        for attempt in 0..4 {
            let options = self.session_options(ifindex, attempt < 3);
            match Session::new_with_opts(self.downloads_dir.clone(), options).await {
                Ok(session) => {
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
        let live = self.session.write().await.take();
        if let Some(live) = live {
            let snapshot: Vec<EngineTorrent> = live
                .session
                .with_torrents(|iter| iter.map(|(_, handle)| engine_torrent(handle)).collect());
            {
                let mut inner = lock(&self.inner);
                inner.last_known = snapshot.into_iter().map(|t| (t.id.clone(), t)).collect();
                for pending in inner.pending.values_mut() {
                    pending.resolving = false;
                }
            }
            live.session.stop().await;
            tracing::info!("torrent session stopped");
        }
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
        let todo: Vec<(String, String)> = {
            let mut inner = lock(&self.inner);
            inner
                .pending
                .iter_mut()
                .filter(|(_, p)| !p.resolving && p.error.is_none())
                .map(|(id, p)| {
                    p.resolving = true;
                    (id.clone(), p.magnet.clone())
                })
                .collect()
        };
        for (id, magnet) in todo {
            let engine = self.clone();
            let session = session.clone();
            tokio::spawn(async move {
                let cancel = session.cancellation_token().clone();
                let result = tokio::select! {
                    _ = cancel.cancelled() => None,
                    r = engine.resolve_and_add(&session, &id, &magnet) => Some(r),
                };
                let mut inner = lock(&engine.inner);
                match result {
                    None => {
                        if let Some(p) = inner.pending.get_mut(&id) {
                            p.resolving = false;
                        }
                    }
                    Some(Ok(())) => {
                        inner.pending.remove(&id);
                        let pending = inner.pending.clone();
                        drop(inner);
                        engine.save_pending(&pending);
                    }
                    Some(Err(e)) => {
                        tracing::warn!(id = %id, error = %format!("{e:#}"), "could not add a magnet link");
                        if let Some(p) = inner.pending.get_mut(&id) {
                            p.resolving = false;
                            p.error = Some("The torrent's details could not be fetched.".into());
                        }
                    }
                }
            });
        }
    }

    async fn resolve_and_add(
        &self,
        session: &Arc<Session>,
        id: &str,
        magnet: &str,
    ) -> anyhow::Result<()> {
        let trackers = Magnet::parse(magnet)
            .map(|m| m.trackers)
            .unwrap_or_default();
        let response = session
            .add_torrent(
                AddTorrent::from_url(magnet),
                Some(AddTorrentOptions {
                    list_only: true,
                    ..Default::default()
                }),
            )
            .await?;
        let RqAddResponse::ListOnly(list) = response else {
            return Ok(());
        };
        if !lock(&self.inner).pending.contains_key(id) {
            // Removed while we were fetching its details.
            return Ok(());
        }
        let name = list.info.name().map(|n| n.into_owned());
        let single = list.info.info().files.is_none();
        let folder = self.unique_folder(&folder_name(name.as_deref(), single, id));
        let options = AddTorrentOptions {
            sub_folder: Some(folder),
            initial_peers: Some(list.seen_peers.clone()),
            trackers: (!trackers.is_empty()).then_some(trackers),
            ..Default::default()
        };
        session
            .add_torrent(
                AddTorrent::from_bytes(list.torrent_bytes.clone()),
                Some(options),
            )
            .await?;
        Ok(())
    }

    async fn check_public_ip(&self, provider: Option<VpnProvider>) {
        match fetch_public_ip(self.tunnel_if.as_deref(), provider).await {
            Ok(info) => {
                tracing::info!(ip = %info.ip, country = ?info.country, protected = ?info.protected, "public IP through the tunnel");
                let mut inner = lock(&self.inner);
                if inner.running {
                    inner.ip = Some(info);
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
                state: if p.error.is_some() {
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

async fn fetch_public_ip(
    interface: Option<&str>,
    provider: Option<VpnProvider>,
) -> anyhow::Result<IpInfo> {
    #[allow(unused_mut)]
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("PopcornStash/", env!("CARGO_PKG_VERSION")));
    if let Some(interface) = interface {
        builder = builder.interface(interface);
    }
    let client = builder.build()?;
    let url = if provider == Some(VpnProvider::Nordvpn) {
        NORDVPN_INSIGHTS
    } else {
        IPIFY
    };
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
        let known: HashSet<String> = torrents.iter().map(|t| t.id.clone()).collect();
        torrents.extend(self.pending_entries(&known));
        torrents.sort_by(|a, b| a.id.cmp(&b.id));
        torrents
    }

    async fn add(&self, torrent: NewTorrent) -> Result<AddTorrentResponse, BackendError> {
        let Some(session) = self.live_session().await else {
            return Err(BackendError::Blocked(MSG_VPN_WAIT.into()));
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
                let options = AddTorrentOptions {
                    sub_folder: Some(folder),
                    ..Default::default()
                };
                let response = session
                    .add_torrent(AddTorrent::from_bytes(bytes), Some(options))
                    .await
                    .map_err(|e| BackendError::Internal(format!("{e:#}")))?;
                let name = response.into_handle().and_then(|h| h.name()).or(name);
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
                let pending = {
                    let mut inner = lock(&self.inner);
                    let entry = inner
                        .pending
                        .entry(id.clone())
                        .or_insert_with(|| PendingAdd {
                            magnet: link.clone(),
                            name: name.clone(),
                            error: None,
                            resolving: false,
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
        let was_pending = {
            let mut inner = lock(&self.inner);
            let removed = inner.pending.remove(&id).is_some();
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
        lock(&self.inner).limits = limits;
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
            })
            .await
            .unwrap();
        let engine = Engine::new(&opts);
        assert_eq!(
            lock(&engine.inner).limits,
            LimitsRequest {
                down_kbps: 100,
                up_kbps: 5
            }
        );
        assert_eq!(kbps_to_bps(0), None);
        assert_eq!(kbps_to_bps(1).map(NonZeroU32::get), Some(1024));
        assert_eq!(kbps_to_bps(u64::MAX).map(NonZeroU32::get), Some(u32::MAX));
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
