//! /api routes: settings (see docs/architecture.md sections 6 and 8).

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use stash_core::internal_api::LimitsRequest;
use stash_core::settings::Settings;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::security::ClientIp;
use crate::state::AppState;
use crate::torrentd_client::{TorrentdClient, TorrentdError};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/settings", get(get_settings).put(put_settings))
}

/// `GET /api/settings` (owner).
async fn get_settings(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Settings>> {
    user.require_owner()?;
    Ok(Json(state.settings.get().await))
}

/// `PUT /api/settings` (owner): merges a partial object into the current settings.
async fn put_settings(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    body: Bytes,
) -> ApiResult<Json<Settings>> {
    user.require_owner()?;
    let patch: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::bad_request("Send the settings to change as a JSON object."))?;
    if !patch.is_object() {
        return Err(ApiError::bad_request(
            "Send the settings to change as a JSON object.",
        ));
    }
    let before = state.settings.get().await;
    let after = state.settings.update(patch).await?;
    if before != after {
        let changed = changed_keys(&before, &after);
        let ip_text = ip.as_string();
        state
            .auth
            .audit(
                Some(&user.user_id),
                "settings_changed",
                serde_json::json!({ "changed": changed }),
                ip_text.as_deref(),
            )
            .await;
        tracing::info!(?changed, "settings changed");
    }
    if limits(&before) != limits(&after) {
        push_limits(state.torrentd.clone(), limits(&after));
    }
    Ok(Json(after))
}

/// Torrent speed limits as the engine wants them.
pub fn limits(settings: &Settings) -> LimitsRequest {
    LimitsRequest {
        down_kbps: settings.torrent.down_limit_kbps,
        up_kbps: settings.torrent.up_limit_kbps,
        seed: settings.torrent.seed_after_complete,
    }
}

/// Send limits to the engine in the background. Best effort: a stopped or disabled
/// engine is ignored (it gets the limits again when settings change next).
pub fn push_limits(torrentd: Arc<TorrentdClient>, limits: LimitsRequest) {
    if !torrentd.enabled() {
        return;
    }
    tokio::spawn(async move {
        match torrentd.set_limits(limits).await {
            Ok(()) => tracing::debug!(?limits, "torrent limits sent to the engine"),
            Err(TorrentdError::Disabled) | Err(TorrentdError::Unreachable(_)) => {
                tracing::debug!("torrent engine not reachable; limits not sent")
            }
            Err(e) => tracing::warn!(error = %e, "torrent engine did not accept the new limits"),
        }
    });
}

/// Dotted names of the settings that differ, e.g. `torrent.seed_ratio`.
fn changed_keys(before: &Settings, after: &Settings) -> Vec<String> {
    let (Ok(before), Ok(after)) = (serde_json::to_value(before), serde_json::to_value(after))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    diff("", &before, &after, &mut out);
    out
}

