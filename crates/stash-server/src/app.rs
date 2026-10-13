//! Startup wiring: state, background tasks, router and the HTTP(S) listener.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::{Router, middleware};
use stash_core::config::Config;
use stash_core::events::EventBus;
use stash_core::jobs::{Job, JobKind, JobQueue, handler};
use stash_core::library::Library;
use stash_core::media::MediaService;
use stash_core::media::transcode::TranscodeManager;
use stash_core::paths::StashLayout;
use stash_core::settings::SettingsStore;
use tokio_util::sync::CancellationToken;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::trace::TraceLayer;

use crate::auth::AuthService;
use crate::downloads::DownloadManager;
use crate::security::{self, RateLimiter};
use crate::state::AppState;
use crate::torrentd_client::TorrentdClient;
use crate::uploads::UploadService;

/// Open the database, create the stash layout and build every service.
pub async fn build_state(config: Config) -> anyhow::Result<AppState> {
    let config = Arc::new(config);
    tokio::fs::create_dir_all(&config.config_dir).await?;
    for dir in [
        config.secrets_dir(),
        config.thumbs_dir(),
        config.renditions_dir(),
        config.transcode_dir(),
        config.vpn_dir(),
        config.pending_torrents_dir(),
    ] {
        tokio::fs::create_dir_all(dir).await?;
    }
    // Thumbnails, photo renditions and live transcodes are pictures of the family's
    // media, and pending torrents can carry private tracker keys: only the web
    // server's user may open these folders (the torrent engine shares its group).
    for dir in [
        config.secrets_dir(),
        config.cache_dir(),
        config.vpn_dir(),
        config.pending_torrents_dir(),
    ] {
        if let Err(e) = make_private_dir(&dir).await {
            tracing::warn!(error = %format!("{e:#}"), "could not make a private folder private");
        }
    }
    let db = stash_core::db::open(&config.db_path()).await?;
    let layout = Arc::new(StashLayout::new(&config.stash_root));
    layout.ensure().await?;
    let events = EventBus::new();
    let jobs = JobQueue::new(db.clone(), events.clone());
    let settings = SettingsStore::load(db.clone(), config.hwaccel).await?;
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
        media.clone(),
    ));
    let transcoder = Arc::new(TranscodeManager::new(config.clone(), settings.clone()));
    let auth = Arc::new(AuthService::new(config.clone(), db.clone())?);
    let torrentd = Arc::new(TorrentdClient::new(config.clone()));
    if torrentd.enabled() {
        TorrentdClient::ensure_internal_token(&config).await?;
    }
    let downloads = Arc::new(DownloadManager::new(
        config.clone(),
        db.clone(),
        layout.clone(),
        events.clone(),
        jobs.clone(),
        settings.clone(),
        library.clone(),
        torrentd.clone(),
    ));
    let uploads = Arc::new(UploadService::new(
        db.clone(),
        layout.clone(),
        downloads.clone(),
    ));
    let http = reqwest::Client::builder()
        .user_agent(concat!("PopcornStash/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()?;
    Ok(AppState {
        config,
        db,
        layout,
        events,
        jobs,
        settings,
        media,
        library,
        transcoder,
        auth,
        downloads,
        uploads,
        torrentd,
        rate_limiter: Arc::new(RateLimiter::new()),
        http,
    })
}

/// Narrow a folder of `/config` to its owner (0700). A link in its place is left alone.
async fn make_private_dir(dir: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = tokio::fs::symlink_metadata(dir).await?;
    if !meta.is_dir() {
        anyhow::bail!("{} is not a plain folder", dir.display());
    }
    if meta.permissions().mode() & 0o077 != 0 {
        tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

/// A fully built state rooted in `dir` (`dir/stash`, `dir/config`) for integration tests.
/// Background tasks are not started.
#[doc(hidden)]
pub async fn test_state(dir: &std::path::Path) -> anyhow::Result<AppState> {
    let stash = dir.join("stash");
    let config_dir = dir.join("config");
    tokio::fs::create_dir_all(&stash).await?;
    build_state(Config::for_tests(&stash, &config_dir)).await
}

/// Register job handlers and spawn every background task.
pub async fn start_background(state: &AppState, cancel: CancellationToken) -> anyhow::Result<()> {
    state.jobs.recover().await?;
    let lib = state.library.clone();
    let scan = handler(move |_job: Job| {
        let lib = lib.clone();
        async move {
            let report = lib.rescan().await?;
            tracing::info!(?report, "library scan finished");
            Ok(())
        }
    });
    let media = state.media.clone();
    let probe = handler(move |job: Job| {
        let media = media.clone();
        async move {
            let id = job
                .item_id
                .ok_or_else(|| anyhow::anyhow!("probe job without item"))?;
            media.process_item(&id).await?;
            Ok(())
        }
    });
    let media = state.media.clone();
    let thumb = handler(move |job: Job| {
        let media = media.clone();
        async move {
            let id = job
                .item_id
                .ok_or_else(|| anyhow::anyhow!("thumb job without item"))?;
            media.thumbnail_item(&id).await?;
            Ok(())
        }
    });
    let lib = state.library.clone();
    let sort = handler(move |job: Job| {
        let lib = lib.clone();
        async move {
            let rel = job
                .payload
                .get("rel_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("sort job without rel_path"))?
                .to_string();
            let download_id = job
                .payload
                .get("download_id")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            lib.sort_path(&rel, download_id.as_deref()).await?;
            Ok(())
        }
    });
    let handlers = HashMap::from([
        (JobKind::Scan, (scan, JobKind::Scan.default_concurrency())),
        (
            JobKind::Probe,
            (probe, JobKind::Probe.default_concurrency()),
        ),
        (
            JobKind::Thumb,
            (thumb, JobKind::Thumb.default_concurrency()),
        ),
        (JobKind::Sort, (sort, JobKind::Sort.default_concurrency())),
    ]);
    state.jobs.start(handlers, cancel.clone());
    state
        .jobs
        .enqueue(JobKind::Scan, None, serde_json::json!({}))
        .await?;
    state.library.spawn_watcher(cancel.clone());
    state.transcoder.spawn_reaper(cancel.clone());
    state.downloads.start(cancel.clone());
    if state.torrentd.enabled() {
        state
            .torrentd
            .spawn_status_watch(state.events.clone(), cancel.clone());
    }
    state.uploads.spawn_cleanup(cancel.clone());
    if let Some(code) = state
        .auth
        .ensure_setup_code()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
    {
        log_setup_code(&state.config, &code);
    }
    Ok(())
}

fn log_setup_code(config: &Config, code: &str) {
    let url = config
        .public_url
        .as_ref()
        .map(|u| u.as_str().trim_end_matches('/').to_string());
    tracing::warn!("────────────────────────────────────────────────────────────");
    tracing::warn!("PopcornStash has no owner yet.");
    match url {
        Some(url) => tracing::warn!("Open {url}/setup and enter this setup code:"),
        None => tracing::warn!(
            "Set STASH_PUBLIC_URL to your HTTPS address, then open /setup with this code:"
        ),
    }
    tracing::warn!("    {code}");
    tracing::warn!("The code is valid for 24 hours and changes on restart until used.");
    tracing::warn!("────────────────────────────────────────────────────────────");
}

/// The full router: API (with security layers) plus the embedded UI.
pub fn router(state: AppState) -> Router {
    let api = crate::api::routes()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::csrf_guard,
        ))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024));
    Router::new()
        .merge(api)
        .merge(crate::web::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::security_headers,
        ))
        .layer(CatchPanicLayer::new())
        .layer(
            // Paths only, with invite and recovery codes hidden; never query strings.
            TraceLayer::new_for_http().make_span_with(|request: &axum::extract::Request| {
                tracing::debug_span!(
                    "request",
                    method = %request.method(),
                    path = %security::loggable_path(request.uri().path()),
                )
            }),
        )
        .with_state(state)
}

