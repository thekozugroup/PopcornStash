//! Internal API on 127.0.0.1:9091 (section 9).
//!
//! Every route except `/livez` needs `Authorization: Bearer <internal token>`.
//! The handlers talk to a [`TorrentBackend`], so tests can use a fake.

use std::future::Future;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use base64::Engine as _;
use serde::Deserialize;
use stash_core::internal_api::{
    AddTorrentRequest, AddTorrentResponse, EngineHealth, EngineTorrent, LimitsRequest,
};

use crate::sys::constant_time_eq;

/// Largest accepted .torrent file (section 8: 10 MB).
pub const MAX_TORRENT_BYTES: usize = 10 * 1024 * 1024;
/// Request body limit: a base64 encoded 10 MB torrent plus JSON overhead.
const BODY_LIMIT: usize = 16 * 1024 * 1024;
/// The message section 9 prescribes for adds refused while the VPN is down.
pub const MSG_VPN_WAIT: &str = "Waiting for the VPN to connect.";

/// A torrent to add, already decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewTorrent {
    Magnet(String),
    File(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    /// The VPN is not connected (409 `vpn_unavailable`).
    #[error("{0}")]
    Blocked(String),
    #[error("No torrent with this id.")]
    NotFound,
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Internal(String),
}

/// What the API needs from the torrent engine.
pub trait TorrentBackend: Send + Sync + 'static {
    fn health(&self) -> impl Future<Output = EngineHealth> + Send;
    fn list(&self) -> impl Future<Output = Vec<EngineTorrent>> + Send;
    fn add(
        &self,
        torrent: NewTorrent,
    ) -> impl Future<Output = Result<AddTorrentResponse, BackendError>> + Send;
    fn pause(&self, id: String) -> impl Future<Output = Result<(), BackendError>> + Send;
    fn resume(&self, id: String) -> impl Future<Output = Result<(), BackendError>> + Send;
    fn remove(
        &self,
        id: String,
        delete_files: bool,
    ) -> impl Future<Output = Result<(), BackendError>> + Send;
    fn set_limits(
        &self,
        limits: LimitsRequest,
    ) -> impl Future<Output = Result<(), BackendError>> + Send;
    /// Ask the engine to flush its state and exit with `EXIT_RELOAD` (after answering).
    fn request_reload(&self);
}

struct ApiState<B> {
    backend: Arc<B>,
    token: Arc<str>,
}

impl<B> Clone for ApiState<B> {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            token: self.token.clone(),
        }
    }
}

fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    let body = serde_json::json!({"error": {"code": code, "message": message.into()}});
    (status, Json(body)).into_response()
}

impl IntoResponse for BackendError {
    fn into_response(self) -> Response {
        match self {
            BackendError::Blocked(message) => {
                error(StatusCode::CONFLICT, "vpn_unavailable", message)
            }
            BackendError::NotFound => error(
                StatusCode::NOT_FOUND,
                "not_found",
                "No torrent with this id.",
            ),
            BackendError::BadRequest(message) => {
                error(StatusCode::BAD_REQUEST, "bad_request", message)
            }
            BackendError::Internal(message) => {
                tracing::warn!(error = %message, "torrent engine error");
                error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "The torrent engine could not do that.",
                )
            }
        }
    }
}

/// The internal API router.
pub fn router<B: TorrentBackend>(backend: Arc<B>, token: &str) -> Router {
    let state = ApiState {
        backend,
        token: Arc::from(token),
    };
    let protected = Router::new()
        .route("/health", get(health::<B>))
        .route("/torrents", get(list::<B>).post(add::<B>))
        .route("/torrents/{id}/pause", post(pause::<B>))
        .route("/torrents/{id}/resume", post(resume::<B>))
        .route("/torrents/{id}", delete(remove::<B>))
        .route("/limits", put(limits::<B>))
        .route("/vpn/reload", post(reload::<B>))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_token::<B>,
        ))
        .with_state(state);
    Router::new()
        .route("/livez", get(|| async { "ok" }))
        .merge(protected)
        .fallback(|| async { error(StatusCode::NOT_FOUND, "not_found", "Unknown route.") })
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
}

/// True when `headers` carry `Authorization: Bearer <token>`.
pub fn bearer_ok(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|given| constant_time_eq(given.trim().as_bytes(), token.as_bytes()))
}

