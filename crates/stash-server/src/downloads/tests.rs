//! End-to-end tests for web downloads, torrents and uploads against local fake
//! servers (a Range-capable file server and a fake torrent engine). No internet,
//! no root.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Path as AxPath, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use futures::StreamExt;
use serde::Deserialize;
use stash_core::config::Config;
use stash_core::db::Db;
use stash_core::events::EventBus;
use stash_core::internal_api::{
    AddTorrentRequest, AddTorrentResponse, EngineHealth, EngineRunState, EngineState,
    EngineTorrent, EngineTorrentState, LimitsRequest, TunnelState, TunnelStatus,
};
use stash_core::jobs::JobQueue;
use stash_core::library::Library;
use stash_core::media::MediaService;
use stash_core::models::{DownloadKind, DownloadRow, DownloadState, Role};
use stash_core::paths::StashLayout;
use stash_core::settings::SettingsStore;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::manager::{MSG_VPN_REQUIRED, MSG_WAITING_FOR_VPN, Tuning};
use super::source::Magnet;
use super::torrents::MSG_LOST;
use super::{AddDownloadRequest, DownloadFilter, DownloadManager, ForwardedHeaders};
use crate::auth::{AuthUser, Scope};
use crate::error::ErrorCode;
use crate::torrentd_client::TorrentdClient;
use crate::uploads::{CHUNK_SIZE, CreateUpload, UploadService};

const MAGNET: &str = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Big+Buck+Bunny&tr=udp%3A%2F%2Ftracker.example%3A80%2Fpasskey%2FSECRETPASSKEY";
const TORRENT_FILE: &[u8] =
    b"d8:announce14:http://t.local4:infod6:lengthi12e4:name9:movie.mkv12:piece lengthi16384e6:pieces0:ee";

macro_rules! wait_until {
    ($what:expr, $cond:expr) => {{
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            if $cond {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for {}",
                $what
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }};
}

fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

fn fast() -> Tuning {
    Tuning {
        retry_delay: Duration::from_millis(20),
        worker_tick: Duration::from_millis(50),
        torrent_fast: Duration::from_millis(50),
        torrent_slow: Duration::from_millis(100),
        engine_backoff_max: Duration::from_millis(200),
        torrent_missing: Duration::from_millis(400),
        finish_retry: Duration::from_millis(100),
        torrent_fetch_timeout: Duration::from_millis(500),
    }
}

// --- environment ---------------------------------------------------------------------

#[derive(Default)]
struct Opts {
    engine: bool,
    allow_local: bool,
    start: bool,
    /// Configure a VPN download proxy address (nothing listens there).
    proxy: bool,
    /// Use this VPN download proxy instead (a fake proxy started by the test).
    proxy_url: Option<Url>,
}

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    config_dir: PathBuf,
    db: Db,
    settings: SettingsStore,
    manager: Arc<DownloadManager>,
    uploads: Arc<UploadService>,
    cancel: CancellationToken,
    owner: AuthUser,
    member: AuthUser,
    engine: Option<FakeEngine>,
}

impl Drop for Env {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn add_user(db: &Db, role: Role) -> AuthUser {
    let id = stash_core::new_id();
    sqlx::query("INSERT INTO users (id, display_name, role, webauthn_id, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(format!("{role} {id}"))
        .bind(role.as_str())
        .bind(stash_core::new_id())
        .bind(stash_core::now())
        .execute(db)
        .await
        .unwrap();
    AuthUser {
        user_id: id,
        display_name: "Test".into(),
        role,
        scopes: vec![Scope::Full],
        session_id: None,
        token_id: None,
    }
}

async fn env(opts: Opts) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("stash");
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();
    let mut config = Config::for_tests(&root, &config_dir);
    config.torrentd_url = None;
    config.torrentd_proxy = None;
    let engine = if opts.engine {
        let token = TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        config.internal_token = Some(token.clone());
        let engine = FakeEngine::start(token, root.clone()).await;
        config.torrentd_url = Some(engine.url.clone());
        if opts.proxy {
            config.torrentd_proxy = Some(Url::parse("http://127.0.0.1:9").unwrap());
        }
        if let Some(proxy) = &opts.proxy_url {
            config.torrentd_proxy = Some(proxy.clone());
        }
        Some(engine)
    } else {
        None
    };
    let config = Arc::new(config);
    let db = stash_core::db::open(&config.db_path()).await.unwrap();
    let layout = Arc::new(StashLayout::new(&root));
    layout.ensure().await.unwrap();
    let events = EventBus::new();
    let jobs = JobQueue::new(db.clone(), events.clone());
    let settings = SettingsStore::load(db.clone(), config.hwaccel)
        .await
        .unwrap();
    let media = Arc::new(MediaService::new(
        config.clone(),
        db.clone(),
        settings.clone(),
        layout.clone(),
        events.clone(),
    ));
    let library = Arc::new(Library::new(
        db.clone(),
        layout.clone(),
        events.clone(),
        jobs.clone(),
        settings.clone(),
        media,
    ));
    let torrentd = Arc::new(TorrentdClient::new(config.clone()));
    let manager = Arc::new(DownloadManager::new(
        config.clone(),
        db.clone(),
        layout.clone(),
        events,
        jobs,
        settings.clone(),
        library,
        torrentd,
    ));
    manager.set_tuning(fast());
    if opts.allow_local {
        manager.allow_destination("127.0.0.1".parse().unwrap());
    }
    let uploads = Arc::new(UploadService::new(db.clone(), layout, manager.clone()));
    // The test machine's free space must not decide whether small uploads fit.
    uploads.set_space_margin(0);
    let owner = add_user(&db, Role::Owner).await;
    let member = add_user(&db, Role::Member).await;
    let cancel = CancellationToken::new();
    if opts.start {
        manager.start(cancel.clone());
    }
    Env {
        _dir: dir,
        root,
        config_dir,
        db,
        settings,
        manager,
        uploads,
        cancel,
        owner,
        member,
        engine,
    }
}

impl Env {
    async fn row(&self, id: &str) -> DownloadRow {
        self.manager.load_row(id).await.unwrap()
    }

    async fn add(&self, url: &str) -> stash_core::models::Download {
        self.manager
            .add_url(
                &self.owner.user_id,
                AddDownloadRequest {
                    url: url.into(),
                    filename: None,
                    headers: None,
                },
            )
            .await
            .unwrap()
    }

    /// Sort jobs as `(rel_path, download_id)`.
    async fn sort_jobs(&self) -> Vec<(String, Option<String>)> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT payload FROM jobs WHERE kind = 'sort'")
            .fetch_all(&self.db)
            .await
            .unwrap();
        rows.into_iter()
            .map(|(p,)| {
                let v: serde_json::Value = serde_json::from_str(&p).unwrap();
                (
                    v["rel_path"].as_str().unwrap_or_default().to_string(),
                    v["download_id"].as_str().map(str::to_string),
                )
            })
            .collect()
    }

    fn engine(&self) -> &FakeEngine {
        self.engine.as_ref().unwrap()
    }

    fn in_progress(&self) -> PathBuf {
        self.root.join("Downloads/In Progress")
    }

    fn completed(&self) -> PathBuf {
        self.root.join("Downloads/Completed")
    }

    /// Partial browser uploads: outside the folder the torrent engine may write.
    fn uploads_dir(&self) -> PathBuf {
        self.root.join(".popcornstash/uploads")
    }
}

fn mode(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o7777
}

/// Root can hand a file to another user (the torrent engine's uid in production).
fn running_as_root() -> bool {
    nix::unistd::geteuid().is_root()
}

// --- fake file server ----------------------------------------------------------------

#[derive(Debug, Clone)]
struct Seen {
    path: String,
    range: Option<String>,
    cookie: Option<String>,
    referer: Option<String>,
    user_agent: Option<String>,
}

#[derive(Clone)]
struct Files {
    seen: Arc<Mutex<Vec<Seen>>>,
    big: Arc<Vec<u8>>,
    port: Arc<OnceLock<u16>>,
    flaky_dropped: Arc<AtomicBool>,
    hits_500: Arc<AtomicU32>,
}

struct FileServer {
    addr: SocketAddr,
    files: Files,
}

impl FileServer {
    async fn start() -> Self {
        let files = Files {
            seen: Arc::default(),
            big: Arc::new(payload(3 * 1024 * 1024 + 17)),
            port: Arc::default(),
            flaky_dropped: Arc::default(),
            hits_500: Arc::default(),
        };
        let router = Router::new()
            .fallback(any(serve_file))
            .with_state(files.clone());
        let addr = serve(router).await;
        let _ = files.port.set(addr.port());
        Self { addr, files }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    fn seen(&self, path: &str) -> Vec<Seen> {
        self.files
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.path == path)
            .cloned()
            .collect()
    }
}

async fn serve(router: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    addr
}

