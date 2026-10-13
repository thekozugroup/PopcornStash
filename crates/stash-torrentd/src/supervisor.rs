//! Root supervisor (`popcornstash run`, section 5 "VPN"): preflight, ownership,
//! policy routing and kill switch, DNS forwarder, tunnel, adopt socket, and
//! the two unprivileged children.

use std::net::{IpAddr, SocketAddr};
use std::os::fd::AsFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nix::fcntl::{OFlag, open, openat};
use nix::sys::stat::{Mode, fchmod, fstat, mkdirat};
use nix::unistd::{Gid, Uid, fchown};
use stash_core::internal_api::{
    DNS_FORWARDER_ADDR, ENGINE_DIR, EXIT_RELOAD, STATUS_FILE, SUPERVISOR_SOCKET, TUNNEL_IF,
    TunnelState, TunnelStatus, VPN_DIR,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::SupervisorOptions;
use crate::adopt::{self, AdoptConfig};
use crate::children::{self, ChildSpec, Credentials, EnvInputs};
use crate::status;
use crate::sys::{self, now_unix};
use crate::vpn::dns::{self, Forwarder, ForwarderConfig};
use crate::vpn::killswitch::{self, KillSwitch};
use crate::vpn::routes::{self, Family};
use crate::vpn::{self, Health, Tunnel, VpnSource};

const MSG_NEED_CAPS: &str = "Torrents need the container to have NET_ADMIN and /dev/net/tun.";
const MONITOR_EVERY: Duration = Duration::from_secs(5);
/// How long a WireGuard session stays usable after its last handshake.
const WIREGUARD_SESSION: Duration =
    Duration::from_secs(crate::vpn::wireguard::HANDSHAKE_MAX_AGE as u64);
/// A new tunnel gets this long to complete its first handshake.
const CONNECT_GRACE: Duration = Duration::from_secs(45);
const BACKOFF_MIN: Duration = Duration::from_secs(5);
const BACKOFF_MAX: Duration = Duration::from_secs(300);
const CHILD_BACKOFF_MIN: Duration = Duration::from_secs(2);
const CHILD_BACKOFF_MAX: Duration = Duration::from_secs(60);
const CHILD_STOP_GRACE: Duration = Duration::from_secs(10);
/// Upper bound for the start-up ownership walk of /config.
const MAX_CONFIG_ENTRIES: u64 = 5_000_000;

/// Paths derived from the options.
#[derive(Debug, Clone)]
struct Layout {
    vpn_dir: PathBuf,
    engine_dir: PathBuf,
    in_progress: PathBuf,
    status_file: PathBuf,
    socket: PathBuf,
}

impl Layout {
    fn new(options: &SupervisorOptions) -> Self {
        let torrent = options.config_dir.join("torrent");
        Self {
            vpn_dir: torrent.join(VPN_DIR),
            engine_dir: torrent.join(ENGINE_DIR),
            in_progress: options.stash_root.join("Downloads").join("In Progress"),
            status_file: options.run_dir.join(STATUS_FILE),
            socket: options.run_dir.join(SUPERVISOR_SOCKET),
        }
    }
}

pub async fn run(options: SupervisorOptions) -> anyhow::Result<()> {
    if !nix::unistd::geteuid().is_root() {
        tracing::warn!(
            "PopcornStash was started as a non-root user, so torrents are turned off. Remove `--user` from the container to use torrents."
        );
        return exec_serve(&options);
    }
    let layout = Layout::new(&options);
    let preflight = preflight(&options).await;
    let torrents = preflight.is_ok();
    let mut off_reason = preflight.as_ref().err().map(|reason| reason.to_string());
    if let Some(reason) = &off_reason {
        tracing::warn!("{reason}");
    }
    if let Err(e) = prepare(&options, &layout, torrents) {
        tracing::error!(error = %format!("{e:#}"), "could not prepare folders and ownership");
    }
    if torrents {
        match full(&options, &layout).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::error!(error = %format!("{e:#}"), "the VPN kill switch could not be set up; torrents are turned off");
                cleanup_rules(&options).await;
                off_reason = Some(
                    "Torrents are off because the VPN kill switch could not be set up. Check the container log.".into(),
                );
            }
        }
    }
    degraded(&options, off_reason.as_deref()).await
}

/// `run` as a non-root user behaves like `serve`.
fn exec_serve(options: &SupervisorOptions) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(&options.exe);
    command.arg("serve");
    for key in children::INTERNAL_VARS {
        command.env_remove(key);
    }
    command.env(
        "STASH_TORRENTS_OFF_REASON",
        "Torrents are off because the container runs as a non-root user. Remove the --user setting to use torrents.",
    );
    let error = command.exec();
    Err(anyhow::anyhow!("could not start the web server: {error}"))
}