async fn require_token<B: TorrentBackend>(
    State(state): State<ApiState<B>>,
    request: Request,
    next: Next,
) -> Response {
    if state.token.is_empty() || !bearer_ok(request.headers(), &state.token) {
        return error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "The internal token is missing or wrong.",
        );
    }
    next.run(request).await
}

async fn health<B: TorrentBackend>(State(state): State<ApiState<B>>) -> Json<EngineHealth> {
    Json(state.backend.health().await)
}

async fn list<B: TorrentBackend>(State(state): State<ApiState<B>>) -> Json<Vec<EngineTorrent>> {
    Json(state.backend.list().await)
}

/// Validate and decode a `POST /torrents` body.
pub fn decode_add(request: AddTorrentRequest) -> Result<NewTorrent, BackendError> {
    let magnet = request.magnet.filter(|m| !m.trim().is_empty());
    let file = request.torrent_base64.filter(|t| !t.trim().is_empty());
    match (magnet, file) {
        (Some(magnet), None) => {
            let magnet = magnet.trim().to_string();
            if !magnet.to_ascii_lowercase().starts_with("magnet:?") || magnet.len() > 64 * 1024 {
                return Err(BackendError::BadRequest(
                    "That is not a magnet link.".into(),
                ));
            }
            Ok(NewTorrent::Magnet(magnet))
        }
        (None, Some(encoded)) => {
            let compact: String = encoded
                .chars()
                .filter(|c| !c.is_ascii_whitespace())
                .collect();
            if compact.len() > MAX_TORRENT_BYTES / 3 * 4 + 8 {
                return Err(BackendError::BadRequest(
                    "The .torrent file is larger than 10 MB.".into(),
                ));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(compact)
                .map_err(|_| {
                    BackendError::BadRequest("The .torrent file is not valid base64.".into())
                })?;
            if bytes.is_empty() || bytes.len() > MAX_TORRENT_BYTES {
                return Err(BackendError::BadRequest(
                    "The .torrent file is empty or larger than 10 MB.".into(),
                ));
            }
            Ok(NewTorrent::File(bytes))
        }
        _ => Err(BackendError::BadRequest(
            "Send either a magnet link or a .torrent file.".into(),
        )),
    }
}

async fn add<B: TorrentBackend>(
    State(state): State<ApiState<B>>,
    body: Result<Json<AddTorrentRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(request)) = body else {
        return error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "The request body is not valid JSON.",
        );
    };
    let torrent = match decode_add(request) {
        Ok(torrent) => torrent,
        Err(e) => return e.into_response(),
    };
    match state.backend.add(torrent).await {
        Ok(added) => (StatusCode::OK, Json(added)).into_response(),
        Err(e) => e.into_response(),
    }
}

fn valid_id(id: &str) -> Result<(), BackendError> {
    if id.len() == 40 && id.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(BackendError::NotFound)
    }
}