fn diff(
    prefix: &str,
    before: &serde_json::Value,
    after: &serde_json::Value,
    out: &mut Vec<String>,
) {
    match (before, after) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for (key, value) in b {
                let name = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                match a.get(key) {
                    Some(old) => diff(&name, old, value, out),
                    None => out.push(name),
                }
            }
        }
        (a, b) if a != b => out.push(prefix.to_string()),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use stash_core::models::Role;
    use tower::ServiceExt;

    use super::*;
    use crate::auth::Scope;

    fn user(role: Role) -> AuthUser {
        AuthUser {
            user_id: format!("{role}-1"),
            display_name: "Someone".into(),
            role,
            scopes: vec![Scope::Full],
            session_id: Some("s".into()),
            token_id: None,
        }
    }

    fn request(method: Method, body: &str, who: Role) -> Request<Body> {
        let mut req = Request::builder()
            .method(method)
            .uri("/api/settings")
            .header("origin", "http://localhost:8788")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        req.extensions_mut().insert(user(who));
        req
    }

    async fn call(app: &Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
        let response = app.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn get_and_partial_put() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::app::test_state(dir.path()).await.unwrap();
        let app = crate::app::router(state.clone());

        let (code, body) = call(&app, request(Method::GET, "", Role::Owner)).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["auto_sort"], true);
        assert_eq!(body["http_via_vpn"], "when_available");
        assert_eq!(body["torrent"]["seed_ratio"], 1.0);
        assert_eq!(body["torrent"]["seed_after_complete"], false);

        let (code, body) = call(
            &app,
            request(
                Method::PUT,
                r#"{"torrent": {"seed_ratio": 2.5}}"#,
                Role::Owner,
            ),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["torrent"]["seed_ratio"], 2.5);
        assert_eq!(body["torrent"]["max_active"], 5);
        assert_eq!(state.settings.get().await.torrent.seed_ratio, 2.5);

        let (code, body) = call(
            &app,
            request(Method::PUT, r#"{"http_via_vpn": "sometimes"}"#, Role::Owner),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "bad_request");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .starts_with("Invalid settings")
        );

        let (code, _) = call(&app, request(Method::PUT, "[1,2]", Role::Owner)).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        let (code, _) = call(&app, request(Method::PUT, "not json", Role::Owner)).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);

        // Members can neither read nor change settings.
        let (code, _) = call(&app, request(Method::GET, "", Role::Member)).await;
        assert_eq!(code, StatusCode::FORBIDDEN);
        let (code, _) = call(
            &app,
            request(Method::PUT, r#"{"auto_sort": false}"#, Role::Member),
        )
        .await;
        assert_eq!(code, StatusCode::FORBIDDEN);
        assert!(state.settings.get().await.auto_sort);
    }

    /// With STASH_HWACCEL set, the settings show its value, and the page's usual
    /// save (which sends back what it read) still works; choosing another value is
    /// refused with a sentence that names the variable.
    #[tokio::test]
    async fn stash_hwaccel_decides_while_it_is_set() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = crate::app::test_state(dir.path()).await.unwrap();
        state
            .settings
            .update(serde_json::json!({"transcode": {"hwaccel": "qsv"}}))
            .await
            .unwrap();
        state.settings = stash_core::settings::SettingsStore::load(
            state.db.clone(),
            Some(stash_core::config::HwAccel::Vaapi),
        )
        .await
        .unwrap();
        let app = crate::app::router(state.clone());

        let (code, body) = call(&app, request(Method::GET, "", Role::Owner)).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["transcode"]["hwaccel"], "vaapi");

        let (code, body) = call(
            &app,
            request(
                Method::PUT,
                r#"{"transcode": {"hwaccel": "vaapi", "crf": 28}}"#,
                Role::Owner,
            ),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["transcode"]["hwaccel"], "vaapi");
        assert_eq!(body["transcode"]["crf"], 28);

        let (code, body) = call(
            &app,
            request(
                Method::PUT,
                r#"{"transcode": {"hwaccel": "none"}}"#,
                Role::Owner,
            ),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("STASH_HWACCEL"),
            "{body}"
        );
        assert_eq!(
            state.settings.get().await.transcode.hwaccel,
            stash_core::config::HwAccel::Vaapi
        );
    }

    #[tokio::test]
    async fn limit_changes_reach_the_engine() {
        use axum::routing::put;
        let dir = tempfile::tempdir().unwrap();
        let received: Arc<Mutex<Option<LimitsRequest>>> = Arc::default();
        let sink = received.clone();
        let fake = Router::new().route(
            "/limits",
            put(move |Json(limits): Json<LimitsRequest>| {
                let sink = sink.clone();
                async move {
                    *sink.lock().unwrap() = Some(limits);
                    StatusCode::NO_CONTENT
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });
        let mut config =
            stash_core::config::Config::for_tests(&dir.path().join("s"), &dir.path().join("c"));
        config.torrentd_url = Some(url::Url::parse(&format!("http://{addr}")).unwrap());
        TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        let client = Arc::new(TorrentdClient::new(Arc::new(config)));

        let mut settings = Settings::default();
        settings.torrent.down_limit_kbps = 800;
        settings.torrent.up_limit_kbps = 100;
        push_limits(client, limits(&settings));
        for _ in 0..50 {
            if received.lock().unwrap().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(
            *received.lock().unwrap(),
            Some(LimitsRequest {
                down_kbps: 800,
                up_kbps: 100,
                seed: false,
            })
        );
    }

    #[test]
    fn changed_key_names() {
        let before = Settings::default();
        let mut after = before.clone();
        after.torrent.seed_ratio = 3.0;
        after.auto_sort = false;
        let mut keys = changed_keys(&before, &after);
        keys.sort();
        assert_eq!(keys, ["auto_sort", "torrent.seed_ratio"]);
    }
}