/// Step 1. `Err` holds the sentence shown in the log (and the reason torrents are off).
async fn preflight(options: &SupervisorOptions) -> Result<(), String> {
    if options.engine_uid == 0 || options.puid == 0 || options.engine_uid == options.puid {
        return Err(format!(
            "STASH_TORRENT_UID ({}) must be a non-zero user id different from PUID ({}); torrents are turned off.",
            options.engine_uid, options.puid
        ));
    }
    if options.allow_no_vpn {
        return Ok(());
    }
    let tun_ok = std::fs::metadata("/dev/net/tun").map(|m| {
        use std::os::unix::fs::FileTypeExt;
        m.file_type().is_char_device()
    });
    if !matches!(tun_ok, Ok(true)) {
        return Err(MSG_NEED_CAPS.into());
    }
    for tool in ["ip", "nft"] {
        if sys::which(tool).is_none() {
            return Err(format!(
                "Torrents need `{tool}` in the image; torrents are turned off."
            ));
        }
    }
    if let Err(e) = KillSwitch::new(options.engine_uid).check().await {
        tracing::debug!(error = %e, "nftables dry run failed");
        return Err(MSG_NEED_CAPS.into());
    }
    Ok(())
}

fn errno(e: nix::errno::Errno) -> std::io::Error {
    std::io::Error::from_raw_os_error(e as i32)
}

/// Open (creating if needed) `name` inside `parent` without following a symlink.
/// Returns the fd and whether it was created.
fn open_or_create_dir(
    parent: &impl std::os::fd::AsFd,
    name: &str,
    mode: u32,
) -> std::io::Result<(std::os::fd::OwnedFd, bool)> {
    let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    let created = match mkdirat(parent, name, Mode::from_bits_truncate(mode)) {
        Ok(()) => true,
        Err(nix::errno::Errno::EEXIST) => false,
        Err(e) => return Err(errno(e)),
    };
    let fd = openat(parent, name, flags, Mode::empty()).map_err(|e| {
        if matches!(e, nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR) {
            std::io::Error::other(format!(
                "{name} is a symbolic link or a file; it must be a folder"
            ))
        } else {
            errno(e)
        }
    })?;
    Ok((fd, created))
}

/// Make `Downloads/In Progress` PUID:PGID 2775, never following symlinks below the stash root.
fn prepare_in_progress(options: &SupervisorOptions) -> std::io::Result<()> {
    std::fs::create_dir_all(&options.stash_root)?;
    let root = open(
        &options.stash_root,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(errno)?;
    let (puid, pgid) = (Uid::from_raw(options.puid), Gid::from_raw(options.pgid));
    let (downloads, created) = open_or_create_dir(&root, "Downloads", 0o775)?;
    let st = fstat(&downloads).map_err(errno)?;
    if created {
        fchown(&downloads, Some(puid), Some(pgid)).map_err(errno)?;
        fchmod(&downloads, Mode::from_bits_truncate(0o775)).map_err(errno)?;
    } else if st.st_mode & 0o011 == 0 {
        // The engine (group PGID) must be able to reach In Progress.
        fchmod(
            &downloads,
            Mode::from_bits_truncate((st.st_mode & 0o7777) | 0o010),
        )
        .map_err(errno)?;
    }
    let (in_progress, _) = open_or_create_dir(&downloads, "In Progress", 0o2775)?;
    // Without CAP_FSETID (compose drops it) the kernel silently clears the
    // setgid bit when root chmods a directory whose group root is not in. So
    // set the mode while the group is root's, then hand the folder over:
    // chown keeps setgid on directories.
    fchown(&in_progress, None, Some(Gid::from_raw(0))).map_err(errno)?;
    fchmod(&in_progress, Mode::from_bits_truncate(0o2775)).map_err(errno)?;
    fchown(&in_progress, Some(puid), Some(pgid)).map_err(errno)?;
    let mode = fstat(&in_progress).map_err(errno)?.st_mode & 0o7777;
    if mode != 0o2775 {
        tracing::warn!(
            mode = format!("{mode:o}"),
            "Downloads/In Progress could not be set to mode 2775"
        );
    }
    let root_mode = fstat(&root).map_err(errno)?.st_mode;
    if root_mode & 0o011 == 0 {
        tracing::warn!(path = %options.stash_root.display(), "the stash folder is not searchable by its group or others; the torrent engine may not reach Downloads/In Progress");
    }
    Ok(())
}

fn ensure_dir(path: &Path, mode: u32) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path).is_err() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(mode)
            .create(path)?;
    }
    Ok(())
}

/// `<config>/torrent`, its `vpn` folder and the engine state folder, opened
/// without following symlinks. The web server's user owns `torrent`, so it can
/// replace any of these entries with a symlink; root then refuses instead of
/// creating, chmodding or chowning at the link's target.
struct TorrentDirs {
    torrent: std::os::fd::OwnedFd,
    vpn: std::os::fd::OwnedFd,
    engine: std::os::fd::OwnedFd,
}

