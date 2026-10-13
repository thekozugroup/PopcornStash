//! Session cookies and bearer authentication.
//!
//! Sessions are 256-bit random tokens (`pst_…`) stored as SHA-256 hex in
//! `sessions.id`. Browsers carry them in an HttpOnly, SameSite=Strict cookie;
//! the Android app receives one as a bearer token from `login/finish`.
//! Device tokens (`api_tokens`) are bearer-only and carry scopes.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::{self, HeaderMap, HeaderValue};
use axum::http::request::Parts;
use axum::response::{AppendHeaders, IntoResponse, Response};
use axum_extra::extract::cookie::{Cookie, SameSite};
use serde::Serialize;
use stash_core::config::Config;
use stash_core::db::Db;
use stash_core::models::Role;
use stash_core::now;

use super::codes::{self, TOKEN_PREFIX};
use super::{AuthUser, MaybeAuthUser, Scope, UserView};
use crate::error::{ApiError, ApiResult};
use crate::security::ClientIp;
use crate::state::AppState;

/// Sliding session lifetime.
pub const SESSION_TTL_S: i64 = 30 * 24 * 3600;
/// Absolute cap on a session's age.
pub const SESSION_MAX_AGE_S: i64 = 180 * 24 * 3600;
/// `last_seen_at`/`expires_at` are refreshed at most this often.
pub const SESSION_REFRESH_S: i64 = 300;
/// `api_tokens.last_used_at` is updated at most this often.
pub const TOKEN_TOUCH_S: i64 = 60;

const MAX_USER_AGENT: usize = 300;

/// `sessions.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Cookie,
    Bearer,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionKind::Cookie => "cookie",
            SessionKind::Bearer => "bearer",
        }
    }
}

/// `__Host-stash` on HTTPS, `stash` on plain-HTTP localhost.
pub fn cookie_name(config: &Config) -> &'static str {
    if config.is_https() {
        "__Host-stash"
    } else {
        "stash"
    }
}

fn base_cookie(config: &Config, value: String) -> Cookie<'static> {
    Cookie::build((cookie_name(config), value))
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/")
        .secure(config.is_https())
        .build()
}

/// `Set-Cookie` value for a new session. The browser may keep it for the absolute
/// session cap; the server enforces the 30-day sliding expiry.
pub fn session_cookie(config: &Config, token: &str) -> HeaderValue {
    let mut cookie = base_cookie(config, token.to_string());
    cookie.set_max_age(time::Duration::seconds(SESSION_MAX_AGE_S));
    header_value(&cookie)
}

/// `Set-Cookie` value that removes the session cookie.
pub fn clear_cookie(config: &Config) -> HeaderValue {
    let mut cookie = base_cookie(config, String::new());
    cookie.set_max_age(time::Duration::ZERO);
    header_value(&cookie)
}

fn header_value(cookie: &Cookie<'_>) -> HeaderValue {
    // Cookie values are base64url and attribute names are ASCII, so this cannot fail.
    HeaderValue::from_str(&cookie.encoded().to_string())
        .unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// The session token from the request's cookie header, if any.
pub fn token_from_cookie(headers: &HeaderMap, config: &Config) -> Option<String> {
    let name = cookie_name(config);
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| Cookie::parse(pair.trim().to_string()).ok())
        .find(|c| c.name() == name)
        .map(|c| c.value().to_string())
        .filter(|v| v.starts_with(TOKEN_PREFIX))
}

/// The token from `Authorization: Bearer …`. `Some(None)` when the header is
/// present but not a usable bearer token.
fn bearer_token(headers: &HeaderMap) -> Option<Option<String>> {
    let value = headers.get(header::AUTHORIZATION)?;
    let token = value
        .to_str()
        .ok()
        .filter(|v| v.len() > 7 && v[..7].eq_ignore_ascii_case("bearer "))
        .map(|v| v[7..].trim().to_string())
        .filter(|t| t.starts_with(TOKEN_PREFIX));
    Some(token)
}

