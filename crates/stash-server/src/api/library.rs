//! /api routes: health, library summary, shows, photo timeline and rescan
//! (see docs/architecture.md section 8).

use axum::extract::{Path, State};
use axum::http::{StatusCode, Uri};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use stash_core::CoreError;
use stash_core::jobs::JobKind;
use stash_core::library::repo::{self, LibrarySummary, PhotoTimeline, ShowDetail, ShowSummary};

use super::items::parse_query;
use crate::auth::{AuthUser, Scope};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/library/summary", get(summary))
        .route("/api/library/rescan", post(rescan))
        .route("/api/shows", get(shows))
        .route("/api/shows/{name}", get(show))
        .route("/api/photos/timeline", get(timeline))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Health {
    pub status: String,
    pub version: String,
}

/// Public liveness check used by the Docker healthcheck.
async fn health() -> Json<Health> {
    Json(Health {
        status: "ok".into(),
        version: stash_core::VERSION.into(),
    })
}

async fn summary(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<LibrarySummary>> {
    user.require(Scope::LibraryRead)?;
    Ok(Json(repo::summary(&state.db, &user.user_id).await?))
}

async fn shows(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<ShowSummary>>> {
    user.require(Scope::LibraryRead)?;
    Ok(Json(repo::shows(&state.db).await?))
}

async fn show(
    State(state): State<AppState>,
    user: AuthUser,
    Path(name): Path<String>,
) -> ApiResult<Json<ShowDetail>> {
    user.require(Scope::LibraryRead)?;
    // `Path` has already percent-decoded the segment, so "Doctor%20Who" arrives as "Doctor Who".
    let not_found = || ApiError::not_found("That show is not in the library.");
    if name.trim().is_empty() || name.chars().count() > 500 {
        return Err(not_found());
    }
    // The exact name first: links are built from `/api/shows`, and a stored name can
    // carry spaces (a "Bluey " folder). Then the trimmed form, for typed addresses.
    let mut candidates = vec![name.as_str()];
    if name.trim() != name {
        candidates.push(name.trim());
    }
    for candidate in candidates {
        match repo::show(&state.db, candidate, &user.user_id).await {
            Ok(detail) if detail.seasons.iter().any(|s| !s.episodes.is_empty()) => {
                return Ok(Json(detail));
            }
            Ok(_) | Err(CoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err(not_found())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TimelineQuery {
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn timeline(
    State(state): State<AppState>,
    user: AuthUser,
    uri: Uri,
) -> ApiResult<Json<PhotoTimeline>> {
    user.require(Scope::LibraryRead)?;
    let query: TimelineQuery = parse_query(&uri)?;
    // Section 11: 200 photos per page unless the client asks, never more than 500.
    let limit = query
        .limit
        .unwrap_or(repo::DEFAULT_TIMELINE_LIMIT)
        .clamp(1, repo::MAX_TIMELINE_LIMIT);
    Ok(Json(
        repo::photo_timeline(&state.db, query.cursor.as_deref(), limit, &user.user_id).await?,
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobAccepted {
    pub job_id: i64,
}

async fn rescan(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<(StatusCode, Json<JobAccepted>)> {
    user.require(Scope::Full)?;
    let job_id = state
        .jobs
        .enqueue(JobKind::Scan, None, serde_json::json!({}))
        .await?;
    tracing::info!(job_id, user = %user.user_id, "library rescan requested");
    Ok((StatusCode::ACCEPTED, Json(JobAccepted { job_id })))
}

/// Helpers shared by the integration tests of this module group.
#[cfg(test)]
pub(crate) mod testing {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{HeaderMap, Request, StatusCode};
    use bytes::Bytes;
    use http_body_util::BodyExt;
    use sha2::{Digest, Sha256};
    use stash_core::models::{Category, ItemStatus, MediaKind};
    use tower::ServiceExt;

    use crate::state::AppState;

    pub struct TestApp {
        pub _dir: tempfile::TempDir,
        pub state: AppState,
        pub router: Router,
    }

    impl TestApp {
        pub async fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let state = crate::app::test_state(dir.path())
                .await
                .expect("test state");
            let router = crate::app::router(state.clone());
            Self {
                _dir: dir,
                state,
                router,
            }
        }

        /// Send a request and collect the whole response.
        pub async fn send(&self, req: Request<Body>) -> (StatusCode, HeaderMap, Bytes) {
            let response = self.router.clone().oneshot(req).await.expect("infallible");
            let status = response.status();
            let headers = response.headers().clone();
            let body = response
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes();
            (status, headers, body)
        }

        pub async fn get(&self, uri: &str, token: Option<&str>) -> (StatusCode, HeaderMap, Bytes) {
            self.send(request("GET", uri, token, None)).await
        }

        pub async fn json(&self, uri: &str, token: &str) -> (StatusCode, serde_json::Value) {
            let (status, _, body) = self.get(uri, Some(token)).await;
            (
                status,
                serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
            )
        }

        /// A user with the given role, signed in through a bearer session.
        pub async fn user(&self, role: &str) -> (String, String) {
            let user_id = stash_core::new_id();
            let now = stash_core::now();
            sqlx::query("INSERT INTO users (id, display_name, role, webauthn_id, created_at) VALUES (?, ?, ?, ?, ?)")
                .bind(&user_id)
                .bind(format!("Test {role}"))
                .bind(role)
                .bind(stash_core::new_id())
                .bind(now)
                .execute(&self.state.db)
                .await
                .expect("insert user");
            let token = new_token();
            sqlx::query(
                "INSERT INTO sessions (id, user_id, kind, created_at, last_seen_at, expires_at) VALUES (?, ?, 'bearer', ?, ?, ?)",
            )
            .bind(sha256_hex(&token))
            .bind(&user_id)
            .bind(now)
            .bind(now)
            .bind(now + 86_400)
            .execute(&self.state.db)
            .await
            .expect("insert session");
            (user_id, token)
        }

        /// A device token with limited scopes for an existing user.
        pub async fn device_token(&self, user_id: &str, scopes: &[&str]) -> String {
            let token = new_token();
            sqlx::query(
                "INSERT INTO api_tokens (id, user_id, name, token_hash, prefix, scopes, client_kind, created_at)
                 VALUES (?, ?, 'Test device', ?, ?, ?, 'chrome-extension', ?)",
            )
            .bind(stash_core::new_id())
            .bind(user_id)
            .bind(sha256_hex(&token))
            .bind(&token[..8])
            .bind(serde_json::to_string(scopes).expect("scopes"))
            .bind(stash_core::now())
            .execute(&self.state.db)
            .await
            .expect("insert token");
            token
        }

        /// Insert an item row (and optionally write its file) without the scanner.
        pub async fn item(
            &self,
            rel_path: &str,
            kind: MediaKind,
            category: Category,
            status: ItemStatus,
            contents: Option<&[u8]>,
        ) -> String {
            if let Some(contents) = contents {
                let abs = self.state.config.stash_root.join(rel_path);
                std::fs::create_dir_all(abs.parent().expect("parent")).expect("mkdir");
                std::fs::write(&abs, contents).expect("write");
            }
            let id = stash_core::new_id();
            let now = stash_core::now();
            let title = rel_path.rsplit('/').next().unwrap_or(rel_path).to_string();
            sqlx::query(
                "INSERT INTO items (id, rel_path, kind, category, status, title, sort_title, size, mtime, thumb_state, added_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
            )
            .bind(&id)
            .bind(rel_path)
            .bind(kind.as_str())
            .bind(category.as_str())
            .bind(status.as_str())
            .bind(&title)
            .bind(title.to_lowercase())
            .bind(contents.map(|c| c.len() as i64).unwrap_or(0))
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(&self.state.db)
            .await
            .expect("insert item");
            id
        }
    }

    pub fn request(
        method: &str,
        uri: &str,
        token: Option<&str>,
        json: Option<serde_json::Value>,
    ) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        match json {
            Some(json) => builder
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .expect("request"),
            None => builder.body(Body::empty()).expect("request"),
        }
    }

    pub fn new_token() -> String {
        use base64::Engine;
        let bytes: [u8; 32] = rand::random();
        format!(
            "pst_{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
        )
    }

    pub fn sha256_hex(value: &str) -> String {
        hex::encode(Sha256::digest(value.as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use stash_core::models::{Category, ItemStatus, MediaKind};

    use super::testing::{TestApp, request};

    #[tokio::test]
    async fn health_is_public() {
        let app = TestApp::new().await;
        let (status, _, body) = app.get("/api/health", None).await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"status": "ok", "version": stash_core::VERSION})
        );
    }

    #[tokio::test]
    async fn library_needs_sign_in() {
        let app = TestApp::new().await;
        let (status, _, body) = app.get("/api/library/summary", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "unauthorized");
    }

    #[tokio::test]
    async fn summary_shows_and_timeline() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        app.item(
            "Shows/Bluey/Season 01/Bluey - S01E01.mkv",
            MediaKind::Video,
            Category::Episode,
            ItemStatus::Ready,
            None,
        )
        .await;
        sqlx::query("UPDATE items SET show_name = 'Bluey', season = 1, episode = 1")
            .execute(&app.state.db)
            .await
            .unwrap();
        let (status, json) = app.json("/api/library/summary", &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert!(json["counts"].is_object());
        let (status, json) = app.json("/api/shows", &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json[0]["show_name"], "Bluey");
        let (status, json) = app.json("/api/shows/Bluey", &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["seasons"][0]["season"], 1);
        let (status, json) = app.json("/api/shows/Nope%20Show", &token).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["error"]["message"], "That show is not in the library.");
        let (status, json) = app.json("/api/photos/timeline?limit=10", &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert!(json["months"].is_array());
    }

    /// A client that does not send `limit` gets the documented 200 photos per page, and
    /// no page holds more than 500 whatever it asks for.
    #[tokio::test]
    async fn timeline_pages_hold_200_photos_by_default_and_500_at_most() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let now = stash_core::now();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 600)
             INSERT INTO items (id, rel_path, kind, category, status, title, sort_title, size, mtime,
                                thumb_state, taken_at, added_at, updated_at)
             SELECT printf('photo-%04d', i), printf('Photos/2024/06/IMG_%04d.jpg', i), 'photo', 'photo',
                    'ready', printf('IMG_%04d', i), printf('img_%04d', i), 10, ?, 'pending',
                    1718236800 + i, ?, ? FROM n",
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&app.state.db)
        .await
        .unwrap();
        let photos = |json: &serde_json::Value| -> usize {
            json["months"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["items"].as_array().unwrap().len())
                .sum()
        };
        let (status, json) = app.json("/api/photos/timeline", &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(photos(&json), 200);
        assert_eq!(json["months"][0]["count"], 600);
        assert!(json["next_cursor"].is_string());
        let (status, json) = app.json("/api/photos/timeline?limit=1000", &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(photos(&json), 500);
    }

    /// The web links to `/api/shows/<show_name>` exactly as `/api/shows` lists it, so a
    /// name with surrounding spaces (a folder named "Bluey " on a Linux share) must work.
    #[tokio::test]
    async fn show_names_are_matched_exactly() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        app.item(
            "Shows/Bluey /Season 01/Bluey - S01E01.mkv",
            MediaKind::Video,
            Category::Episode,
            ItemStatus::Ready,
            None,
        )
        .await;
        sqlx::query("UPDATE items SET show_name = 'Bluey ', season = 1, episode = 1")
            .execute(&app.state.db)
            .await
            .unwrap();
        let (status, json) = app.json("/api/shows", &token).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json[0]["show_name"], "Bluey ");
        let (status, json) = app.json("/api/shows/Bluey%20", &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["show_name"], "Bluey ");
        let (status, _) = app.json("/api/shows/%20%20", &token).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn rescan_requires_full_scope() {
        let app = TestApp::new().await;
        let (user_id, token) = app.user("member").await;
        let (status, _, body) = app
            .send(request(
                "POST",
                "/api/library/rescan",
                Some(&token),
                Some(serde_json::json!({})),
            ))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["job_id"].as_i64().unwrap() > 0);
        let reader = app.device_token(&user_id, &["library:read"]).await;
        let (status, _, _) = app
            .send(request(
                "POST",
                "/api/library/rescan",
                Some(&reader),
                Some(serde_json::json!({})),
            ))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