fn open_torrent_dirs(config_dir: &Path) -> std::io::Result<TorrentDirs> {
    let config = open(
        config_dir,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(errno)?;
    let (torrent, _) = open_or_create_dir(&config, "torrent", 0o700)?;
    let (vpn, _) = open_or_create_dir(&torrent, VPN_DIR, 0o700)?;
    let (engine, _) = open_or_create_dir(&torrent, ENGINE_DIR, 0o700)?;
    Ok(TorrentDirs {
        torrent,
        vpn,
        engine,
    })
}

/// Step 2: run dir and ownership (section 3).
fn prepare(options: &SupervisorOptions, layout: &Layout, torrents: bool) -> anyhow::Result<()> {
    ensure_dir(&options.run_dir, 0o755)?;
    std::fs::set_permissions(&options.run_dir, std::fs::Permissions::from_mode(0o755))?;
    for stale in [&layout.socket, &layout.status_file] {
        let _ = std::fs::remove_file(stale);
    }

    ensure_dir(&options.config_dir, 0o755)?;
    // A broken torrent folder turns torrents off but must not stop the web
    // server's folders from being prepared; the error is returned at the end.
    let mut torrent_error = None;
    let dirs = if torrents {
        match open_torrent_dirs(&options.config_dir) {
            Ok(dirs) => Some(dirs),
            Err(e) => {
                torrent_error = Some(anyhow::anyhow!(
                    "{}: {e}",
                    options.config_dir.join("torrent").display()
                ));
                None
            }
        }
    } else {
        None
    };
    let skip = match &dirs {
        Some(dirs) => Some(adopt::file_id(&dirs.engine)?),
        None => None,
    };
    let started = Instant::now();
    let report = adopt::own_tree(
        &options.config_dir,
        options.puid,
        options.pgid,
        skip,
        MAX_CONFIG_ENTRIES,
    )
    .map_err(|e| {
        anyhow::anyhow!(
            "{} could not be given to PUID:PGID: {e}",
            options.config_dir.display()
        )
    })?;
    tracing::info!(
        changed = report.changed,
        visited = report.visited,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "config folder owned by PUID:PGID"
    );

    // The engine (group PGID) must traverse /config and /config/torrent to reach its state.
    let config_mode = std::fs::metadata(&options.config_dir)?.mode();
    if config_mode & 0o011 == 0 {
        std::fs::set_permissions(
            &options.config_dir,
            std::fs::Permissions::from_mode((config_mode & 0o7777) | 0o010),
        )?;
    }
    if let Some(dirs) = &dirs {
        let mode = fstat(&dirs.torrent).map_err(errno)?.st_mode;
        if mode & 0o010 == 0 {
            fchmod(
                &dirs.torrent,
                Mode::from_bits_truncate((mode & 0o7777) | 0o010),
            )
            .map_err(errno)?;
        }
        // Only the web server's user may list the VPN files.
        fchmod(&dirs.vpn, Mode::from_bits_truncate(0o700)).map_err(errno)?;
        adopt::own_tree_at(
            dirs.torrent.as_fd(),
            &std::ffi::CString::new(ENGINE_DIR)?,
            options.engine_uid,
            options.pgid,
            None,
            MAX_CONFIG_ENTRIES,
        )
        .map_err(|e| {
            anyhow::anyhow!("the engine state folder could not be given to the engine user: {e}")
        })?;
        fchmod(&dirs.engine, Mode::from_bits_truncate(0o700)).map_err(errno)?;
    }
    prepare_in_progress(options).map_err(|e| anyhow::anyhow!("Downloads/In Progress: {e}"))?;
    match torrent_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Step 3: routing rules and kill switch.
async fn apply_base_policy(options: &SupervisorOptions, ipv6: bool) -> anyhow::Result<()> {
    routes::apply_rules(Family::V4, options.engine_uid).await?;
    if ipv6 && let Err(e) = routes::apply_rules(Family::V6, options.engine_uid).await {
        tracing::warn!(error = %e, "IPv6 routing rules could not be applied; the kill switch still drops the engine's IPv6");
    }
    KillSwitch::new(options.engine_uid).apply().await?;
    tracing::info!(
        engine_uid = options.engine_uid,
        "policy routing and kill switch applied"
    );
    Ok(())
}

/// Re-check the routing rules and the kill switch every few seconds and put
/// them back if anything removed them. Nothing else in the container should
/// touch them; this is defence in depth against an engine leak.
fn spawn_policy_guard(
    options: SupervisorOptions,
    ipv6: bool,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(MONITOR_EVERY) => {}
            }
            let rules_ok = routes::rules_in_place(Family::V4, options.engine_uid)
                .await
                .unwrap_or(false);
            let table_ok = killswitch::in_place().await;
            if cancel.is_cancelled() {
                return;
            }
            if !rules_ok || !table_ok {
                tracing::error!(
                    rules_ok,
                    table_ok,
                    "the routing rules or the kill switch were changed from outside; applying them again"
                );
                if let Err(e) = apply_base_policy(&options, ipv6).await {
                    tracing::error!(error = %e, "could not re-apply the kill switch");
                }
            }
        }
    })
}