async fn pause<B: TorrentBackend>(
    State(state): State<ApiState<B>>,
    Path(id): Path<String>,
) -> Response {
    if let Err(e) = valid_id(&id) {
        return e.into_response();
    }
    match state.backend.pause(id.to_ascii_lowercase()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

async fn resume<B: TorrentBackend>(
    State(state): State<ApiState<B>>,
    Path(id): Path<String>,
) -> Response {
    if let Err(e) = valid_id(&id) {
        return e.into_response();
    }
    match state.backend.resume(id.to_ascii_lowercase()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct DeleteQuery {
    delete_files: Option<bool>,
}

async fn remove<B: TorrentBackend>(
    State(state): State<ApiState<B>>,
    Path(id): Path<String>,
    query: Result<Query<DeleteQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(query)) = query else {
        return error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "delete_files must be true or false.",
        );
    };
    if let Err(e) = valid_id(&id) {
        return e.into_response();
    }
    match state
        .backend
        .remove(id.to_ascii_lowercase(), query.delete_files.unwrap_or(false))
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

async fn limits<B: TorrentBackend>(
    State(state): State<ApiState<B>>,
    body: Result<Json<LimitsRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(limits)) = body else {
        return error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "Send {\"down_kbps\": n, \"up_kbps\": n}.",
        );
    };
    match state.backend.set_limits(limits).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

async fn reload<B: TorrentBackend>(State(state): State<ApiState<B>>) -> Response {
    tracing::info!("VPN reload requested; the engine will restart");
    state.backend.request_reload();
    StatusCode::ACCEPTED.into_response()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use stash_core::internal_api::{EngineRunState, EngineState, EngineTorrentState, TunnelStatus};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tower::ServiceExt;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const HASH: &str = "c9e15763f722f23e98a29decdfae341b98d53056";

    #[derive(Default)]
    pub(crate) struct FakeBackend {
        pub blocked: AtomicBool,
        pub reloads: AtomicBool,
        pub added: Mutex<Vec<NewTorrent>>,
        pub calls: Mutex<Vec<String>>,
        pub limits: Mutex<Option<LimitsRequest>>,
    }

    impl TorrentBackend for FakeBackend {
        async fn health(&self) -> EngineHealth {
            let blocked = self.blocked.load(Ordering::SeqCst);
            EngineHealth {
                vpn: TunnelStatus::not_configured(1),
                engine: EngineState {
                    state: if blocked {
                        EngineRunState::Blocked
                    } else {
                        EngineRunState::Running
                    },
                    reason: blocked.then(|| MSG_VPN_WAIT.to_string()),
                    allow_no_vpn: false,
                },
                public_ip: None,
                country: None,
                protected: None,
                checked_at: None,
            }
        }
        async fn list(&self) -> Vec<EngineTorrent> {
            vec![EngineTorrent {
                id: HASH.into(),
                info_hash: HASH.into(),
                name: Some("ubuntu.iso".into()),
                state: EngineTorrentState::Downloading,
                progress: 0.5,
                total_bytes: Some(100),
                done_bytes: 50,
                uploaded_bytes: 0,
                down_bps: 10,
                up_bps: 0,
                peers: Some(3),
                eta_s: Some(5),
                error: None,
                output_dir: Some("/stash/Downloads/In Progress/ubuntu".into()),
            }]
        }
        async fn add(&self, torrent: NewTorrent) -> Result<AddTorrentResponse, BackendError> {
            if self.blocked.load(Ordering::SeqCst) {
                return Err(BackendError::Blocked(MSG_VPN_WAIT.into()));
            }
            self.added.lock().unwrap().push(torrent);
            Ok(AddTorrentResponse {
                id: HASH.into(),
                info_hash: HASH.into(),
                name: Some("ubuntu.iso".into()),
            })
        }
        async fn pause(&self, id: String) -> Result<(), BackendError> {
            self.calls.lock().unwrap().push(format!("pause {id}"));
            if id == HASH {
                Ok(())
            } else {
                Err(BackendError::NotFound)
            }
        }
        async fn resume(&self, id: String) -> Result<(), BackendError> {
            self.calls.lock().unwrap().push(format!("resume {id}"));
            Ok(())
        }
        async fn remove(&self, id: String, delete_files: bool) -> Result<(), BackendError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("remove {id} {delete_files}"));
            Ok(())
        }
        async fn set_limits(&self, limits: LimitsRequest) -> Result<(), BackendError> {
            *self.limits.lock().unwrap() = Some(limits);
            Ok(())
        }
        fn request_reload(&self) {
            self.reloads.store(true, Ordering::SeqCst);
        }
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        auth: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if let Some(token) = auth {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        let request = match body {
            Some(json) => builder
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&bytes).into()));
        (status, json)
    }

    #[tokio::test]
    async fn livez_needs_no_token_everything_else_does() {
        let app = router(Arc::new(FakeBackend::default()), TOKEN);
        let (status, body) = call(&app, "GET", "/livez", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::Value::String("ok".into()));
        for (method, uri) in [
            ("GET", "/health"),
            ("GET", "/torrents"),
            ("POST", "/torrents"),
            ("PUT", "/limits"),
            ("POST", "/vpn/reload"),
            ("DELETE", &format!("/torrents/{HASH}")),
        ] {
            let (status, body) = call(&app, method, uri, None, None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
            assert_eq!(body["error"]["code"], "unauthorized");
            let (status, _) = call(&app, method, uri, Some("wrong"), None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        }
        let (status, body) = call(&app, "GET", "/health", Some(TOKEN), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["engine"]["state"], "running");
        let (status, body) = call(&app, "GET", "/torrents", Some(TOKEN), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body[0]["info_hash"], HASH);
        assert_eq!(body[0]["state"], "downloading");
    }

    #[tokio::test]
    async fn empty_token_refuses_everything() {
        let app = router(Arc::new(FakeBackend::default()), "");
        let (status, _) = call(&app, "GET", "/health", Some(""), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn add_validates_and_reports_vpn_block() {
        let backend = Arc::new(FakeBackend::default());
        let app = router(backend.clone(), TOKEN);
        let magnet = format!("magnet:?xt=urn:btih:{HASH}&dn=ubuntu");
        let (status, body) = call(
            &app,
            "POST",
            "/torrents",
            Some(TOKEN),
            Some(serde_json::json!({"magnet": magnet})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"], HASH);
        let file = base64::engine::general_purpose::STANDARD.encode(b"d4:infod4:name1:xee");
        let (status, _) = call(
            &app,
            "POST",
            "/torrents",
            Some(TOKEN),
            Some(serde_json::json!({"torrent_base64": file})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(backend.added.lock().unwrap().len(), 2);

        for bad in [
            serde_json::json!({}),
            serde_json::json!({"magnet": "http://x"}),
            serde_json::json!({"magnet": magnet, "torrent_base64": file}),
            serde_json::json!({"torrent_base64": "***"}),
        ] {
            let (status, body) =
                call(&app, "POST", "/torrents", Some(TOKEN), Some(bad.clone())).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
            assert_eq!(body["error"]["code"], "bad_request");
        }

        backend.blocked.store(true, Ordering::SeqCst);
        let (status, body) = call(
            &app,
            "POST",
            "/torrents",
            Some(TOKEN),
            Some(serde_json::json!({"magnet": magnet})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(
            body,
            serde_json::json!({"error": {"code": "vpn_unavailable", "message": "Waiting for the VPN to connect."}})
        );
    }

    #[tokio::test]
    async fn torrent_actions_limits_and_reload() {
        let backend = Arc::new(FakeBackend::default());
        let app = router(backend.clone(), TOKEN);
        let (status, _) = call(
            &app,
            "POST",
            &format!("/torrents/{HASH}/pause"),
            Some(TOKEN),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(
            &app,
            "POST",
            &format!("/torrents/{}/pause", HASH.to_uppercase()),
            Some(TOKEN),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, body) = call(&app, "POST", "/torrents/nope/pause", Some(TOKEN), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
        let (status, _) = call(
            &app,
            "POST",
            &format!("/torrents/{HASH}/resume"),
            Some(TOKEN),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(
            &app,
            "DELETE",
            &format!("/torrents/{HASH}?delete_files=true"),
            Some(TOKEN),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(
            &app,
            "DELETE",
            &format!("/torrents/{HASH}"),
            Some(TOKEN),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(
            &app,
            "DELETE",
            &format!("/torrents/{HASH}?delete_files=maybe"),
            Some(TOKEN),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            *backend.calls.lock().unwrap(),
            vec![
                format!("pause {HASH}"),
                format!("pause {HASH}"),
                format!("resume {HASH}"),
                format!("remove {HASH} true"),
                format!("remove {HASH} false")
            ]
        );

        let (status, _) = call(
            &app,
            "PUT",
            "/limits",
            Some(TOKEN),
            Some(serde_json::json!({"down_kbps": 500, "up_kbps": 0})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            *backend.limits.lock().unwrap(),
            Some(LimitsRequest {
                down_kbps: 500,
                up_kbps: 0
            })
        );
        let (status, _) = call(
            &app,
            "PUT",
            "/limits",
            Some(TOKEN),
            Some(serde_json::json!({"down": 1})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = call(&app, "POST", "/vpn/reload", Some(TOKEN), None).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(backend.reloads.load(Ordering::SeqCst));
    }

    #[test]
    fn decode_add_limits() {
        let huge = "A".repeat(MAX_TORRENT_BYTES / 3 * 4 + 100);
        assert!(
            decode_add(AddTorrentRequest {
                magnet: None,
                torrent_base64: Some(huge)
            })
            .is_err()
        );
        assert_eq!(
            decode_add(AddTorrentRequest {
                magnet: Some(" magnet:?xt=urn:btih:abc ".into()),
                torrent_base64: None
            })
            .unwrap(),
            NewTorrent::Magnet("magnet:?xt=urn:btih:abc".into())
        );
    }
}