/// A freshly created session. `token` is the only copy of the plaintext.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub id: String,
    pub token: String,
    pub expires_at: i64,
}

/// Start a session for `user_id`.
pub async fn create_session(
    db: &Db,
    user_id: &str,
    kind: SessionKind,
    user_agent: Option<&str>,
    ip: Option<&str>,
) -> ApiResult<NewSession> {
    let token = codes::new_token();
    let id = codes::sha256_hex(&token);
    let now = now();
    let expires_at = now + SESSION_TTL_S;
    let user_agent = user_agent.map(|ua| truncate(ua, MAX_USER_AGENT));
    sqlx::query("DELETE FROM sessions WHERE expires_at < ? OR created_at < ?")
        .bind(now)
        .bind(now - SESSION_MAX_AGE_S)
        .execute(db)
        .await?;
    sqlx::query(
        "INSERT INTO sessions (id, user_id, kind, created_at, last_seen_at, expires_at, user_agent, ip) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(kind.as_str())
    .bind(now)
    .bind(now)
    .bind(expires_at)
    .bind(user_agent)
    .bind(ip)
    .execute(db)
    .await?;
    Ok(NewSession {
        id,
        token,
        expires_at,
    })
}

pub(crate) fn truncate(value: &str, max: usize) -> String {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

/// Resolve the caller from the session cookie or `Authorization: Bearer`.
///
/// A request with an `Authorization` header is judged on that header alone; the
/// cookie is never consulted, so the CSRF guard can let bearer requests through.
pub async fn authenticate(parts: &mut Parts, state: &AppState) -> ApiResult<AuthUser> {
    if let Some(user) = parts.extensions.get::<AuthUser>() {
        return Ok(user.clone());
    }
    let ip = ClientIp::from_parts(parts, &state.config).as_string();
    let user = match bearer_token(&parts.headers) {
        Some(Some(token)) => {
            let hash = codes::sha256_hex(&token);
            match session_user(&state.db, &hash, SessionKind::Bearer, ip.as_deref()).await? {
                Some(user) => user,
                None => token_user(&state.db, &hash)
                    .await?
                    .ok_or_else(ApiError::unauthorized)?,
            }
        }
        Some(None) => return Err(ApiError::unauthorized()),
        None => {
            let token = token_from_cookie(&parts.headers, &state.config)
                .ok_or_else(ApiError::unauthorized)?;
            let hash = codes::sha256_hex(&token);
            session_user(&state.db, &hash, SessionKind::Cookie, ip.as_deref())
                .await?
                .ok_or_else(ApiError::unauthorized)?
        }
    };
    parts.extensions.insert(user.clone());
    Ok(user)
}

type SessionRow = (String, i64, i64, i64, String, Role);

async fn session_user(
    db: &Db,
    id: &str,
    kind: SessionKind,
    ip: Option<&str>,
) -> ApiResult<Option<AuthUser>> {
    let row: Option<SessionRow> = sqlx::query_as(
        "SELECT s.user_id, s.created_at, s.last_seen_at, s.expires_at, u.display_name, u.role \
         FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.id = ? AND s.kind = ?",
    )
    .bind(id)
    .bind(kind.as_str())
    .fetch_optional(db)
    .await?;
    let Some((user_id, created_at, last_seen_at, expires_at, display_name, role)) = row else {
        return Ok(None);
    };
    let now = now();
    let hard_limit = created_at + SESSION_MAX_AGE_S;
    if expires_at <= now || hard_limit <= now {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(db)
            .await?;
        return Ok(None);
    }
    if now - last_seen_at >= SESSION_REFRESH_S {
        let new_expiry = (now + SESSION_TTL_S).min(hard_limit);
        sqlx::query("UPDATE sessions SET last_seen_at = ?, expires_at = ?, ip = COALESCE(?, ip) WHERE id = ?")
            .bind(now)
            .bind(new_expiry)
            .bind(ip)
            .bind(id)
            .execute(db)
            .await?;
    }
    Ok(Some(AuthUser {
        user_id,
        display_name,
        role,
        scopes: vec![Scope::Full],
        session_id: Some(id.to_string()),
        token_id: None,
    }))
}

type TokenRow = (
    String,
    String,
    String,
    Option<i64>,
    Option<i64>,
    String,
    Role,
);

async fn token_user(db: &Db, hash: &str) -> ApiResult<Option<AuthUser>> {
    let row: Option<TokenRow> = sqlx::query_as(
        "SELECT t.id, t.user_id, t.scopes, t.last_used_at, t.expires_at, u.display_name, u.role \
         FROM api_tokens t JOIN users u ON u.id = t.user_id WHERE t.token_hash = ?",
    )
    .bind(hash)
    .fetch_optional(db)
    .await?;
    let Some((token_id, user_id, scopes, last_used_at, expires_at, display_name, role)) = row
    else {
        return Ok(None);
    };
    let now = now();
    if expires_at.is_some_and(|e| e <= now) {
        return Ok(None);
    }
    if last_used_at.is_none_or(|t| now - t >= TOKEN_TOUCH_S) {
        sqlx::query("UPDATE api_tokens SET last_used_at = ? WHERE id = ?")
            .bind(now)
            .bind(&token_id)
            .execute(db)
            .await?;
    }
    let scopes = Scope::parse_json(&scopes);
    if scopes.is_empty() {
        return Ok(None);
    }
    Ok(Some(AuthUser {
        user_id,
        display_name,
        role,
        scopes,
        session_id: None,
        token_id: Some(token_id),
    }))
}

// ───────────────────────────── handlers ─────────────────────────────

/// `GET /api/auth/state` (public).
#[derive(Debug, Serialize)]
pub struct AuthState {
    pub setup_required: bool,
    pub authenticated: bool,
    pub user: Option<UserView>,
    pub public_url: Option<String>,
    pub rp_id: Option<String>,
    pub passkeys_ready: bool,
    pub reason: Option<String>,
}

pub async fn auth_state(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
) -> ApiResult<Json<AuthState>> {
    let (users,): (i64,) = sqlx::query_as("SELECT count(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    let user = match user {
        Some(u) => super::users::load_user(&state.db, &u.user_id).await?,
        None => None,
    };
    let config = &state.config;
    let ready = config.passkeys_ready();
    Ok(Json(AuthState {
        setup_required: users == 0,
        authenticated: user.is_some(),
        user,
        public_url: super::public_base(config),
        rp_id: config.rp_id(),
        passkeys_ready: ready,
        reason: (!ready).then(|| super::passkeys::unavailable_reason(config)),
    }))
}

/// `GET /api/me`.
pub async fn me(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<UserView>> {
    super::users::load_user(&state.db, &user.user_id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::unauthorized)
}

/// `POST /api/auth/logout`: ends this session (or revokes this device token) and clears the cookie.
pub async fn logout(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
) -> ApiResult<Response> {
    if let Some(session_id) = &user.session_id {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(token_id) = &user.token_id {
        sqlx::query("DELETE FROM api_tokens WHERE id = ?")
            .bind(token_id)
            .execute(&state.db)
            .await?;
    }
    state
        .auth
        .audit(
            Some(&user.user_id),
            "logout",
            serde_json::json!({}),
            ip.as_string().as_deref(),
        )
        .await;
    Ok((
        StatusCode::NO_CONTENT,
        AppendHeaders([(header::SET_COOKIE, clear_cookie(&state.config))]),
    )
        .into_response())
}

/// `Session` (section 8).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SessionView {
    pub id: String,
    pub kind: String,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub user_agent: Option<String>,
    pub ip: Option<String>,
    #[sqlx(skip)]
    pub current: bool,
}

/// `GET /api/me/sessions`.
pub async fn list_sessions(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<SessionView>>> {
    user.require(Scope::Full)?;
    let mut rows: Vec<SessionView> = sqlx::query_as(
        "SELECT id, kind, created_at, last_seen_at, user_agent, ip FROM sessions \
         WHERE user_id = ? AND expires_at > ? ORDER BY last_seen_at DESC",
    )
    .bind(&user.user_id)
    .bind(now())
    .fetch_all(&state.db)
    .await?;
    for row in &mut rows {
        row.current = user.session_id.as_deref() == Some(row.id.as_str());
    }
    Ok(Json(rows))
}

/// `DELETE /api/me/sessions/:id`. Revoking the current session also clears its cookie.
pub async fn revoke_session(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    user.require(Scope::Full)?;
    let done = sqlx::query("DELETE FROM sessions WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user.user_id)
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("That session has already ended."));
    }
    state
        .auth
        .audit(
            Some(&user.user_id),
            "session_revoked",
            serde_json::json!({}),
            ip.as_string().as_deref(),
        )
        .await;
    if user.session_id.as_deref() == Some(id.as_str()) {
        return Ok((
            StatusCode::NO_CONTENT,
            AppendHeaders([(header::SET_COOKIE, clear_cookie(&state.config))]),
        )
            .into_response());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn config(url: &str) -> Config {
        let mut c = Config::for_tests(Path::new("/tmp/s"), Path::new("/tmp/c"));
        c.public_url = Some(url::Url::parse(url).unwrap());
        c
    }

    #[test]
    fn cookie_on_https() {
        let c = config("https://stash.example.com");
        assert_eq!(cookie_name(&c), "__Host-stash");
        let v = session_cookie(&c, "pst_abc").to_str().unwrap().to_string();
        assert!(v.starts_with("__Host-stash=pst_abc"), "{v}");
        assert!(v.contains("HttpOnly"));
        assert!(v.contains("SameSite=Strict"));
        assert!(v.contains("Path=/"));
        assert!(v.contains("Secure"));
        assert!(!v.contains("Domain"));
        assert!(v.contains(&format!("Max-Age={SESSION_MAX_AGE_S}")));
        let clear = clear_cookie(&c).to_str().unwrap().to_string();
        assert!(clear.starts_with("__Host-stash=;"), "{clear}");
        assert!(clear.contains("Max-Age=0"));
    }

    #[test]
    fn cookie_on_localhost() {
        let c = config("http://localhost:8788");
        assert_eq!(cookie_name(&c), "stash");
        let v = session_cookie(&c, "pst_abc").to_str().unwrap().to_string();
        assert!(v.starts_with("stash=pst_abc"));
        assert!(v.contains("HttpOnly"));
        assert!(v.contains("SameSite=Strict"));
        assert!(v.contains("Path=/"));
        assert!(!v.contains("Secure"));
    }

    #[test]
    fn reads_cookie_and_bearer() {
        let c = config("https://stash.example.com");
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("a=b; __Host-stash=pst_xyz; stash=pst_other"),
        );
        assert_eq!(token_from_cookie(&h, &c).as_deref(), Some("pst_xyz"));
        let local = config("http://localhost:8788");
        assert_eq!(token_from_cookie(&h, &local).as_deref(), Some("pst_other"));
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("__Host-stash=nope"),
        );
        assert_eq!(token_from_cookie(&h, &c), None);

        let mut h = HeaderMap::new();
        assert_eq!(bearer_token(&h), None);
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer pst_tok"),
        );
        assert_eq!(bearer_token(&h), Some(Some("pst_tok".into())));
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer other"),
        );
        assert_eq!(bearer_token(&h), Some(None));
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic abc"));
        assert_eq!(bearer_token(&h), Some(None));
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate("héllo", 2), "h");
        assert_eq!(truncate("hello", 10), "hello");
    }
}