async fn cleanup_rules(options: &SupervisorOptions) {
    if options.allow_no_vpn {
        return;
    }
    let _ = routes::remove_rules(Family::V4).await;
    if routes::ipv6_available().await {
        let _ = routes::remove_rules(Family::V6).await;
    }
    if let Err(e) = killswitch::remove().await {
        tracing::warn!(error = %e, "could not remove the nftables table");
    }
}

fn wait_for_signal(cancel: CancellationToken) {
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut term), Ok(mut int)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) else {
            tracing::error!("could not install signal handlers");
            return;
        };
        tokio::select! {
            _ = term.recv() => tracing::info!("SIGTERM received; shutting down"),
            _ = int.recv() => tracing::info!("SIGINT received; shutting down"),
        }
        cancel.cancel();
    });
}

fn env_inputs(options: &SupervisorOptions, layout: &Layout, token: &str) -> EnvInputs {
    EnvInputs {
        vars: std::env::vars().collect(),
        token: token.to_string(),
        run_dir: options.run_dir.clone(),
        engine_state: layout.engine_dir.clone(),
        engine_downloads: layout.in_progress.clone(),
        status_file: layout.status_file.clone(),
        allow_no_vpn: options.allow_no_vpn,
    }
}

/// GPU render node used for VAAPI and Quick Sync transcoding.
const RENDER_DEVICE: &str = "/dev/dri/renderD128";

/// The render device's group, when the web server needs it to transcode with the GPU.
/// Never the root group, and nothing when PGID already covers it.
fn render_group(pgid: u32) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    let gid = std::fs::metadata(RENDER_DEVICE).ok()?.gid();
    (gid != 0 && gid != pgid).then_some(gid)
}

/// `off_reason` is the sentence the UI shows when torrents are off (degraded mode).
fn serve_spec(options: &SupervisorOptions, inputs: &EnvInputs, torrents: bool, off_reason: Option<&str>) -> ChildSpec {
    let mut env = children::serve_env(inputs, torrents);
    if let (false, Some(reason)) = (torrents, off_reason) {
        env.push(("STASH_TORRENTS_OFF_REASON".into(), reason.to_string()));
    }
    ChildSpec {
        name: "serve",
        program: options.exe.clone(),
        args: vec!["serve".into()],
        env,
        cwd: PathBuf::from("/"),
        credentials: Some(Credentials {
            uid: options.puid,
            gid: options.pgid,
            umask: None,
            extra_gid: render_group(options.pgid),
        }),
        capture_stdout: false,
    }
}

fn engine_spec(options: &SupervisorOptions, inputs: &EnvInputs) -> ChildSpec {
    ChildSpec {
        name: "torrent-engine",
        program: options.exe.clone(),
        args: vec!["torrent-engine".into()],
        env: children::engine_env(inputs),
        cwd: PathBuf::from("/"),
        credentials: Some(Credentials {
            uid: options.engine_uid,
            gid: options.pgid,
            umask: Some(0o002),
            extra_gid: None,
        }),
        capture_stdout: false,
    }
}

type ReloadRequest = oneshot::Sender<()>;

/// Keep one child running until `cancel`. An engine exiting with
/// `EXIT_RELOAD` triggers a tunnel reload and is restarted right away.
async fn child_loop(
    spec: ChildSpec,
    cancel: CancellationToken,
    reload: Option<mpsc::Sender<ReloadRequest>>,
) {
    let mut backoff = CHILD_BACKOFF_MIN;
    loop {
        if cancel.is_cancelled() {
            return;
        }
        let started = Instant::now();
        let mut child = match children::spawn(&spec) {
            Ok(child) => child,
            Err(e) => {
                tracing::error!(child = spec.name, error = %e, "could not start");
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(CHILD_BACKOFF_MAX);
                continue;
            }
        };
        let credentials = spec.credentials;
        tracing::info!(
            child = spec.name,
            pid = child.id(),
            uid = credentials.map(|c| c.uid),
            gid = credentials.map(|c| c.gid),
            "started"
        );
        let status = tokio::select! {
            status = child.wait() => status,
            _ = cancel.cancelled() => {
                crate::vpn::wireguard::stop_child(&mut child, CHILD_STOP_GRACE).await;
                tracing::info!(child = spec.name, "stopped");
                return;
            }
        };
        let code = status.as_ref().ok().and_then(|s| s.code());
        if code == Some(EXIT_RELOAD)
            && let Some(reload) = &reload
        {
            tracing::info!(child = spec.name, "reload requested; re-applying the VPN");
            let (ack, done) = oneshot::channel();
            if reload.send(ack).await.is_ok() {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = done => {}
                }
            }
            backoff = CHILD_BACKOFF_MIN;
            continue;
        }
        if started.elapsed() > CHILD_BACKOFF_MAX {
            backoff = CHILD_BACKOFF_MIN;
        }
        match status {
            Ok(s) => {
                tracing::warn!(child = spec.name, status = %s, "exited; restarting in {}s", backoff.as_secs())
            }
            Err(e) => {
                tracing::warn!(child = spec.name, error = %e, "lost track of the process; restarting in {}s", backoff.as_secs())
            }
        }
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(CHILD_BACKOFF_MAX);
    }
}