/// Run the server until SIGINT/SIGTERM.
pub async fn serve(config: Config) -> anyhow::Result<()> {
    let bind = config.bind;
    let tls = match (&config.tls_cert, &config.tls_key) {
        (Some(cert), Some(key)) => Some((cert.clone(), key.clone())),
        (None, None) => None,
        _ => anyhow::bail!("Set both STASH_TLS_CERT and STASH_TLS_KEY, or neither"),
    };
    if let Err(e) = config
        .public_url
        .as_ref()
        .map(stash_core::config::validate_public_url)
        .transpose()
    {
        tracing::warn!("{e}. Passkey sign-in stays unavailable until STASH_PUBLIC_URL is fixed.");
    } else if config.public_url.is_none() {
        tracing::warn!(
            "STASH_PUBLIC_URL is not set. Passkey sign-in stays unavailable until it is."
        );
    }
    let state = build_state(config).await?;
    let cancel = CancellationToken::new();
    start_background(&state, cancel.clone()).await?;
    let app = router(state).into_make_service_with_connect_info::<std::net::SocketAddr>();

    let shutdown = {
        let cancel = cancel.clone();
        async move {
            shutdown_signal().await;
            tracing::info!("shutting down");
            cancel.cancel();
        }
    };

    match tls {
        Some((cert, key)) => {
            let rustls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
            let handle = axum_server::Handle::new();
            let h = handle.clone();
            tokio::spawn(async move {
                shutdown.await;
                h.graceful_shutdown(Some(Duration::from_secs(10)));
            });
            tracing::info!(%bind, "PopcornStash listening (HTTPS)");
            axum_server::bind_rustls(bind, rustls)
                .handle(handle)
                .serve(app)
                .await?;
        }
        None => {
            let listener = tokio::net::TcpListener::bind(bind).await?;
            tracing::info!(%bind, "PopcornStash listening");
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown)
                .await?;
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// Without Landlock the torrent engine can read whatever its group (PGID) or
    /// everyone may read. The family's pictures in the caches and the database are
    /// not among them.
    #[tokio::test]
    async fn private_config_folders() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("config/cache");
        std::fs::create_dir_all(cache.join("thumbs")).unwrap();
        std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o755)).unwrap();
        let state = super::test_state(dir.path()).await.unwrap();
        let config = &state.config;
        for private in [
            config.cache_dir(),
            config.secrets_dir(),
            config.vpn_dir(),
            config.pending_torrents_dir(),
        ] {
            assert_eq!(mode(&private), 0o700, "{}", private.display());
        }
        assert_eq!(mode(&config.db_path()), 0o600);
    }
}