fn ranged(data: &[u8], range: Option<&str>, slow: bool) -> Response {
    let len = data.len();
    let start = range.and_then(|r| {
        r.strip_prefix("bytes=")?
            .strip_suffix('-')?
            .parse::<usize>()
            .ok()
    });
    if let Some(start) = start
        && start >= len
    {
        return (
            StatusCode::RANGE_NOT_SATISFIABLE,
            [(header::CONTENT_RANGE, format!("bytes */{len}"))],
        )
            .into_response();
    }
    let from = start.unwrap_or(0);
    let mut builder = Response::builder()
        .status(if start.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::CONTENT_LENGTH, len - from)
        .header(header::ACCEPT_RANGES, "bytes");
    if start.is_some() {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {from}-{}/{len}", len - 1),
        );
    }
    let rest = data[from..].to_vec();
    let body = if slow {
        let chunks: Vec<Bytes> = rest.chunks(32 * 1024).map(Bytes::copy_from_slice).collect();
        Body::from_stream(futures::stream::iter(chunks).then(|chunk| async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            Ok::<_, std::io::Error>(chunk)
        }))
    } else {
        Body::from(rest)
    };
    builder.body(body).unwrap()
}

async fn serve_file(State(files): State<Files>, uri: Uri, headers: HeaderMap) -> Response {
    let get = |name: header::HeaderName| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let range = get(header::RANGE);
    files.seen.lock().unwrap().push(Seen {
        path: uri.path().to_string(),
        range: range.clone(),
        cookie: get(header::COOKIE),
        referer: get(header::REFERER),
        user_agent: get(header::USER_AGENT),
    });
    match uri.path() {
        "/file.bin" | "/echo.bin" | "/query.bin" | "/get/" => ranged(&files.big, range.as_deref(), false),
        "/slow.bin" => ranged(&files.big, range.as_deref(), true),
        "/flaky.bin" => {
            if files.flaky_dropped.swap(true, Ordering::SeqCst) {
                return ranged(&files.big, range.as_deref(), false);
            }
            // First request: half the body, then the connection breaks.
            let half = Bytes::copy_from_slice(&files.big[..files.big.len() / 2]);
            let stream = futures::stream::iter(vec![Ok(half), Err(std::io::Error::other("connection reset"))]);
            Response::builder()
                .header(header::CONTENT_LENGTH, files.big.len())
                .body(Body::from_stream(stream))
                .unwrap()
        }
        "/named" => Response::builder()
            .header(
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"fallback.mp4\"; filename*=UTF-8''Na%C3%AFve%20Beach%20Day.mp4",
            )
            .body(Body::from(payload(1000)))
            .unwrap(),
        "/redirect" => (StatusCode::FOUND, [(header::LOCATION, "/file.bin")]).into_response(),
        "/redirect-local" => {
            let port = files.port.get().copied().unwrap_or(80);
            (StatusCode::FOUND, [(header::LOCATION, format!("http://127.0.0.2:{port}/file.bin"))]).into_response()
        }
        "/broken" => {
            files.hits_500.fetch_add(1, Ordering::SeqCst);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        "/sample.torrent" => Body::from(TORRENT_FILE).into_response(),
        // A sign-in page sent without a length (chunked), whatever the Range asked.
        "/chunked.bin" => {
            let page = futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from_static(
                b"<html>Sign in to continue</html>",
            ))]);
            Response::builder()
                .header(header::CONTENT_TYPE, "text/html")
                .body(Body::from_stream(page))
                .unwrap()
        }
        // Starts a .torrent, then never sends another byte.
        "/stall.torrent" => {
            let stalled = futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from_static(
                b"d8:announce",
            ))])
            .chain(futures::stream::pending());
            Body::from_stream(stalled).into_response()
        }
        "/fake.torrent" => Body::from("<html>sign in</html>").into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

// --- fake torrent engine ------------------------------------------------------------

#[derive(Default)]
struct EngineInner {
    blocked: bool,
    torrents: Vec<EngineTorrent>,
    added: Vec<AddTorrentRequest>,
    removed: Vec<(String, bool)>,
    limits: Vec<LimitsRequest>,
    next: u32,
}

#[derive(Clone)]
struct EngineShared {
    token: String,
    root: PathBuf,
    inner: Arc<Mutex<EngineInner>>,
}

struct FakeEngine {
    url: Url,
    shared: EngineShared,
}

impl FakeEngine {
    async fn start(token: String, root: PathBuf) -> Self {
        let shared = EngineShared {
            token,
            root,
            inner: Arc::default(),
        };
        let router = Router::new()
            .route("/health", get(engine_health))
            .route("/torrents", get(engine_list).post(engine_add))
            .route("/torrents/{id}", axum::routing::delete(engine_remove))
            .route("/torrents/{id}/{action}", post(engine_action))
            .route("/limits", put(engine_limits))
            .with_state(shared.clone());
        let addr = serve(router).await;
        Self {
            url: Url::parse(&format!("http://{addr}")).unwrap(),
            shared,
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut EngineInner) -> T) -> T {
        f(&mut self.shared.inner.lock().unwrap())
    }

    fn set_blocked(&self, blocked: bool) {
        self.with(|e| e.blocked = blocked);
    }

    fn finish(&self, id: &str, uploaded: u64) {
        self.with(|e| {
            let t = e.torrents.iter_mut().find(|t| t.id == id).unwrap();
            t.done_bytes = t.total_bytes.unwrap();
            t.progress = 1.0;
            t.state = EngineTorrentState::Seeding;
            t.uploaded_bytes = uploaded;
            t.eta_s = None;
        });
    }

    fn set_uploaded(&self, id: &str, uploaded: u64) {
        self.with(|e| {
            e.torrents
                .iter_mut()
                .find(|t| t.id == id)
                .unwrap()
                .uploaded_bytes = uploaded
        });
    }

    fn forget(&self, id: &str) {
        self.with(|e| e.torrents.retain(|t| t.id != id));
    }
}

fn authorized(shared: &EngineShared, headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        == Some(&format!("Bearer {}", shared.token))
}