/// Degraded mode: only the web server, torrents off.
async fn degraded(options: &SupervisorOptions, off_reason: Option<&str>) -> anyhow::Result<()> {
    let layout = Layout::new(options);
    let cancel = CancellationToken::new();
    wait_for_signal(cancel.clone());
    let inputs = env_inputs(options, &layout, "");
    let mut spec = serve_spec(options, &inputs, false, off_reason);
    if options.puid == 0 {
        tracing::warn!("PUID is 0, so the web server runs as root");
        spec.credentials = None;
    }
    child_loop(spec, cancel, None).await;
    Ok(())
}

/// Write status.json now and every 5 s.
fn spawn_status_writer(
    path: PathBuf,
    mut rx: watch::Receiver<TunnelStatus>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let mut current = rx.borrow_and_update().clone();
            current.updated_at = now_unix();
            if let Err(e) = status::write(&path, &current) {
                tracing::warn!(error = %e, "could not write status.json");
            }
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = rx.changed() => {}
                _ = tokio::time::sleep(MONITOR_EVERY) => {}
            }
        }
    })
}

/// Full mode: steps 3–10.
async fn full(options: &SupervisorOptions, layout: &Layout) -> anyhow::Result<()> {
    let token = sys::random_token();
    let ipv6 = routes::ipv6_available().await;
    if !options.allow_no_vpn {
        apply_base_policy(options, ipv6).await?;
    }
    let cancel = CancellationToken::new();
    wait_for_signal(cancel.clone());

    // Step 4: DNS forwarder.
    let (dns_tx, dns_rx) = watch::channel::<Vec<IpAddr>>(Vec::new());
    if !options.allow_no_vpn {
        let bind: SocketAddr = DNS_FORWARDER_ADDR.parse()?;
        Forwarder::new(dns_rx, ForwarderConfig::default())
            .spawn(bind, cancel.clone())
            .await?;
        tracing::info!(%bind, "DNS forwarder listening");
    }

    // Step 9: adopt socket.
    let listener = adopt::bind(&layout.socket, options.puid, options.pgid)?;
    let adopt_config = Arc::new(AdoptConfig::new(
        options.stash_root.clone(),
        options.puid,
        options.pgid,
        options.engine_uid,
    ));
    tokio::spawn(adopt::serve(listener, adopt_config, cancel.clone()));

    // Status file and tunnel manager (step 5, 7, 8).
    let initial = {
        let mut s = TunnelStatus::not_configured(now_unix());
        s.kill_switch = !options.allow_no_vpn;
        s
    };
    let (status_tx, status_rx) = watch::channel(initial);
    let writer = spawn_status_writer(layout.status_file.clone(), status_rx, cancel.clone());
    let (reload_tx, reload_rx) = mpsc::channel::<ReloadRequest>(4);
    let manager = TunnelManager {
        vpn_dir: layout.vpn_dir.clone(),
        run_dir: options.run_dir.clone(),
        status: status_tx,
        dns: dns_tx,
        reload: reload_rx,
        cancel: cancel.clone(),
        ipv6,
        allow_no_vpn: options.allow_no_vpn,
    };
    let manager_task = tokio::spawn(manager.run());
    let guard =
        (!options.allow_no_vpn).then(|| spawn_policy_guard(options.clone(), ipv6, cancel.clone()));

    // Step 6: children.
    let inputs = env_inputs(options, layout, &token);
    let serve_task = tokio::spawn(child_loop(
        serve_spec(options, &inputs, true, None),
        cancel.clone(),
        None,
    ));
    let engine_task = tokio::spawn(child_loop(
        engine_spec(options, &inputs),
        cancel.clone(),
        Some(reload_tx),
    ));

    // Step 10: shutdown.
    cancel.cancelled().await;
    let _ = tokio::join!(serve_task, engine_task);
    let _ = manager_task.await;
    let _ = writer.await;
    if let Some(guard) = guard {
        let _ = guard.await;
    }
    cleanup_rules(options).await;
    let _ = std::fs::remove_file(&layout.socket);
    let _ = std::fs::remove_file(&layout.status_file);
    tracing::info!("stopped cleanly");
    Ok(())
}

struct TunnelManager {
    vpn_dir: PathBuf,
    run_dir: PathBuf,
    status: watch::Sender<TunnelStatus>,
    dns: watch::Sender<Vec<IpAddr>>,
    reload: mpsc::Receiver<ReloadRequest>,
    cancel: CancellationToken,
    ipv6: bool,
    allow_no_vpn: bool,
}

enum Wake {
    Cancel,
    Reload(ReloadRequest),
    Timer,
}

