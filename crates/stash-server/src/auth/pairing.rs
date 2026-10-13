//! Device pairing (device-code flow) for the Chrome extension and Android app.
//!
//! 1. The device calls `POST /api/pairing/request` and shows `user_code`.
//! 2. A signed-in person approves the code in Settings (or at `verify_url`).
//! 3. The device polls with its `poll_secret` and receives its token exactly once.
//!
//! An approved token waits in `pending_token` until the device collects it. Until then
//! it carries an expiry ([`DELIVERY_GRACE_S`] past the request's), so a token nobody
//! collects stops working on its own even if no cleanup ever runs.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use stash_core::db::Db;
use stash_core::{new_id, now};

use super::codes;
use super::handlers::JsonBody;
use super::tokens::{self, CLIENT_KINDS};
use super::{AuthUser, Scope};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::security::ClientIp;
use crate::state::AppState;

/// Pairing requests expire after 10 minutes.
pub const PAIRING_TTL_S: i64 = 10 * 60;
/// An approved token waits this long past expiry for the device to collect it.
pub const DELIVERY_GRACE_S: i64 = 10 * 60;
/// Suggested seconds between polls (fits the 10/min pairing rate limit).
pub const POLL_INTERVAL_S: i64 = 6;
/// Most devices that may wait for approval at once. Requests are unauthenticated, so
/// this bounds the table and keeps the approval list readable.
pub const MAX_PENDING_PAIRINGS: i64 = 50;

/// Scopes a newly paired device receives.
pub fn scopes_for(client_kind: &str) -> Vec<Scope> {
    match client_kind {
        "chrome-extension" => vec![Scope::DownloadsCreate, Scope::DownloadsRead],
        "android" => vec![Scope::Full],
        _ => vec![Scope::DownloadsCreate],
    }
}

/// Wipe undelivered tokens and forget requests that are long finished.
async fn cleanup(db: &Db) -> ApiResult<()> {
    let now = now();
    let stale: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT id, token_id FROM pairing_requests WHERE status = 'approved' AND expires_at + ? < ?")
            .bind(DELIVERY_GRACE_S)
            .bind(now)
            .fetch_all(db)
            .await?;
    for (id, token_id) in stale {
        expire_undelivered(db, &id, token_id.as_deref()).await?;
    }
    sqlx::query("DELETE FROM pairing_requests WHERE expires_at < ? AND status != 'approved'")
        .bind(now - 24 * 3600)
        .execute(db)
        .await?;
    Ok(())
}