async fn engine_health(State(s): State<EngineShared>, headers: HeaderMap) -> Response {
    if !authorized(&s, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let blocked = s.inner.lock().unwrap().blocked;
    Json(EngineHealth {
        vpn: TunnelStatus {
            state: if blocked {
                TunnelState::Disconnected
            } else {
                TunnelState::Connected
            },
            provider: None,
            protocol: None,
            label: None,
            server: None,
            country: None,
            interface: None,
            since: None,
            error: None,
            kill_switch: true,
            updated_at: 0,
        },
        engine: EngineState {
            state: if blocked {
                EngineRunState::Blocked
            } else {
                EngineRunState::Running
            },
            reason: None,
            allow_no_vpn: false,
        },
        public_ip: None,
        country: None,
        protected: None,
        checked_at: None,
    })
    .into_response()
}

async fn engine_list(State(s): State<EngineShared>, headers: HeaderMap) -> Response {
    if !authorized(&s, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(s.inner.lock().unwrap().torrents.clone()).into_response()
}

async fn engine_add(
    State(s): State<EngineShared>,
    headers: HeaderMap,
    Json(request): Json<AddTorrentRequest>,
) -> Response {
    if !authorized(&s, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut inner = s.inner.lock().unwrap();
    if inner.blocked {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": {"code": "vpn_unavailable", "message": "Waiting for the VPN to connect."}})),
        )
            .into_response();
    }
    inner.next += 1;
    let id = format!("t{}", inner.next);
    let info_hash = format!("{:040x}", inner.next);
    let name = request
        .magnet
        .as_deref()
        .and_then(Magnet::parse)
        .and_then(|m| m.display_name)
        .unwrap_or_else(|| "Fake Torrent".to_string());
    // Write the files the way the engine would.
    let folder = s.root.join("Downloads/In Progress").join(&name);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(format!("{name}.mkv")), vec![7u8; 1000]).unwrap();
    inner.added.push(request);
    inner.torrents.push(EngineTorrent {
        id: id.clone(),
        info_hash: info_hash.clone(),
        name: Some(name.clone()),
        state: EngineTorrentState::Downloading,
        progress: 0.0,
        total_bytes: Some(1000),
        done_bytes: 100,
        uploaded_bytes: 0,
        down_bps: 500,
        up_bps: 0,
        peers: Some(3),
        eta_s: Some(2),
        error: None,
        output_dir: Some(format!("/stash/Downloads/In Progress/{name}")),
    });
    Json(AddTorrentResponse {
        id,
        info_hash,
        name: Some(name),
    })
    .into_response()
}

#[derive(Deserialize)]
struct RemoveQuery {
    delete_files: Option<bool>,
}

async fn engine_remove(
    State(s): State<EngineShared>,
    headers: HeaderMap,
    AxPath(id): AxPath<String>,
    Query(q): Query<RemoveQuery>,
) -> Response {
    if !authorized(&s, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut inner = s.inner.lock().unwrap();
    let before = inner.torrents.len();
    inner.torrents.retain(|t| t.id != id);
    inner.removed.push((id, q.delete_files.unwrap_or(false)));
    if inner.torrents.len() == before {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::NO_CONTENT
    }
    .into_response()
}

async fn engine_action(
    State(s): State<EngineShared>,
    headers: HeaderMap,
    AxPath((id, action)): AxPath<(String, String)>,
) -> Response {
    if !authorized(&s, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut inner = s.inner.lock().unwrap();
    let Some(t) = inner.torrents.iter_mut().find(|t| t.id == id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    t.state = match action.as_str() {
        "pause" => EngineTorrentState::Paused,
        "resume" => EngineTorrentState::Downloading,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    StatusCode::NO_CONTENT.into_response()
}

async fn engine_limits(
    State(s): State<EngineShared>,
    headers: HeaderMap,
    Json(limits): Json<LimitsRequest>,
) -> Response {
    if !authorized(&s, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    s.inner.lock().unwrap().limits.push(limits);
    StatusCode::NO_CONTENT.into_response()
}

// --- web downloads -------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn web_download_lands_in_completed_and_queues_sort() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(&files.url("/file.bin")).await;
    assert_eq!(d.kind, DownloadKind::Http);
    assert_eq!(d.state, DownloadState::Queued);
    assert_eq!(d.name, "file.bin");

    wait_until!(
        "the download to be sorting",
        env.row(&d.id).await.state == DownloadState::Sorting
    );
    let row = env.row(&d.id).await;
    assert_eq!(
        row.rel_path.as_deref(),
        Some("Downloads/Completed/file.bin")
    );
    assert_eq!(row.done_bytes, files.files.big.len() as i64);
    assert_eq!(row.total_bytes, Some(files.files.big.len() as i64));
    assert!(row.completed_at.is_some());
    assert_eq!(
        std::fs::read(env.completed().join("file.bin")).unwrap(),
        *files.files.big
    );
    assert!(!env.in_progress().join("file.bin.part").exists());
    assert!(!env.in_progress().join("file.bin").exists());
    assert_eq!(
        env.sort_jobs().await,
        vec![(
            "Downloads/Completed/file.bin".to_string(),
            Some(d.id.clone())
        )]
    );

    // Same name again: a free name in Completed, nothing overwritten.
    let again = env.add(&files.url("/redirect")).await;
    wait_until!(
        "the second download",
        env.row(&again.id).await.state == DownloadState::Sorting
    );
    assert_eq!(
        env.row(&again.id).await.rel_path.as_deref(),
        Some("Downloads/Completed/file (2).bin")
    );

    let listed = env.manager.list(DownloadFilter::Active, 50).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert!(
        env.manager
            .list(DownloadFilter::Finished, 50)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupted_download_resumes_from_the_part_file() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(&files.url("/flaky.bin")).await;
    wait_until!(
        "the download to finish",
        env.row(&d.id).await.state == DownloadState::Sorting
    );
    let seen = files.seen("/flaky.bin");
    assert_eq!(
        seen.len(),
        2,
        "one broken request, one resumed request: {seen:?}"
    );
    assert_eq!(seen[0].range, None);
    let resumed_at: u64 = seen[1]
        .range
        .as_deref()
        .unwrap()
        .trim_start_matches("bytes=")
        .trim_end_matches('-')
        .parse()
        .unwrap();
    assert!(resumed_at > 0, "the retry must resume, not restart");
    assert_eq!(
        std::fs::read(env.completed().join("flaky.bin")).unwrap(),
        *files.files.big
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_interrupted_by_a_restart_resumes() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: false,
        ..Default::default()
    })
    .await;
    // A row and .part left behind by a previous run.
    let part = env.in_progress().join("file.bin.part");
    std::fs::write(&part, &files.files.big[..1024 * 1024]).unwrap();
    let id = stash_core::new_id();
    let t = stash_core::now();
    sqlx::query(
        "INSERT INTO downloads (id, kind, name, source, state, progress, done_bytes, rel_path, created_by, created_at, updated_at)
         VALUES (?, 'http', 'file.bin', ?, 'downloading', 0.3, 1048576, 'Downloads/In Progress/file.bin.part', ?, ?, ?)",
    )
    .bind(&id)
    .bind(files.url("/file.bin"))
    .bind(&env.owner.user_id)
    .bind(t)
    .bind(t)
    .execute(&env.db)
    .await
    .unwrap();
    env.manager.start(env.cancel.clone());
    wait_until!(
        "the resumed download",
        env.row(&id).await.state == DownloadState::Sorting
    );
    let seen = files.seen("/file.bin");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].range.as_deref(), Some("bytes=1048576-"));
    assert_eq!(
        std::fs::read(env.completed().join("file.bin")).unwrap(),
        *files.files.big
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_refuses_a_file_that_changed_on_the_server() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: false,
        ..Default::default()
    })
    .await;
    let part = env.in_progress().join("file.bin.part");
    std::fs::write(&part, b"0123456789").unwrap();
    let id = stash_core::new_id();
    let t = stash_core::now();
    // The first run saw a 5000-byte file; the server now has a different one.
    sqlx::query(
        "INSERT INTO downloads (id, kind, name, source, state, total_bytes, done_bytes, rel_path, created_by, created_at, updated_at)
         VALUES (?, 'http', 'file.bin', ?, 'queued', 5000, 10, 'Downloads/In Progress/file.bin.part', ?, ?, ?)",
    )
    .bind(&id)
    .bind(files.url("/file.bin"))
    .bind(&env.owner.user_id)
    .bind(t)
    .bind(t)
    .execute(&env.db)
    .await
    .unwrap();
    env.manager.start(env.cancel.clone());
    wait_until!(
        "the download to fail",
        env.row(&id).await.state == DownloadState::Failed
    );
    assert_eq!(
        env.row(&id).await.error.as_deref(),
        Some("The file on the server changed since the download started. Add the link again.")
    );
    assert_eq!(
        std::fs::read(&part).unwrap(),
        b"0123456789",
        "the partial file is untouched"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn content_disposition_names_the_file() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(&files.url("/named")).await;
    assert_eq!(d.name, "named", "the URL names it until the headers arrive");
    wait_until!(
        "the named download",
        env.row(&d.id).await.state == DownloadState::Sorting
    );
    let row = env.row(&d.id).await;
    assert_eq!(row.name, "Naïve Beach Day.mp4");
    assert_eq!(
        row.rel_path.as_deref(),
        Some("Downloads/Completed/Naïve Beach Day.mp4")
    );
    assert!(env.completed().join("Naïve Beach Day.mp4").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chosen_filename_wins() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env
        .manager
        .add_url(
            &env.owner.user_id,
            AddDownloadRequest {
                url: files.url("/named"),
                filename: Some("../Holiday.mp4".into()),
                headers: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(d.name, "Holiday.mp4");
    wait_until!(
        "the download",
        env.row(&d.id).await.state == DownloadState::Sorting
    );
    assert!(env.completed().join("Holiday.mp4").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssrf_guard_refuses_local_destinations() {
    let files = FileServer::start().await;
    let env = env(Opts {
        start: true,
        ..Default::default()
    })
    .await;
    for url in [
        files.url("/file.bin"),
        "http://localhost:8788/api/health".to_string(),
        "http://[::1]:9091/torrents".to_string(),
        "http://169.254.169.254/latest/meta-data/".to_string(),
        "http://10.0.0.1/".to_string(),
        "http://192.168.1.1/admin".to_string(),
    ] {
        let err = env
            .manager
            .add_url(
                &env.owner.user_id,
                AddDownloadRequest {
                    url: url.clone(),
                    filename: None,
                    headers: None,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest, "{url}");
        assert_eq!(err.message, super::ssrf::MSG_BLOCKED, "{url}");
    }
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM downloads")
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
    assert!(files.seen("/file.bin").is_empty());

    for bad in [
        "ftp://example.com/x",
        "file:///etc/passwd",
        "not a link",
        "javascript:alert(1)",
    ] {
        let err = env
            .manager
            .add_url(
                &env.owner.user_id,
                AddDownloadRequest {
                    url: bad.into(),
                    filename: None,
                    headers: None,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest, "{bad}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redirects_to_local_addresses_are_refused() {
    let files = FileServer::start().await;
    // Only the test server itself is trusted; the redirect target is not.
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(&files.url("/redirect-local")).await;
    wait_until!(
        "the download to fail",
        env.row(&d.id).await.state == DownloadState::Failed
    );
    assert_eq!(
        env.row(&d.id).await.error.as_deref(),
        Some(super::ssrf::MSG_BLOCKED)
    );
    assert!(files.seen("/file.bin").is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_errors_fail_without_retrying() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(&files.url("/missing")).await;
    wait_until!(
        "the download to fail",
        env.row(&d.id).await.state == DownloadState::Failed
    );
    let row = env.row(&d.id).await;
    assert!(
        row.error.as_deref().unwrap().contains("404"),
        "{:?}",
        row.error
    );
    assert_eq!(files.seen("/missing").len(), 1);
    assert!(env.sort_jobs().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_errors_retry_three_times() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(&files.url("/broken")).await;
    wait_until!(
        "the download to fail",
        env.row(&d.id).await.state == DownloadState::Failed
    );
    assert_eq!(files.files.hits_500.load(Ordering::SeqCst), 3);
    assert!(env.row(&d.id).await.error.unwrap().contains("500"));

    // A failed web download can be retried.
    env.manager.resume(&d.id).await.unwrap();
    wait_until!(
        "the retry to fail",
        files.files.hits_500.load(Ordering::SeqCst) == 6
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forwarded_headers_reach_the_server_but_are_never_stored() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env
        .manager
        .add_url(
            &env.owner.user_id,
            AddDownloadRequest {
                url: files.url("/query.bin?id=7&token=SECRETTOKEN"),
                filename: None,
                headers: Some(ForwardedHeaders {
                    cookie: Some("session=SECRETCOOKIE".into()),
                    referer: Some("https://example.com/SECRETREFERER".into()),
                    user_agent: Some("SecretAgent/1.0 SECRETUA".into()),
                }),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        d.source,
        format!("{}#redacted", files.url("/query.bin")),
        "query with a token is not stored"
    );
    wait_until!(
        "the download",
        env.row(&d.id).await.state == DownloadState::Sorting
    );
    let seen = files.seen("/query.bin");
    assert_eq!(seen[0].cookie.as_deref(), Some("session=SECRETCOOKIE"));
    assert_eq!(
        seen[0].referer.as_deref(),
        Some("https://example.com/SECRETREFERER")
    );
    assert_eq!(
        seen[0].user_agent.as_deref(),
        Some("SecretAgent/1.0 SECRETUA")
    );

    // Nothing secret anywhere in the database files.
    sqlx::query("PRAGMA wal_checkpoint(FULL)")
        .execute(&env.db)
        .await
        .unwrap();
    for entry in std::fs::read_dir(&env.config_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(&path).unwrap();
            for secret in ["SECRETTOKEN", "SECRETCOOKIE", "SECRETREFERER", "SECRETUA"] {
                assert!(
                    !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                    "{secret} found in {}",
                    path.display()
                );
            }
        }
    }
    // Finished downloads drop the in-memory copy too.
    assert!(!env.manager.secrets.contains_key(&d.id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_keeps_the_part_and_resume_continues() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(&files.url("/slow.bin")).await;
    let part = env.in_progress().join("slow.bin.part");
    wait_until!("the part file", part.exists());
    tokio::time::sleep(Duration::from_millis(300)).await;
    let paused = env.manager.pause(&d.id).await.unwrap();
    assert_eq!(paused.state, DownloadState::Paused);
    wait_until!(
        "the worker to stop",
        !env.manager.running.contains_key(&d.id)
    );
    let kept = std::fs::metadata(&part).unwrap().len();
    assert!(
        kept > 0 && kept < files.files.big.len() as u64,
        "kept {kept} bytes"
    );
    assert_eq!(env.row(&d.id).await.state, DownloadState::Paused);

    env.manager.resume(&d.id).await.unwrap();
    wait_until!(
        "the resumed download",
        env.row(&d.id).await.state == DownloadState::Sorting
    );
    let seen = files.seen("/slow.bin");
    assert_eq!(
        seen.last().unwrap().range.as_deref(),
        Some(format!("bytes={kept}-").as_str())
    );
    assert_eq!(
        std::fs::read(env.completed().join("slow.bin")).unwrap(),
        *files.files.big
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remove_deletes_partial_files_but_never_library_files() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(&files.url("/slow.bin")).await;
    let part = env.in_progress().join("slow.bin.part");
    wait_until!("the part file", part.exists());
    env.manager.remove(&d.id, true).await.unwrap();
    assert!(!part.exists());
    assert_eq!(
        env.manager.get(&d.id).await.unwrap_err().code,
        ErrorCode::NotFound
    );

    // A row pointing into the library (it cannot happen, but must still be safe).
    let movie = env.root.join("Movies/Heat (1995)/Heat (1995).mkv");
    std::fs::create_dir_all(movie.parent().unwrap()).unwrap();
    std::fs::write(&movie, b"precious").unwrap();
    let id = stash_core::new_id();
    let t = stash_core::now();
    sqlx::query(
        "INSERT INTO downloads (id, kind, name, source, state, rel_path, created_at, updated_at)
         VALUES (?, 'http', 'Heat', 'x', 'failed', 'Movies/Heat (1995)/Heat (1995).mkv', ?, ?)",
    )
    .bind(&id)
    .bind(t)
    .bind(t)
    .execute(&env.db)
    .await
    .unwrap();
    env.manager.remove(&id, true).await.unwrap();
    assert_eq!(std::fs::read(&movie).unwrap(), b"precious");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrency_limit_and_unique_names() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: false,
        ..Default::default()
    })
    .await;
    env.settings
        .update(serde_json::json!({"max_concurrent_downloads": 1}))
        .await
        .unwrap();
    env.manager.start(env.cancel.clone());
    let first = env.add(&files.url("/slow.bin")).await;
    let second = env.add(&files.url("/slow.bin")).await;
    wait_until!(
        "the first download to start",
        env.row(&first.id).await.state == DownloadState::Downloading
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        env.row(&second.id).await.state,
        DownloadState::Queued,
        "only one at a time"
    );
    wait_until!(
        "both downloads",
        env.row(&second.id).await.state == DownloadState::Sorting
    );
    assert_eq!(
        env.row(&first.id).await.rel_path.as_deref(),
        Some("Downloads/Completed/slow.bin")
    );
    assert_eq!(
        env.row(&second.id).await.rel_path.as_deref(),
        Some("Downloads/Completed/slow (2).bin")
    );
    assert_eq!(
        std::fs::read(env.completed().join("slow (2).bin")).unwrap(),
        *files.files.big
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_state_change_is_published() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let mut events = env.manager.events.subscribe();
    let d = env.add(&files.url("/file.bin")).await;
    let mut states = Vec::new();
    // Until the last event arrives (the row changes just before its event goes out).
    while states.last() != Some(&DownloadState::Sorting) {
        let event = tokio::time::timeout(Duration::from_secs(20), events.recv())
            .await
            .expect("the sorting event")
            .unwrap();
        if let stash_core::events::Event::Download(download) = event {
            assert_eq!(download.id, d.id);
            if states.last() != Some(&download.state) {
                states.push(download.state);
            }
        }
    }
    assert_eq!(
        states,
        vec![
            DownloadState::Queued,
            DownloadState::Downloading,
            DownloadState::Completed,
            DownloadState::Sorting
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn always_via_vpn_fails_when_the_vpn_is_down() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    env.settings
        .update(serde_json::json!({"http_via_vpn": "always"}))
        .await
        .unwrap();
    let d = env.add(&files.url("/file.bin")).await;
    wait_until!(
        "the download to fail",
        env.row(&d.id).await.state == DownloadState::Failed
    );
    assert_eq!(
        env.row(&d.id).await.error.as_deref(),
        Some(MSG_VPN_REQUIRED)
    );
    assert!(files.seen("/file.bin").is_empty());
}

// --- torrents -------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn torrents_disabled_answer_vpn_unavailable() {
    let env = env(Opts {
        start: true,
        ..Default::default()
    })
    .await;
    let err = env
        .manager
        .add_url(
            &env.owner.user_id,
            AddDownloadRequest {
                url: MAGNET.into(),
                filename: None,
                headers: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::VpnUnavailable);
    assert_eq!(err.message, "Torrents are turned off on this server.");
    let err = env
        .manager
        .add_torrent_file(&env.owner.user_id, TORRENT_FILE.to_vec(), None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::VpnUnavailable);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn magnet_completes_into_completed_and_leaves_the_engine() {
    let env = env(Opts {
        engine: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(MAGNET).await;
    assert_eq!(d.kind, DownloadKind::Torrent);
    assert_eq!(d.state, DownloadState::Downloading);
    assert_eq!(d.name, "Big Buck Bunny");
    assert!(
        !d.source.contains("SECRETPASSKEY"),
        "tracker passkeys are not stored: {}",
        d.source
    );
    let engine_id = env.row(&d.id).await.engine_id.unwrap();
    assert_eq!(
        env.engine().with(|e| e.added[0].magnet.clone()).as_deref(),
        Some(MAGNET)
    );

    wait_until!(
        "the speed limits",
        !env.engine().with(|e| e.limits.is_empty())
    );
    wait_until!(
        "progress from the engine",
        env.row(&d.id).await.done_bytes == 100
    );
    let row = env.row(&d.id).await;
    assert_eq!(row.peers, Some(3));
    assert_eq!(
        row.rel_path.as_deref(),
        Some("Downloads/In Progress/Big Buck Bunny")
    );

    env.engine().finish(&engine_id, 0);
    wait_until!(
        "the torrent to be sorting",
        env.row(&d.id).await.state == DownloadState::Sorting
    );
    assert_eq!(
        env.engine().with(|e| e.removed.clone()),
        vec![(engine_id, false)]
    );
    assert!(
        env.completed()
            .join("Big Buck Bunny/Big Buck Bunny.mkv")
            .exists()
    );
    assert!(!env.in_progress().join("Big Buck Bunny").exists());
    let row = env.row(&d.id).await;
    assert_eq!(
        row.rel_path.as_deref(),
        Some("Downloads/Completed/Big Buck Bunny")
    );
    assert_eq!(row.progress, 1.0);
    assert_eq!(
        env.sort_jobs().await,
        vec![(
            "Downloads/Completed/Big Buck Bunny".to_string(),
            Some(d.id.clone())
        )]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seeding_policy_is_respected() {
    let env = env(Opts {
        engine: true,
        start: true,
        ..Default::default()
    })
    .await;
    env.settings
        .update(serde_json::json!({"torrent": {"seed_after_complete": true, "seed_ratio": 2.0, "seed_hours": 24}}))
        .await
        .unwrap();
    let d = env.add(MAGNET).await;
    let engine_id = env.row(&d.id).await.engine_id.unwrap();
    env.engine().finish(&engine_id, 500);
    wait_until!(
        "seeding",
        env.row(&d.id).await.state == DownloadState::Seeding
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(env.row(&d.id).await.state, DownloadState::Seeding);
    assert!(env.row(&d.id).await.completed_at.is_some());
    assert!(env.engine().with(|e| e.removed.is_empty()));

    // Ratio reached: finish like any other torrent.
    env.engine().set_uploaded(&engine_id, 2000);
    wait_until!(
        "sorting",
        env.row(&d.id).await.state == DownloadState::Sorting
    );
    assert_eq!(
        env.engine().with(|e| e.removed.clone()),
        vec![(engine_id, false)]
    );
    assert!(env.completed().join("Big Buck Bunny").is_dir());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn torrent_added_while_the_vpn_is_down_waits_then_starts() {
    let env = env(Opts {
        engine: true,
        start: true,
        ..Default::default()
    })
    .await;
    env.engine().set_blocked(true);
    let d = env.add(MAGNET).await;
    assert_eq!(d.state, DownloadState::Queued);
    assert_eq!(d.error.as_deref(), Some(MSG_WAITING_FOR_VPN));
    assert!(env.manager.pending_path(&d.id).exists());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(env.row(&d.id).await.state, DownloadState::Queued);

    env.engine().set_blocked(false);
    wait_until!(
        "the engine to take it",
        env.row(&d.id).await.state == DownloadState::Downloading
    );
    let row = env.row(&d.id).await;
    assert!(row.engine_id.is_some());
    assert_eq!(row.error, None);
    assert!(!env.manager.pending_path(&d.id).exists());
    assert_eq!(env.engine().with(|e| e.torrents.len()), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn torrent_missing_from_the_engine_fails() {
    let env = env(Opts {
        engine: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(MAGNET).await;
    let engine_id = env.row(&d.id).await.engine_id.unwrap();
    env.engine().forget(&engine_id);
    wait_until!(
        "the torrent to fail",
        env.row(&d.id).await.state == DownloadState::Failed
    );
    assert_eq!(env.row(&d.id).await.error.as_deref(), Some(MSG_LOST));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn torrent_pause_resume_and_remove() {
    let env = env(Opts {
        engine: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(MAGNET).await;
    let engine_id = env.row(&d.id).await.engine_id.unwrap();
    assert_eq!(
        env.manager.pause(&d.id).await.unwrap().state,
        DownloadState::Paused
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        env.row(&d.id).await.state,
        DownloadState::Paused,
        "the poller mirrors the paused engine"
    );
    assert_eq!(
        env.manager.resume(&d.id).await.unwrap().state,
        DownloadState::Downloading
    );
    env.manager.remove(&d.id, true).await.unwrap();
    assert_eq!(
        env.engine().with(|e| e.removed.clone()),
        vec![(engine_id, true)]
    );
    assert!(!env.in_progress().join("Big Buck Bunny").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn torrent_files_are_validated_and_added() {
    let files = FileServer::start().await;
    let env = env(Opts {
        engine: true,
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let err = env
        .manager
        .add_torrent_file(
            &env.owner.user_id,
            b"<html>nope</html>".to_vec(),
            Some("x.torrent".into()),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);

    let d = env
        .manager
        .add_torrent_file(
            &env.owner.user_id,
            TORRENT_FILE.to_vec(),
            Some("movie.torrent".into()),
        )
        .await
        .unwrap();
    assert_eq!(d.kind, DownloadKind::Torrent);
    assert_eq!(d.source, "movie.torrent");
    use base64::Engine as _;
    let sent = env
        .engine()
        .with(|e| e.added[0].torrent_base64.clone())
        .unwrap();
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(sent)
            .unwrap(),
        TORRENT_FILE
    );

    // A .torrent link is fetched (with the same destination checks) and added.
    let linked = env.add(&files.url("/sample.torrent")).await;
    assert_eq!(linked.kind, DownloadKind::Torrent);
    assert_eq!(env.engine().with(|e| e.added.len()), 2);
    let err = env
        .manager
        .add_url(
            &env.owner.user_id,
            AddDownloadRequest {
                url: files.url("/fake.torrent"),
                filename: None,
                headers: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
}

// --- uploads -------------------------------------------------------------------------

fn create(name: &str, size: i64) -> CreateUpload {
    CreateUpload {
        filename: name.into(),
        size,
        mime: Some("video/mp4".into()),
        last_modified: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_in_chunks_resumes_and_completes() {
    let env = env(Opts::default()).await;
    let user = &env.member;
    let data = payload(40_000);
    let created = env
        .uploads
        .create(user, create("Beach Day.mp4", data.len() as i64))
        .await
        .unwrap();
    assert_eq!(created.chunk_size, CHUNK_SIZE);
    assert_eq!(created.received, 0);
    let part = env.uploads_dir().join(format!("{}.part", created.id));
    assert!(part.exists());

    let put = |offset: usize, bytes: &[u8]| {
        let body = Body::from(bytes.to_vec());
        let len = bytes.len() as u64;
        env.uploads
            .write_chunk(user, &created.id, offset as i64, Some(len), body)
    };
    assert_eq!(put(0, &data[..16_000]).await.unwrap(), 16_000);

    // A stale offset answers 409 with what the server has.
    let err = put(0, &data[..16_000]).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(err.extra.unwrap()["received"], 16_000);

    // A dropped connection keeps the bytes that arrived.
    let broken = Body::from_stream(futures::stream::iter(vec![
        Ok(Bytes::copy_from_slice(&data[16_000..20_000])),
        Err(std::io::Error::other("connection reset")),
    ]));
    let err = env
        .uploads
        .write_chunk(user, &created.id, 16_000, None, broken)
        .await
        .unwrap_err();
    assert_eq!(err.extra.unwrap()["received"], 20_000);
    let status = env.uploads.status(user, &created.id).await.unwrap();
    assert_eq!(
        (status.received, status.state.as_str()),
        (20_000, "receiving")
    );

    // Completing early is refused.
    let err = env.uploads.complete(user, &created.id).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);

    // Past the end is refused.
    let err = put(20_000, &payload(30_000)).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);

    assert_eq!(put(20_000, &data[20_000..]).await.unwrap(), 40_000);
    let download = env.uploads.complete(user, &created.id).await.unwrap();
    assert_eq!(download.kind, DownloadKind::Upload);
    assert_eq!(download.state, DownloadState::Sorting);
    assert_eq!(download.name, "Beach Day.mp4");
    assert_eq!(download.total_bytes, Some(40_000));
    assert_eq!(
        std::fs::read(env.completed().join("Beach Day.mp4")).unwrap(),
        data
    );
    assert!(!part.exists());
    assert!(!env.in_progress().join("Beach Day.mp4").exists());
    assert_eq!(
        env.sort_jobs().await,
        vec![(
            "Downloads/Completed/Beach Day.mp4".to_string(),
            Some(download.id.clone())
        )]
    );
    assert_eq!(
        env.row(&download.id).await.created_by.as_deref(),
        Some(user.user_id.as_str())
    );
    assert_eq!(
        env.uploads.status(user, &created.id).await.unwrap().state,
        "complete"
    );
    assert_eq!(
        env.uploads
            .complete(user, &created.id)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uploads_belong_to_their_uploader() {
    let env = env(Opts::default()).await;
    let other = add_user(&env.db, Role::Member).await;
    let created = env
        .uploads
        .create(&env.member, create("mine.mp4", 10))
        .await
        .unwrap();
    let id = created.id.as_str();
    assert_eq!(
        env.uploads.status(&other, id).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    let err = env
        .uploads
        .write_chunk(&other, id, 0, Some(10), Body::from(vec![1u8; 10]))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert_eq!(
        env.uploads.complete(&other, id).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        env.uploads.cancel(&other, id).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        env.uploads.status(&env.member, id).await.unwrap().received,
        0
    );
    // The owner can see and cancel anyone's upload.
    assert_eq!(
        env.uploads.status(&env.owner, id).await.unwrap().filename,
        "mine.mp4"
    );
    env.uploads.cancel(&env.owner, id).await.unwrap();
    assert_eq!(
        env.uploads.status(&env.member, id).await.unwrap().state,
        "cancelled"
    );
    assert!(!env.uploads_dir().join(format!("{id}.part")).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uploads_need_free_space() {
    let env = env(Opts::default()).await;
    let err = env
        .uploads
        .create(&env.member, create("huge.mkv", i64::MAX / 4))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(
        err.message,
        "There is not enough free space on the stash for this file."
    );
    let err = env
        .uploads
        .create(&env.member, create("  ", 10))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_uploads_are_cleaned_up() {
    let env = env(Opts::default()).await;
    let old = env
        .uploads
        .create(&env.member, create("old.mp4", 10))
        .await
        .unwrap();
    let fresh = env
        .uploads
        .create(&env.member, create("fresh.mp4", 10))
        .await
        .unwrap();
    sqlx::query("UPDATE uploads SET updated_at = updated_at - 8 * 86400 WHERE id = ?")
        .bind(&old.id)
        .execute(&env.db)
        .await
        .unwrap();
    assert_eq!(
        env.uploads
            .cleanup(crate::uploads::STALE_AFTER_S)
            .await
            .unwrap(),
        1
    );
    assert!(!env.uploads_dir().join(format!("{}.part", old.id)).exists());
    assert!(
        env.uploads_dir()
            .join(format!("{}.part", fresh.id))
            .exists()
    );
    assert_eq!(
        env.uploads
            .status(&env.member, &old.id)
            .await
            .unwrap()
            .state,
        "cancelled"
    );
}

// --- HTTP API through the real router ---------------------------------------------------

async fn api_state() -> (tempfile::TempDir, crate::state::AppState, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let state = crate::app::test_state(dir.path()).await.unwrap();
    let owner = add_user(&state.db, Role::Owner).await;
    let mut conn = state.db.acquire().await.unwrap();
    let full = crate::auth::tokens::insert_token(
        &mut conn,
        &owner.user_id,
        "test",
        &[Scope::Full],
        "other",
    )
    .await
    .unwrap()
    .token;
    let reader = crate::auth::tokens::insert_token(
        &mut conn,
        &owner.user_id,
        "reader",
        &[Scope::DownloadsRead],
        "chrome-extension",
    )
    .await
    .unwrap()
    .token;
    drop(conn);
    // The test machine's free space must not decide whether small uploads fit.
    state.uploads.set_space_margin(0);
    (dir, state, full, reader)
}

async fn call(
    router: &Router,
    method: &str,
    uri: &str,
    token: &str,
    content_type: &str,
    body: Body,
) -> (StatusCode, serde_json::Value) {
    use tower::ServiceExt;
    let request = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_accepts_a_20_mib_upload_chunk() {
    let (_dir, state, token, _) = api_state().await;
    let router = crate::app::router(state);
    let size = 20 * 1024 * 1024;
    let body = serde_json::json!({"filename": "big.mov", "size": size}).to_string();
    let (status, created) = call(
        &router,
        "POST",
        "/api/uploads",
        &token,
        "application/json",
        Body::from(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["chunk_size"], CHUNK_SIZE);
    let id = created["id"].as_str().unwrap().to_string();

    // 20 MiB is ten times the router-wide 2 MiB default: the route-level limit applies.
    let chunk = vec![42u8; size];
    let uri = format!("/api/uploads/{id}?offset=0");
    let (status, answer) = call(
        &router,
        "PUT",
        &uri,
        &token,
        "application/octet-stream",
        Body::from(chunk),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["received"], size);

    let (status, answer) = call(
        &router,
        "PUT",
        &uri,
        &token,
        "application/octet-stream",
        Body::from(vec![1u8]),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(answer["error"]["code"], "conflict");
    assert_eq!(answer["error"]["received"], size);
    // Section 8: "409 with {received}". The web client reads it at the top level.
    assert_eq!(answer["received"], size);

    let (status, answer) = call(
        &router,
        "GET",
        &format!("/api/uploads/{id}"),
        &token,
        "application/json",
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["received"], size);

    let (status, download) = call(
        &router,
        "POST",
        &format!("/api/uploads/{id}/complete"),
        &token,
        "application/json",
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{download}");
    assert_eq!(download["kind"], "upload");
    assert_eq!(download["name"], "big.mov");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_download_routes() {
    let (_dir, state, token, reader) = api_state().await;
    let router = crate::app::router(state);
    let json = "application/json";

    let (status, list) = call(
        &router,
        "GET",
        "/api/downloads?state=active&limit=10",
        &reader,
        json,
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list, serde_json::json!({"items": []}));

    let body = serde_json::json!({"url": MAGNET}).to_string();
    let (status, err) = call(
        &router,
        "POST",
        "/api/downloads",
        &token,
        json,
        Body::from(body),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(err["error"]["code"], "vpn_unavailable");
    assert_eq!(
        err["error"]["message"],
        "Torrents are turned off on this server."
    );

    // Read-only devices cannot add or control downloads.
    let body = serde_json::json!({"url": "https://example.com/a.mkv"}).to_string();
    let (status, _) = call(
        &router,
        "POST",
        "/api/downloads",
        &reader,
        json,
        Body::from(body),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &router,
        "POST",
        "/api/downloads/nope/pause",
        &reader,
        json,
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = call(
        &router,
        "DELETE",
        "/api/downloads/nope?delete_files=true",
        &token,
        json,
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let boundary = "XBOUNDARY";
    let multipart = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"x.torrent\"\r\nContent-Type: application/x-bittorrent\r\n\r\n{}\r\n--{boundary}--\r\n",
        String::from_utf8_lossy(TORRENT_FILE)
    );
    let (status, err) = call(
        &router,
        "POST",
        "/api/downloads/torrent",
        &token,
        &format!("multipart/form-data; boundary={boundary}"),
        Body::from(multipart),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{err}");
}

// --- review: restarts, file safety and contract edges ------------------------------------

/// Insert a `downloads` row the way an earlier run of the server left it.
#[allow(clippy::too_many_arguments)]
async fn insert_row(
    env: &Env,
    kind: &str,
    name: &str,
    source: &str,
    state: &str,
    rel_path: Option<&str>,
    total_bytes: Option<i64>,
    engine_id: Option<&str>,
) -> String {
    let id = stash_core::new_id();
    let t = stash_core::now();
    sqlx::query(
        "INSERT INTO downloads (id, kind, name, source, state, total_bytes, done_bytes, rel_path, engine_id,
                                created_by, created_at, updated_at, completed_at)
         VALUES (?, ?, ?, ?, ?, ?, COALESCE(?, 0), ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(kind)
    .bind(name)
    .bind(source)
    .bind(state)
    .bind(total_bytes)
    .bind(total_bytes)
    .bind(rel_path)
    .bind(engine_id)
    .bind(&env.owner.user_id)
    .bind(t)
    .bind(t)
    .bind(matches!(state, "completed" | "sorting" | "sorted").then_some(t))
    .execute(&env.db)
    .await
    .unwrap();
    id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn finished_torrent_left_in_in_progress_is_moved_after_a_restart() {
    let env = env(Opts {
        engine: true,
        ..Default::default()
    })
    .await;
    // The previous run had decided the torrent was done, then stopped before moving it.
    let folder = env.in_progress().join("Big Buck Bunny");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Big Buck Bunny.mkv"), vec![7u8; 1000]).unwrap();
    let id = insert_row(
        &env,
        "torrent",
        "Big Buck Bunny",
        "magnet:?xt=urn:btih:abc",
        "completed",
        Some("Downloads/In Progress/Big Buck Bunny"),
        Some(1000),
        Some("t99"),
    )
    .await;
    env.manager.start(env.cancel.clone());
    wait_until!(
        "the torrent to be sorting",
        env.row(&id).await.state == DownloadState::Sorting
    );
    assert!(
        env.completed()
            .join("Big Buck Bunny/Big Buck Bunny.mkv")
            .exists()
    );
    assert!(!folder.exists());
    assert_eq!(
        env.engine().with(|e| e.removed.clone()),
        vec![("t99".to_string(), false)],
        "the engine is asked to let go (it no longer knows the torrent)"
    );
    assert_eq!(
        env.sort_jobs().await,
        vec![(
            "Downloads/Completed/Big Buck Bunny".to_string(),
            Some(id.clone())
        )]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn finished_torrent_that_cannot_be_moved_yet_is_retried() {
    let env = env(Opts {
        engine: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(MAGNET).await;
    let engine_id = env.row(&d.id).await.engine_id.unwrap();
    // Completed is unusable for a moment: a file sits where the folder belongs.
    std::fs::remove_dir_all(env.completed()).unwrap();
    std::fs::write(env.completed(), b"in the way").unwrap();
    env.engine().finish(&engine_id, 0);
    wait_until!(
        "the engine to let go",
        !env.engine().with(|e| e.removed.is_empty())
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    let row = env.row(&d.id).await;
    assert_eq!(
        row.state,
        DownloadState::Completed,
        "not failed: {:?}",
        row.error
    );
    assert!(env.in_progress().join("Big Buck Bunny").is_dir());

    std::fs::remove_file(env.completed()).unwrap();
    std::fs::create_dir_all(env.completed()).unwrap();
    wait_until!(
        "the torrent to be sorting",
        env.row(&d.id).await.state == DownloadState::Sorting
    );
    assert!(env.completed().join("Big Buck Bunny").is_dir());
    assert_eq!(env.row(&d.id).await.error, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_link_is_not_fetched_without_its_secret_after_a_restart() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        ..Default::default()
    })
    .await;
    let d = env
        .manager
        .add_url(
            &env.owner.user_id,
            AddDownloadRequest {
                url: files.url("/query.bin?id=7&token=SECRETTOKEN"),
                filename: None,
                headers: None,
            },
        )
        .await
        .unwrap();
    assert!(!d.source.contains("SECRETTOKEN"));
    // The server restarts before the download starts: the full link was memory only.
    env.manager.secrets.clear();
    env.manager.start(env.cancel.clone());
    wait_until!(
        "the download to fail",
        env.row(&d.id).await.state == DownloadState::Failed
    );
    assert!(
        files.seen("/query.bin").is_empty(),
        "the link without its token is a different link"
    );
    let error = env.row(&d.id).await.error.unwrap();
    assert!(error.contains("Send the link again"), "{error}");

    // Retrying in place is refused, and so is re-adding the stored (incomplete) link.
    let err = env.manager.resume(&d.id).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    let err = env
        .manager
        .add_url(
            &env.owner.user_id,
            AddDownloadRequest {
                url: d.source.clone(),
                filename: None,
                headers: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    assert!(files.seen("/query.bin").is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_refuses_an_answer_of_unknown_size() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        ..Default::default()
    })
    .await;
    let part = env.in_progress().join("movie.mkv.part");
    std::fs::write(&part, b"0123456789").unwrap();
    // The first run saw a 5000-byte file; now the server answers with a page of unknown length.
    let id = insert_row(
        &env,
        "http",
        "movie.mkv",
        &files.url("/chunked.bin"),
        "queued",
        Some("Downloads/In Progress/movie.mkv.part"),
        Some(5000),
        None,
    )
    .await;
    sqlx::query("UPDATE downloads SET done_bytes = 10 WHERE id = ?")
        .bind(&id)
        .execute(&env.db)
        .await
        .unwrap();
    env.manager.start(env.cancel.clone());
    wait_until!(
        "the download to fail",
        env.row(&id).await.state == DownloadState::Failed
    );
    assert_eq!(std::fs::read(&part).unwrap(), b"0123456789");
    assert!(!env.completed().join("movie.mkv").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_symlinked_part_file_is_never_written_through() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        ..Default::default()
    })
    .await;
    // Something else that can write in In Progress (the torrent engine user) planted a
    // link to a library file. (Links leaving the stash are already refused by the layout.)
    let victim = env.root.join("Movies/Heat (1995)/Heat (1995).mkv");
    std::fs::create_dir_all(victim.parent().unwrap()).unwrap();
    std::fs::write(&victim, b"precious").unwrap();
    std::os::unix::fs::symlink(&victim, env.in_progress().join("file.bin.part")).unwrap();
    let id = insert_row(
        &env,
        "http",
        "file.bin",
        &files.url("/file.bin"),
        "queued",
        Some("Downloads/In Progress/file.bin.part"),
        None,
        None,
    )
    .await;
    env.manager.start(env.cancel.clone());
    // The link is replaced by a file of the server's own, never written through.
    wait_until!(
        "the download",
        env.row(&id).await.state == DownloadState::Sorting
    );
    assert_eq!(std::fs::read(&victim).unwrap(), b"precious");
    assert_eq!(
        std::fs::read(env.completed().join("file.bin")).unwrap(),
        *files.files.big
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_symlinked_upload_part_is_never_written_through() {
    let env = env(Opts::default()).await;
    let created = env
        .uploads
        .create(&env.member, create("clip.mp4", 10))
        .await
        .unwrap();
    let part = env.uploads_dir().join(format!("{}.part", created.id));
    let victim = env.config_dir.join("victim.db");
    std::fs::write(&victim, b"precious").unwrap();
    std::fs::remove_file(&part).unwrap();
    std::os::unix::fs::symlink(&victim, &part).unwrap();
    let result = env
        .uploads
        .write_chunk(
            &env.member,
            &created.id,
            0,
            Some(10),
            Body::from(vec![1u8; 10]),
        )
        .await;
    assert!(result.is_err());
    assert_eq!(std::fs::read(&victim).unwrap(), b"precious");
    let result = env.uploads.complete(&env.member, &created.id).await;
    assert!(result.is_err());
    assert_eq!(std::fs::read(&victim).unwrap(), b"precious");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_bytes_lost_on_disk_are_asked_for_again() {
    let env = env(Opts::default()).await;
    let user = &env.member;
    let data = payload(100);
    let created = env
        .uploads
        .create(user, create("lost.mp4", 100))
        .await
        .unwrap();
    let part = env.uploads_dir().join(format!("{}.part", created.id));
    let put = |offset: usize, bytes: &[u8]| {
        env.uploads.write_chunk(
            user,
            &created.id,
            offset as i64,
            Some(bytes.len() as u64),
            Body::from(bytes.to_vec()),
        )
    };
    assert_eq!(put(0, &data[..60]).await.unwrap(), 60);
    // A power cut lost the last bytes the database had already counted.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&part)
        .unwrap()
        .set_len(40)
        .unwrap();
    let err = put(60, &data[60..]).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(err.extra.unwrap()["received"], 40);
    assert_eq!(put(40, &data[40..]).await.unwrap(), 100);

    // The same when the loss shows up only at the end.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&part)
        .unwrap()
        .set_len(90)
        .unwrap();
    let err = env.uploads.complete(user, &created.id).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(err.extra.unwrap()["received"], 90);
    assert_eq!(put(90, &data[90..]).await.unwrap(), 100);
    env.uploads.complete(user, &created.id).await.unwrap();
    assert_eq!(
        std::fs::read(env.completed().join("lost.mp4")).unwrap(),
        data
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stalled_upload_chunk_gives_up_and_frees_the_upload() {
    let env = env(Opts::default()).await;
    env.uploads.set_chunk_idle(Duration::from_millis(200));
    let user = &env.member;
    let data = payload(100);
    let created = env
        .uploads
        .create(user, create("stall.mp4", 100))
        .await
        .unwrap();
    // The phone left the network mid-chunk: nothing more arrives, nothing closes.
    let stalled = Body::from_stream(
        futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::copy_from_slice(
            &data[..30],
        ))])
        .chain(futures::stream::pending()),
    );
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        env.uploads
            .write_chunk(user, &created.id, 0, Some(100), stalled),
    )
    .await
    .expect("a stalled chunk must not hold the upload forever");
    let err = result.unwrap_err();
    assert_eq!(err.extra.unwrap()["received"], 30);
    let resumed = tokio::time::timeout(
        Duration::from_secs(5),
        env.uploads.write_chunk(
            user,
            &created.id,
            30,
            Some(70),
            Body::from(data[30..].to_vec()),
        ),
    )
    .await
    .expect("the upload is free again");
    assert_eq!(resumed.unwrap(), 100);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_locks_are_not_kept_for_unknown_uploads() {
    let env = env(Opts::default()).await;
    for i in 0..20 {
        let id = format!("nope-{i}");
        let _ = env
            .uploads
            .write_chunk(&env.member, &id, 0, Some(1), Body::from(vec![1u8]))
            .await;
        let _ = env.uploads.complete(&env.member, &id).await;
        let _ = env.uploads.cancel(&env.member, &id).await;
    }
    assert_eq!(env.uploads.lock_count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_a_sorted_download_never_deletes_what_now_sits_at_its_old_path() {
    let env = env(Opts::default()).await;
    // Sorted long ago; its file went to the library and the name was reused since.
    let id = insert_row(
        &env,
        "http",
        "file.bin",
        "https://example.com/file.bin",
        "sorted",
        Some("Downloads/Completed/file.bin"),
        Some(10),
        None,
    )
    .await;
    let newer = env.completed().join("file.bin");
    std::fs::write(&newer, b"a newer download").unwrap();
    env.manager.remove(&id, true).await.unwrap();
    assert_eq!(std::fs::read(&newer).unwrap(), b"a newer download");
    assert_eq!(
        env.manager.get(&id).await.unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_stuck_in_sorting_can_be_removed() {
    let env = env(Opts::default()).await;
    // Its sort job gave up after three attempts: nothing will ever move it on.
    let stuck = insert_row(
        &env,
        "http",
        "a.mkv",
        "https://example.com/a.mkv",
        "sorting",
        Some("Downloads/Completed/a.mkv"),
        Some(10),
        None,
    )
    .await;
    env.manager.remove(&stuck, false).await.unwrap();

    // While a sort job is still waiting, removal waits too.
    let busy = insert_row(
        &env,
        "http",
        "b.mkv",
        "https://example.com/b.mkv",
        "sorting",
        Some("Downloads/Completed/b.mkv"),
        Some(10),
        None,
    )
    .await;
    std::fs::write(env.completed().join("b.mkv"), b"x").unwrap();
    env.manager
        .library
        .enqueue_sort("Downloads/Completed/b.mkv", Some(&busy))
        .await
        .unwrap();
    assert_eq!(
        env.manager.remove(&busy, false).await.unwrap_err().code,
        ErrorCode::Conflict
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vpn_proxy_is_skipped_for_ports_it_cannot_reach() {
    let files = FileServer::start().await;
    // The VPN is up and a download proxy is configured, but it only reaches ports 80
    // and 443; the test server listens on a random port.
    let env = env(Opts {
        engine: true,
        allow_local: true,
        start: true,
        proxy: true,
        ..Default::default()
    })
    .await;
    let d = env.add(&files.url("/file.bin")).await;
    wait_until!(
        "the direct download",
        env.row(&d.id).await.state == DownloadState::Sorting
    );

    // "Always" never goes around the VPN: it fails with a plain reason instead.
    env.settings
        .update(serde_json::json!({"http_via_vpn": "always"}))
        .await
        .unwrap();
    let before = files.seen("/echo.bin").len();
    let d = env.add(&files.url("/echo.bin")).await;
    wait_until!(
        "the download to fail",
        env.row(&d.id).await.state == DownloadState::Failed
    );
    let error = env.row(&d.id).await.error.unwrap();
    assert!(error.contains("80 and 443"), "{error}");
    assert_eq!(files.seen("/echo.bin").len(), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stalled_torrent_link_gives_up() {
    let files = FileServer::start().await;
    let env = env(Opts {
        engine: true,
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        env.manager.add_url(
            &env.owner.user_id,
            AddDownloadRequest {
                url: files.url("/stall.torrent"),
                filename: None,
                headers: None,
            },
        ),
    )
    .await
    .expect("fetching a .torrent link must not hang");
    assert_eq!(result.unwrap_err().code, ErrorCode::BadRequest);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_errors_keep_the_json_shape() {
    let (_dir, state, token, _) = api_state().await;
    let router = crate::app::router(state);
    let json = "application/json";
    let cases: [(&str, &str, &str, &str); 7] = [
        ("POST", "/api/downloads", json, "{}"),
        ("POST", "/api/downloads", json, "not json"),
        ("GET", "/api/downloads?state=bogus", json, ""),
        ("DELETE", "/api/downloads/x?delete_files=maybe", json, ""),
        (
            "PUT",
            "/api/uploads/x?offset=abc",
            "application/octet-stream",
            "x",
        ),
        ("POST", "/api/downloads/torrent", json, "{}"),
        ("POST", "/api/uploads", json, "{\"filename\": 3}"),
    ];
    for (method, uri, content_type, body) in cases {
        let (status, answer) = call(
            &router,
            method,
            uri,
            &token,
            content_type,
            Body::from(body.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}: {answer}");
        assert_eq!(answer["error"]["code"], "bad_request", "{method} {uri}");
        let message = answer["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.ends_with('.') && !message.contains("serde"),
            "{method} {uri}: {message}"
        );
    }
}

// --- fake VPN download proxy ---------------------------------------------------------

/// What the fake VPN proxy was asked for: the absolute URL and the Cookie header.
#[derive(Clone, Default)]
struct ProxyLog(Arc<Mutex<Vec<ProxiedRequest>>>);

/// The absolute URL asked for and its Cookie header.
type ProxiedRequest = (String, Option<String>);

impl ProxyLog {
    fn cookie_for(&self, url: &str) -> Vec<Option<String>> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(u, _)| u == url)
            .map(|(_, c)| c.clone())
            .collect()
    }
}

/// A plain HTTP proxy standing in for the engine's VPN proxy. It never connects
/// anywhere: it answers absolute-form requests itself, so the hosts in the links
/// (127.0.0.1 and 127.0.0.2 on port 80) are only names.
async fn fake_proxy(log: ProxyLog) -> Url {
    async fn answer(State(log): State<ProxyLog>, uri: Uri, headers: HeaderMap) -> Response {
        let cookie = headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        log.0.lock().unwrap().push((uri.to_string(), cookie));
        let host = uri.host().unwrap_or_default().to_string();
        match (host.as_str(), uri.path()) {
            // Same origin first, then off to another host.
            ("127.0.0.1", "/start.bin") => {
                (StatusCode::FOUND, [(header::LOCATION, "/same.bin")]).into_response()
            }
            ("127.0.0.1", "/same.bin") => (
                StatusCode::FOUND,
                [(header::LOCATION, "http://127.0.0.2/other.bin")],
            )
                .into_response(),
            ("127.0.0.1", "/start.torrent") => (
                StatusCode::FOUND,
                [(header::LOCATION, "http://127.0.0.2/other.torrent")],
            )
                .into_response(),
            ("127.0.0.2", "/other.bin") => Body::from(payload(4096)).into_response(),
            ("127.0.0.2", "/other.torrent") => Body::from(TORRENT_FILE).into_response(),
            _ => StatusCode::NOT_FOUND.into_response(),
        }
    }
    let router = Router::new().fallback(any(answer)).with_state(log);
    let addr = serve(router).await;
    Url::parse(&format!("http://{addr}")).unwrap()
}

/// Through the VPN proxy redirects are followed by hand. The browser's cookies belong
/// to the site they came from: they follow a redirect on that site, never one to
/// another host (reqwest does the same for direct downloads).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forwarded_cookies_never_follow_a_redirect_to_another_host() {
    let log = ProxyLog::default();
    let proxy = fake_proxy(log.clone()).await;
    let env = env(Opts {
        engine: true,
        allow_local: true,
        start: true,
        proxy_url: Some(proxy),
        ..Default::default()
    })
    .await;
    env.manager.allow_destination("127.0.0.2".parse().unwrap());
    let headers = || {
        Some(ForwardedHeaders {
            cookie: Some("session=SECRETCOOKIE".into()),
            referer: None,
            user_agent: None,
        })
    };
    let d = env
        .manager
        .add_url(
            &env.owner.user_id,
            AddDownloadRequest {
                url: "http://127.0.0.1/start.bin".into(),
                filename: None,
                headers: headers(),
            },
        )
        .await
        .unwrap();
    wait_until!(
        "the download through the proxy",
        env.row(&d.id).await.state == DownloadState::Sorting
    );
    let secret = Some("session=SECRETCOOKIE".to_string());
    assert_eq!(
        log.cookie_for("http://127.0.0.1/start.bin"),
        vec![secret.clone()]
    );
    assert_eq!(
        log.cookie_for("http://127.0.0.1/same.bin"),
        vec![secret.clone()]
    );
    assert_eq!(log.cookie_for("http://127.0.0.2/other.bin"), vec![None]);

    // A .torrent link takes the same path.
    env.manager
        .add_url(
            &env.owner.user_id,
            AddDownloadRequest {
                url: "http://127.0.0.1/start.torrent".into(),
                filename: None,
                headers: headers(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        log.cookie_for("http://127.0.0.1/start.torrent"),
        vec![secret]
    );
    assert_eq!(log.cookie_for("http://127.0.0.2/other.torrent"), vec![None]);
}

// --- partial files the torrent engine must not read ----------------------------------

/// The engine may write `Downloads/In Progress` (and with Landlock nothing else), so a
/// family member's upload never passes through it, and the files are private.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uploads_stay_out_of_the_engines_folder() {
    let env = env(Opts::default()).await;
    let data = payload(1000);
    let created = env
        .uploads
        .create(&env.member, create("Kids.mp4", data.len() as i64))
        .await
        .unwrap();
    let part = env.uploads_dir().join(format!("{}.part", created.id));
    assert!(part.is_file(), "{}", part.display());
    assert_eq!(mode(&env.uploads_dir()), 0o700);
    assert_eq!(mode(&part), 0o600);
    assert!(
        std::fs::read_dir(env.in_progress())
            .unwrap()
            .all(|e| e.unwrap().file_name() != ".uploads"),
        "nothing is created in In Progress"
    );
    env.uploads
        .write_chunk(
            &env.member,
            &created.id,
            0,
            Some(data.len() as u64),
            Body::from(data.clone()),
        )
        .await
        .unwrap();
    let download = env
        .uploads
        .complete(&env.member, &created.id)
        .await
        .unwrap();
    assert_eq!(download.state, DownloadState::Sorting);
    assert_eq!(
        std::fs::read(env.completed().join("Kids.mp4")).unwrap(),
        data
    );
}

/// Uploads in progress when the server is updated move to the new folder; anything
/// else in the old one (a link or a file the engine planted) stays where it is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uploads_in_the_old_folder_move_over() {
    let env = env(Opts::default()).await;
    let created = env
        .uploads
        .create(&env.member, create("old.mp4", 4))
        .await
        .unwrap();
    let legacy = env.in_progress().join(".uploads");
    std::fs::create_dir_all(&legacy).unwrap();
    let new_part = env.uploads_dir().join(format!("{}.part", created.id));
    std::fs::rename(&new_part, legacy.join(format!("{}.part", created.id))).unwrap();
    std::fs::write(legacy.join(format!("{}.part", created.id)), b"ab").unwrap();
    let outside = env.root.join("Other/secret.txt");
    std::fs::write(&outside, b"secret").unwrap();
    std::os::unix::fs::symlink(&outside, legacy.join("0193aaaa-link.part")).unwrap();

    env.uploads.migrate_legacy().await;
    assert_eq!(std::fs::read(&new_part).unwrap(), b"ab");
    assert_eq!(mode(&new_part), 0o600);
    assert!(!env.uploads_dir().join("0193aaaa-link.part").exists());
    assert!(
        std::fs::symlink_metadata(legacy.join("0193aaaa-link.part"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let status = env.uploads.status(&env.member, &created.id).await.unwrap();
    assert_eq!(status.state, "receiving");
}

/// A web download's `.part` lives in In Progress (the engine's folder): it is private,
/// and a file someone else put at its name is never written to.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn web_download_parts_are_private() {
    let files = FileServer::start().await;
    let env = env(Opts {
        allow_local: true,
        start: true,
        ..Default::default()
    })
    .await;
    let d = env.add(&files.url("/slow.bin")).await;
    let part = env.in_progress().join("slow.bin.part");
    wait_until!("the part file", part.exists());
    assert_eq!(mode(&part), 0o600);
    env.manager.pause(&d.id).await.unwrap();
    wait_until!(
        "the worker to stop",
        !env.manager.running.contains_key(&d.id)
    );

    if running_as_root() {
        // The engine swaps the partial file for one it owns (and could read).
        let kept = std::fs::metadata(&part).unwrap().len();
        std::fs::remove_file(&part).unwrap();
        std::fs::write(&part, vec![0u8; kept as usize]).unwrap();
        std::os::unix::fs::chown(&part, Some(6881), None).unwrap();
        env.manager.resume(&d.id).await.unwrap();
        wait_until!(
            "the resumed download",
            env.row(&d.id).await.state == DownloadState::Sorting
        );
        // It started over instead of continuing into the planted file.
        assert_eq!(files.seen("/slow.bin").last().unwrap().range, None);
        let done = env.completed().join("slow.bin");
        assert_eq!(std::fs::read(&done).unwrap(), *files.files.big);
        use std::os::unix::fs::MetadataExt;
        assert_ne!(std::fs::metadata(&done).unwrap().uid(), 6881);
    } else {
        eprintln!("skipping the planted-file half: needs root");
    }
}