fn status_for(
    source: Option<&VpnSource>,
    state: TunnelState,
    error: Option<String>,
    since: Option<i64>,
    kill_switch: bool,
) -> TunnelStatus {
    let meta = source.and_then(|s| s.meta.as_ref());
    TunnelStatus {
        state,
        provider: source.map(VpnSource::provider),
        protocol: source.map(VpnSource::protocol),
        label: meta.and_then(|m| m.label.clone()),
        server: source.and_then(VpnSource::server),
        country: meta.and_then(|m| m.country.clone()),
        interface: (state == TunnelState::Connected).then(|| TUNNEL_IF.to_string()),
        since,
        error,
        kill_switch,
        updated_at: now_unix(),
    }
}

/// A tiny DNS query sent through the tunnel so a fresh WireGuard peer handshakes.
fn nudge_query() -> Vec<u8> {
    let id = rand::random::<[u8; 2]>();
    vec![
        id[0], id[1], 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1,
    ]
}

impl TunnelManager {
    async fn wait(&mut self, timeout: Option<Duration>) -> Wake {
        let timer = async {
            match timeout {
                Some(d) => tokio::time::sleep(d).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = self.cancel.cancelled() => Wake::Cancel,
            request = self.reload.recv() => match request {
                Some(ack) => Wake::Reload(ack),
                None => Wake::Cancel,
            },
            _ = timer => Wake::Timer,
        }
    }

    fn publish(&self, status: TunnelStatus) {
        if self.status.borrow().state != status.state || self.status.borrow().error != status.error
        {
            match status.state {
                TunnelState::Connected => tracing::info!(server = ?status.server, "VPN connected"),
                TunnelState::Error => {
                    tracing::warn!(error = status.error.as_deref().unwrap_or(""), "VPN problem")
                }
                state => tracing::info!(?state, "VPN state"),
            }
        }
        self.status.send_replace(status);
    }

    async fn run(mut self) {
        if self.allow_no_vpn {
            // No tunnel and no rules: just answer reloads so the engine restarts.
            loop {
                match self.wait(None).await {
                    Wake::Cancel => return,
                    Wake::Reload(ack) => {
                        let _ = ack.send(());
                    }
                    Wake::Timer => {}
                }
            }
        }
        let mut backoff = BACKOFF_MIN;
        loop {
            let source = match vpn::load(&self.vpn_dir) {
                Ok(Some(source)) => source,
                Ok(None) => {
                    self.publish(status_for(
                        None,
                        TunnelState::NotConfigured,
                        None,
                        None,
                        true,
                    ));
                    match self.wait(None).await {
                        Wake::Cancel => return,
                        Wake::Reload(ack) => {
                            let _ = ack.send(());
                            continue;
                        }
                        Wake::Timer => continue,
                    }
                }
                Err(e) => {
                    self.publish(status_for(None, TunnelState::Error, Some(e.0), None, true));
                    match self.wait(None).await {
                        Wake::Cancel => return,
                        Wake::Reload(ack) => {
                            let _ = ack.send(());
                            continue;
                        }
                        Wake::Timer => continue,
                    }
                }
            };
            self.publish(status_for(
                Some(&source),
                TunnelState::Connecting,
                None,
                None,
                true,
            ));
            let up = tokio::select! {
                _ = self.cancel.cancelled() => return,
                up = Tunnel::bring_up(&source, TUNNEL_IF, &self.run_dir, self.ipv6) => up,
            };
            let mut tunnel = match up {
                Ok(tunnel) => tunnel,
                Err(reason) => {
                    self.publish(status_for(
                        Some(&source),
                        TunnelState::Error,
                        Some(reason),
                        None,
                        true,
                    ));
                    match self.wait(Some(backoff)).await {
                        Wake::Cancel => return,
                        Wake::Reload(ack) => {
                            backoff = BACKOFF_MIN;
                            let _ = ack.send(());
                        }
                        Wake::Timer => backoff = (backoff * 2).min(BACKOFF_MAX),
                    }
                    continue;
                }
            };
            if let Err(e) = self.install_routes(&source).await {
                tunnel.teardown().await;
                routes::flush_table(Family::V4).await;
                self.publish(status_for(
                    Some(&source),
                    TunnelState::Error,
                    Some(format!("Could not route through the tunnel: {}", e.message)),
                    None,
                    true,
                ));
                match self.wait(Some(backoff)).await {
                    Wake::Cancel => return,
                    Wake::Reload(ack) => {
                        backoff = BACKOFF_MIN;
                        let _ = ack.send(());
                    }
                    Wake::Timer => backoff = (backoff * 2).min(BACKOFF_MAX),
                }
                continue;
            }
            self.dns.send_replace(tunnel.dns_servers(&source));

            // When the tunnel last started waiting for a handshake / reconnect.
            let mut pending_since = Some(Instant::now());
            let mut since: Option<i64> = None;
            let mut last_up: Option<Instant> = None;
            let mut first = true;
            let outcome = loop {
                let wait = if first {
                    Duration::from_millis(500)
                } else {
                    MONITOR_EVERY
                };
                first = false;
                match self.wait(Some(wait)).await {
                    Wake::Cancel => break Wake::Cancel,
                    Wake::Reload(ack) => break Wake::Reload(ack),
                    Wake::Timer => {}
                }
                match tunnel.health(now_unix()).await {
                    Health::Up => {
                        last_up = Some(Instant::now());
                        pending_since = None;
                        if since.is_none() {
                            since = Some(now_unix());
                            backoff = BACKOFF_MIN;
                        }
                        // OpenVPN learns DNS from the server; keep the forwarder current.
                        let servers = tunnel.dns_servers(&source);
                        if !servers.is_empty() && *self.dns.borrow() != servers {
                            self.dns.send_replace(servers);
                        }
                        if !routes::has_default_route(Family::V4, TUNNEL_IF).await {
                            let _ = self.install_routes(&source).await;
                        }
                        self.publish(status_for(
                            Some(&source),
                            TunnelState::Connected,
                            None,
                            since,
                            true,
                        ));
                    }
                    Health::Pending => {
                        // boringtun reports no handshake while it rekeys; a WireGuard
                        // session stays valid until the last handshake is 180 s old.
                        let rekeying = matches!(tunnel, Tunnel::WireGuard(_))
                            && last_up.is_some_and(|t| t.elapsed() < WIREGUARD_SESSION);
                        if rekeying {
                            continue;
                        }
                        let waiting = *pending_since.get_or_insert_with(Instant::now);
                        if waiting.elapsed() > CONNECT_GRACE {
                            break Wake::Timer;
                        }
                        // Traffic makes a WireGuard peer handshake; send a little through the tunnel.
                        let servers = self.current_dns();
                        tokio::spawn(async move {
                            let config = ForwarderConfig {
                                timeout: Duration::from_secs(1),
                                ..ForwarderConfig::default()
                            };
                            let _ = dns::forward(&nudge_query(), &servers, &config, false).await;
                        });
                        self.publish(status_for(
                            Some(&source),
                            TunnelState::Connecting,
                            None,
                            None,
                            true,
                        ));
                        since = None;
                    }
                    Health::Down(reason) => {
                        tracing::warn!(reason = %reason, "the tunnel failed; tearing it down");
                        self.publish(status_for(
                            Some(&source),
                            TunnelState::Error,
                            Some(reason),
                            None,
                            true,
                        ));
                        break Wake::Timer;
                    }
                }
            };
            tunnel.teardown().await;
            routes::flush_table(Family::V4).await;
            if self.ipv6 {
                routes::flush_table(Family::V6).await;
            }
            match outcome {
                Wake::Cancel => return,
                Wake::Reload(ack) => {
                    backoff = BACKOFF_MIN;
                    self.publish(status_for(
                        Some(&source),
                        TunnelState::Connecting,
                        None,
                        None,
                        true,
                    ));
                    let _ = ack.send(());
                }
                Wake::Timer => {
                    if since.is_none() && self.status.borrow().state != TunnelState::Error {
                        self.publish(status_for(
                            Some(&source),
                            TunnelState::Error,
                            Some(
                                "The VPN server did not answer in time. Check the server address and keys."
                                    .into(),
                            ),
                            None,
                            true,
                        ));
                    }
                    match self.wait(Some(backoff)).await {
                        Wake::Cancel => return,
                        Wake::Reload(ack) => {
                            backoff = BACKOFF_MIN;
                            let _ = ack.send(());
                        }
                        Wake::Timer => backoff = (backoff * 2).min(BACKOFF_MAX),
                    }
                }
            }
        }
    }

    fn current_dns(&self) -> Vec<IpAddr> {
        let servers = self.dns.borrow().clone();
        if servers.is_empty() {
            dns::DEFAULT_UPSTREAMS.to_vec()
        } else {
            servers
        }
    }

    async fn install_routes(&self, source: &VpnSource) -> Result<(), sys::CommandError> {
        routes::set_default_route(Family::V4, TUNNEL_IF).await?;
        let tunnel_v6 = matches!(&source.spec, vpn::TunnelSpec::WireGuard(plan) if plan.has_ipv6());
        if self.ipv6
            && tunnel_v6
            && let Err(e) = routes::set_default_route(Family::V6, TUNNEL_IF).await
        {
            tracing::warn!(error = %e, "no IPv6 route through the tunnel");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(dir: &Path) -> SupervisorOptions {
        SupervisorOptions {
            config_dir: dir.join("config"),
            stash_root: dir.join("stash"),
            puid: 4242,
            pgid: 4343,
            engine_uid: 6881,
            allow_no_vpn: false,
            exe: PathBuf::from("/bin/true"),
            run_dir: dir.join("run"),
        }
    }

    #[tokio::test]
    async fn preflight_rejects_bad_uids() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options(dir.path());
        opts.engine_uid = opts.puid;
        assert!(
            preflight(&opts)
                .await
                .unwrap_err()
                .contains("STASH_TORRENT_UID")
        );
        opts.engine_uid = 0;
        assert!(preflight(&opts).await.is_err());
        opts.engine_uid = 6881;
        opts.puid = 0;
        assert!(preflight(&opts).await.is_err());
    }

    #[test]
    fn prepare_sets_ownership_and_modes() {
        if !nix::unistd::geteuid().is_root() {
            eprintln!("skipped: needs root");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path());
        let layout = Layout::new(&opts);
        std::fs::create_dir_all(opts.config_dir.join("cache")).unwrap();
        std::fs::write(opts.config_dir.join("popcornstash.db"), b"").unwrap();
        prepare(&opts, &layout, true).unwrap();
        let meta = |p: &Path| std::fs::symlink_metadata(p).unwrap();
        assert_eq!(
            (meta(&opts.config_dir).uid(), meta(&opts.config_dir).gid()),
            (4242, 4343)
        );
        assert_eq!(meta(&opts.config_dir.join("popcornstash.db")).uid(), 4242);
        assert_eq!(meta(&layout.vpn_dir).uid(), 4242);
        assert_eq!(
            (
                meta(&layout.engine_dir).uid(),
                meta(&layout.engine_dir).gid()
            ),
            (6881, 4343)
        );
        assert_eq!(meta(&layout.engine_dir).mode() & 0o7777, 0o700);
        let ip = meta(&layout.in_progress);
        assert_eq!(
            (ip.uid(), ip.gid(), ip.mode() & 0o7777),
            (4242, 4343, 0o2775)
        );
        assert_eq!(meta(&opts.run_dir).mode() & 0o777, 0o755);
        // Idempotent.
        prepare(&opts, &layout, true).unwrap();

        // A symlinked "In Progress" is refused, and its target is untouched.
        let target = dir.path().join("elsewhere");
        std::fs::create_dir(&target).unwrap();
        std::fs::remove_dir(&layout.in_progress).unwrap();
        std::os::unix::fs::symlink(&target, &layout.in_progress).unwrap();
        assert!(prepare(&opts, &layout, true).is_err());
        assert_eq!(meta(&target).uid(), 0);
    }

    /// `/config/torrent` belongs to PUID, so the web server's user can replace
    /// `engine`, `vpn` or `torrent` itself with a symlink. Root must not chmod,
    /// chown or create anything at the link's target on the next start.
    #[test]
    fn prepare_does_not_follow_planted_symlinks() {
        if !nix::unistd::geteuid().is_root() {
            eprintln!("skipped: needs root");
            return;
        }
        let meta = |p: &Path| std::fs::symlink_metadata(p).unwrap();
        for planted in ["engine", "vpn", "torrent"] {
            let dir = tempfile::tempdir().unwrap();
            let opts = options(dir.path());
            let layout = Layout::new(&opts);
            prepare(&opts, &layout, true).unwrap();
            let target = dir.path().join("target");
            std::fs::create_dir(&target).unwrap();
            std::fs::create_dir(target.join("engine")).unwrap();
            std::fs::write(target.join("engine/file"), b"x").unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
            let link = if planted == "torrent" {
                opts.config_dir.join("torrent")
            } else {
                opts.config_dir.join("torrent").join(planted)
            };
            std::fs::rename(&link, dir.path().join("moved-away")).unwrap();
            std::os::unix::fs::symlink(&target, &link).unwrap();

            let _ = prepare(&opts, &layout, true);
            assert_eq!(meta(&target).mode() & 0o7777, 0o755, "{planted}: mode");
            assert_eq!(
                (meta(&target).uid(), meta(&target).gid()),
                (0, 0),
                "{planted}"
            );
            let inner = target.join("engine");
            assert_eq!(
                (meta(&inner).uid(), meta(&inner).gid()),
                (0, 0),
                "{planted}"
            );
            assert_eq!(meta(&inner.join("file")).uid(), 0, "{planted}");
            assert!(
                !target.join("vpn").exists(),
                "{planted}: created through the link"
            );
        }
    }

    #[test]
    fn status_reflects_source() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("wireguard.conf"),
            "[Interface]\nPrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=\nAddress = 10.5.0.2/32\n[Peer]\nPublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\nEndpoint = wg.example:51820\nAllowedIPs = 0.0.0.0/0\n",
        )
        .unwrap();
        let source = vpn::load(dir.path()).unwrap().unwrap();
        let status = status_for(Some(&source), TunnelState::Connected, None, Some(5), true);
        assert_eq!(status.interface.as_deref(), Some("psw0"));
        assert_eq!(status.server.as_deref(), Some("wg.example"));
        assert_eq!(status.since, Some(5));
        let status = status_for(Some(&source), TunnelState::Connecting, None, None, true);
        assert!(status.interface.is_none());
        assert_eq!(nudge_query().len(), 17);
        assert_eq!(dns::question_end(&nudge_query()), Some(17));
    }
}