/// An approved token nobody collected is revoked and its plaintext wiped. The row ends
/// as `delivered` (nothing left to deliver), so later polls answer `expired`.
async fn expire_undelivered(db: &Db, id: &str, token_id: Option<&str>) -> ApiResult<()> {
    let mut tx = db.begin().await?;
    let done = sqlx::query(
        "UPDATE pairing_requests SET status = 'delivered', pending_token = NULL WHERE id = ? AND status = 'approved'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 1
        && let Some(token_id) = token_id
    {
        sqlx::query("DELETE FROM api_tokens WHERE id = ?")
            .bind(token_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct PairingRequestBody {
    pub client_name: String,
    pub client_kind: String,
}

#[derive(Debug, Serialize)]
pub struct PairingStarted {
    pub pairing_id: String,
    pub user_code: String,
    pub poll_secret: String,
    pub expires_at: i64,
    pub verify_url: String,
    /// Seconds to wait between polls.
    pub interval: i64,
}

/// `POST /api/pairing/request` (public, rate limited).
pub async fn request(
    State(state): State<AppState>,
    ip: ClientIp,
    JsonBody(body): JsonBody<PairingRequestBody>,
) -> ApiResult<Json<PairingStarted>> {
    state.rate_limiter.check(&ip.key("pairing"))?;
    let client_name = super::users::clean_name(&body.client_name, "Device name")?;
    let client_kind = body.client_kind.trim();
    if !CLIENT_KINDS.contains(&client_kind) {
        return Err(ApiError::bad_request(
            "Device type must be chrome-extension, android or other.",
        ));
    }
    cleanup(&state.db).await?;
    let (waiting,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM pairing_requests WHERE status = 'pending' AND expires_at > ?",
    )
    .bind(now())
    .fetch_one(&state.db)
    .await?;
    if waiting >= MAX_PENDING_PAIRINGS {
        tracing::warn!(waiting, "too many devices are waiting to pair");
        return Err(ApiError::new(
            ErrorCode::RateLimited,
            "Too many devices are waiting to pair. Try again in a few minutes.",
        ));
    }
    let pairing_id = new_id();
    let poll_secret = codes::random_b64::<32>();
    let now = now();
    let expires_at = now + PAIRING_TTL_S;
    let mut user_code = String::new();
    for attempt in 0..5 {
        user_code = codes::new_user_code();
        let result = sqlx::query(
            "INSERT INTO pairing_requests (id, user_code, poll_secret_hash, client_name, client_kind, ip, status, created_at, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
        )
        .bind(&pairing_id)
        .bind(&user_code)
        .bind(codes::sha256_hex(&poll_secret))
        .bind(&client_name)
        .bind(client_kind)
        .bind(ip.as_string())
        .bind(now)
        .bind(expires_at)
        .execute(&state.db)
        .await;
        match result {
            Ok(_) => break,
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() && attempt < 4 => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let verify_url = format!(
        "{}/pair?code={user_code}",
        super::public_base(&state.config).unwrap_or_default()
    );
    Ok(Json(PairingStarted {
        pairing_id,
        user_code,
        poll_secret,
        expires_at,
        verify_url,
        interval: POLL_INTERVAL_S,
    }))
}

/// Mark an approved request delivered and hand out its token plaintext, exactly once.
async fn take_pending_token(db: &Db, pairing_id: &str) -> ApiResult<Option<String>> {
    let mut tx = db.begin().await?;
    // Claim first: the write lock makes concurrent polls wait, and only one sees rows_affected = 1.
    let claimed = sqlx::query(
        "UPDATE pairing_requests SET status = 'delivered' WHERE id = ? AND status = 'approved'",
    )
    .bind(pairing_id)
    .execute(&mut *tx)
    .await?;
    if claimed.rows_affected() == 0 {
        tx.rollback().await?;
        return Ok(None);
    }
    // Delivered: the device keeps its token for as long as nobody revokes it.
    sqlx::query(
        "UPDATE api_tokens SET expires_at = NULL WHERE id = (SELECT token_id FROM pairing_requests WHERE id = ?)",
    )
    .bind(pairing_id)
    .execute(&mut *tx)
    .await?;
    let (token,): (Option<String>,) =
        sqlx::query_as("SELECT pending_token FROM pairing_requests WHERE id = ?")
            .bind(pairing_id)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("UPDATE pairing_requests SET pending_token = NULL WHERE id = ?")
        .bind(pairing_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(token)
}

#[derive(Debug, Deserialize)]
pub struct PollBody {
    pub pairing_id: String,
    pub poll_secret: String,
}

type PairingRow = (String, String, i64, Option<String>);

fn gone(status: &str) -> Response {
    (
        StatusCode::GONE,
        Json(serde_json::json!({ "status": status })),
    )
        .into_response()
}

/// Name shown by the device once paired: the server's host name.
fn server_name(state: &AppState) -> String {
    state
        .config
        .rp_id()
        .unwrap_or_else(|| "PopcornStash".to_string())
}

/// `POST /api/pairing/poll` (public, rate limited):
/// 202 pending, 200 approved (token delivered once), 410 denied or expired.
pub async fn poll(
    State(state): State<AppState>,
    ip: ClientIp,
    JsonBody(body): JsonBody<PollBody>,
) -> ApiResult<Response> {
    state.rate_limiter.check(&ip.key("pairing-poll"))?;
    let row: Option<PairingRow> = sqlx::query_as(
        "SELECT poll_secret_hash, status, expires_at, token_id FROM pairing_requests WHERE id = ?",
    )
    .bind(&body.pairing_id)
    .fetch_optional(&state.db)
    .await?;
    let unknown =
        || ApiError::not_found("That pairing request does not exist. Start pairing again.");
    let Some((secret_hash, status, expires_at, token_id)) = row else {
        return Err(unknown());
    };
    if !codes::constant_time_eq(&secret_hash, &codes::sha256_hex(&body.poll_secret)) {
        return Err(unknown());
    }
    let now = now();
    match status.as_str() {
        "pending" if expires_at > now => Ok((
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "status": "pending" })),
        )
            .into_response()),
        "pending" | "delivered" => Ok(gone("expired")),
        "denied" => Ok(gone("denied")),
        "approved" if expires_at + DELIVERY_GRACE_S <= now => {
            expire_undelivered(&state.db, &body.pairing_id, token_id.as_deref()).await?;
            Ok(gone("expired"))
        }
        "approved" => {
            let Some(token) = take_pending_token(&state.db, &body.pairing_id).await? else {
                return Ok(gone("expired"));
            };
            let scopes: Option<(String,)> =
                sqlx::query_as("SELECT scopes FROM api_tokens WHERE id = ?")
                    .bind(token_id.as_deref())
                    .fetch_optional(&state.db)
                    .await?;
            let Some((scopes,)) = scopes else {
                return Ok(gone("expired"));
            };
            let scopes: Vec<&str> = Scope::parse_json(&scopes)
                .into_iter()
                .map(Scope::as_str)
                .collect();
            Ok((
                StatusCode::OK,
                [super::handlers::NO_STORE],
                Json(serde_json::json!({
                    "status": "approved",
                    "token": token,
                    "scopes": scopes,
                    "server_name": server_name(&state),
                })),
            )
                .into_response())
        }
        _ => Ok(gone("expired")),
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PendingPairing {
    pub pairing_id: String,
    pub user_code: String,
    pub client_name: String,
    pub client_kind: String,
    pub ip: Option<String>,
    pub created_at: i64,
}

/// `GET /api/pairing/pending`: devices waiting for approval.
pub async fn pending(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<PendingPairing>>> {
    user.require(Scope::Full)?;
    let rows = sqlx::query_as(
        "SELECT id AS pairing_id, user_code, client_name, client_kind, ip, created_at FROM pairing_requests \
         WHERE status = 'pending' AND expires_at > ? ORDER BY created_at DESC",
    )
    .bind(now())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct UserCodeBody {
    pub user_code: String,
}

fn no_such_code() -> ApiError {
    ApiError::not_found(
        "No device is waiting with that code. Check the code on the device, or start pairing again.",
    )
}

async fn find_pending(db: &Db, typed: &str) -> ApiResult<(String, String, String)> {
    let code = codes::normalize_code(typed, 2).ok_or_else(no_such_code)?;
    sqlx::query_as(
        "SELECT id, client_name, client_kind FROM pairing_requests WHERE user_code = ? AND status = 'pending' AND expires_at > ?",
    )
    .bind(&code)
    .bind(now())
    .fetch_optional(db)
    .await?
    .ok_or_else(no_such_code)
}

/// `POST /api/pairing/approve` `{user_code}`: issues a device token owned by the approver.
pub async fn approve(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    JsonBody(body): JsonBody<UserCodeBody>,
) -> ApiResult<Json<serde_json::Value>> {
    user.require(Scope::Full)?;
    let (pairing_id, client_name, client_kind) = find_pending(&state.db, &body.user_code).await?;
    let scopes = scopes_for(&client_kind);
    let mut tx = state.db.begin().await?;
    let created =
        tokens::insert_token(&mut tx, &user.user_id, &client_name, &scopes, &client_kind).await?;
    let done = sqlx::query(
        "UPDATE pairing_requests SET status = 'approved', token_id = ?, pending_token = ?, approved_by = ? \
         WHERE id = ? AND status = 'pending'",
    )
    .bind(&created.info.id)
    .bind(&created.token)
    .bind(&user.user_id)
    .bind(&pairing_id)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(no_such_code());
    }
    // Until the device collects it, the token only lives as long as the request may be polled.
    sqlx::query(
        "UPDATE api_tokens SET expires_at = (SELECT expires_at FROM pairing_requests WHERE id = ?) + ? WHERE id = ?",
    )
    .bind(&pairing_id)
    .bind(DELIVERY_GRACE_S)
    .bind(&created.info.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "device_paired",
            serde_json::json!({
                "pairing_id": pairing_id,
                "token_id": created.info.id,
                "client_name": client_name,
                "client_kind": client_kind,
                "scopes": created.info.scopes,
            }),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /api/pairing/deny` `{user_code}`.
pub async fn deny(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    JsonBody(body): JsonBody<UserCodeBody>,
) -> ApiResult<Json<serde_json::Value>> {
    user.require(Scope::Full)?;
    let (pairing_id, client_name, client_kind) = find_pending(&state.db, &body.user_code).await?;
    sqlx::query(
        "UPDATE pairing_requests SET status = 'denied' WHERE id = ? AND status = 'pending'",
    )
    .bind(&pairing_id)
    .execute(&state.db)
    .await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "pairing_denied",
            serde_json::json!({"pairing_id": pairing_id, "client_name": client_name, "client_kind": client_kind}),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_scopes() {
        assert_eq!(
            scopes_for("chrome-extension"),
            vec![Scope::DownloadsCreate, Scope::DownloadsRead]
        );
        assert_eq!(scopes_for("android"), vec![Scope::Full]);
        assert_eq!(scopes_for("other"), vec![Scope::DownloadsCreate]);
    }
}
