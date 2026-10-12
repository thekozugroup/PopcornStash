//! Users, invites, setup/recovery codes and the audit log.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use stash_core::config::Config;
use stash_core::db::Db;
use stash_core::models::Role;
use stash_core::{new_id, now};

use super::codes;
use super::{AuthService, AuthUser, UserView};
use crate::error::{ApiError, ApiResult};
use crate::security::ClientIp;
use crate::state::AppState;

/// Setup codes stay valid for 24 hours (and are replaced on every start until used).
pub const SETUP_CODE_TTL_S: i64 = 24 * 3600;
/// Owner reset links stay valid for 30 minutes.
pub const OWNER_RESET_TTL_S: i64 = 30 * 60;
/// Default and maximum invite lifetimes.
pub const INVITE_DEFAULT_HOURS: i64 = 72;
pub const INVITE_MAX_HOURS: i64 = 30 * 24;
/// Longest display name, passkey name or device name.
pub const MAX_NAME_CHARS: usize = 60;

/// Trim and validate a person-facing name.
pub fn clean_name(value: &str, field: &str) -> ApiResult<String> {
    let value = value.trim();
    let chars = value.chars().count();
    if chars == 0 || chars > MAX_NAME_CHARS {
        return Err(ApiError::bad_request(format!("{field} must be 1 to {MAX_NAME_CHARS} characters.")));
    }
    if value.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!("{field} cannot contain control characters.")));
    }
    Ok(value.to_string())
}

/// `User` by id.
pub async fn load_user(db: &Db, user_id: &str) -> ApiResult<Option<UserView>> {
    Ok(sqlx::query_as("SELECT id, display_name, role, created_at FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(db)
        .await?)
}

pub async fn user_count(db: &Db) -> ApiResult<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM users").fetch_one(db).await?;
    Ok(n)
}

/// When no users exist, replace any unused setup code with a fresh one and return it.
pub async fn ensure_setup_code(auth: &AuthService) -> ApiResult<Option<String>> {
    let db = &auth.db;
    if user_count(db).await? > 0 {
        return Ok(None);
    }
    let code = codes::new_setup_code();
    let now = now();
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM recovery_codes WHERE kind = 'setup' AND used_at IS NULL").execute(&mut *tx).await?;
    sqlx::query("INSERT INTO recovery_codes (id, code_hash, kind, created_at, expires_at) VALUES (?, ?, 'setup', ?, ?)")
        .bind(new_id())
        .bind(codes::sha256_hex(&code))
        .bind(now)
        .bind(now + SETUP_CODE_TTL_S)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(code))
}

/// Hash of a typed setup code when it is currently valid.
pub async fn valid_setup_code(db: &Db, typed: &str) -> ApiResult<Option<String>> {
    let Some(code) = codes::normalize_code(typed, 3) else { return Ok(None) };
    let hash = codes::sha256_hex(&code);
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM recovery_codes WHERE code_hash = ? AND kind = 'setup' AND used_at IS NULL AND expires_at > ?",
    )
    .bind(&hash)
    .bind(now())
    .fetch_optional(db)
    .await?;
    Ok(row.map(|_| hash))
}

/// Append to the audit log. Never fails the caller.
pub async fn audit(db: &Db, user_id: Option<&str>, event: &str, detail: serde_json::Value, ip: Option<&str>) {
    let result = sqlx::query("INSERT INTO audit_log (at, user_id, event, detail, ip) VALUES (?, ?, ?, ?, ?)")
        .bind(now())
        .bind(user_id)
        .bind(event)
        .bind(detail.to_string())
        .bind(ip)
        .execute(db)
        .await;
    if let Err(e) = result {
        tracing::warn!(error = %e, event, "could not write the audit log");
    }
}

/// One-time owner recovery link, valid 30 minutes.
pub async fn create_owner_reset_link(db: &Db, config: &Config) -> anyhow::Result<String> {
    let owners: (i64,) = sqlx::query_as("SELECT count(*) FROM users WHERE role = 'owner'").fetch_one(db).await?;
    if owners.0 == 0 {
        anyhow::bail!("There is no owner yet. Finish setup with the setup code from the container log instead.");
    }
    let base = super::public_base(config)
        .ok_or_else(|| anyhow::anyhow!("Set STASH_PUBLIC_URL to your HTTPS address first; recovery links need it."))?;
    let code = codes::random_b64::<16>();
    let now = now();
    sqlx::query("DELETE FROM recovery_codes WHERE kind = 'owner_reset' AND expires_at < ?").bind(now).execute(db).await?;
    sqlx::query("INSERT INTO recovery_codes (id, code_hash, kind, created_at, expires_at) VALUES (?, ?, 'owner_reset', ?, ?)")
        .bind(new_id())
        .bind(codes::sha256_hex(&code))
        .bind(now)
        .bind(now + OWNER_RESET_TTL_S)
        .execute(db)
        .await?;
    audit(db, None, "owner_reset_link_created", serde_json::json!({}), None).await;
    Ok(format!("{base}/recover/{code}"))
}

/// Id of a currently valid owner reset code.
pub async fn valid_reset_code(db: &Db, code: &str) -> ApiResult<Option<String>> {
    let code = code.trim();
    if code.is_empty() || code.len() > 64 {
        return Ok(None);
    }
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM recovery_codes WHERE code_hash = ? AND kind = 'owner_reset' AND used_at IS NULL AND expires_at > ?",
    )
    .bind(codes::sha256_hex(code))
    .bind(now())
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| r.0))
}

/// A valid invite: `(id, role, expires_at)`.
pub async fn valid_invite(db: &Db, code: &str) -> ApiResult<Option<(String, Role, i64)>> {
    let code = code.trim();
    if code.is_empty() || code.len() > 64 {
        return Ok(None);
    }
    Ok(sqlx::query_as("SELECT id, role, expires_at FROM invites WHERE code_hash = ? AND used_at IS NULL AND expires_at > ?")
        .bind(codes::sha256_hex(code))
        .bind(now())
        .fetch_optional(db)
        .await?)
}

// ───────────────────────────── people ─────────────────────────────

/// `GET /api/users` (owner).
pub async fn list_users(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<UserView>>> {
    user.require_owner()?;
    let users = sqlx::query_as("SELECT id, display_name, role, created_at FROM users ORDER BY created_at, id")
        .fetch_all(&state.db)
        .await?;
    Ok(Json(users))
}

#[derive(Debug, Deserialize)]
pub struct UpdateUser {
    pub role: Option<Role>,
    pub display_name: Option<String>,
}

fn last_owner() -> ApiError {
    ApiError::conflict("PopcornStash needs at least one owner. Make someone else an owner first.")
}

/// `PATCH /api/users/:id` (owner): `{role}`, optionally `{display_name}`.
pub async fn update_user(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateUser>,
) -> ApiResult<Json<UserView>> {
    user.require_owner()?;
    let current = load_user(&state.db, &id).await?.ok_or_else(|| ApiError::not_found("That person no longer exists."))?;
    if let Some(name) = &body.display_name {
        let name = clean_name(name, "Name")?;
        sqlx::query("UPDATE users SET display_name = ? WHERE id = ?").bind(&name).bind(&id).execute(&state.db).await?;
    }
    if let Some(role) = body.role.filter(|r| *r != current.role) {
        let done = sqlx::query(
            "UPDATE users SET role = ? WHERE id = ? AND (? = 'owner' OR role = 'member' \
             OR (SELECT count(*) FROM users WHERE role = 'owner') > 1)",
        )
        .bind(role.as_str())
        .bind(&id)
        .bind(role.as_str())
        .execute(&state.db)
        .await?;
        if done.rows_affected() == 0 {
            return Err(last_owner());
        }
        state
            .auth
            .audit(
                Some(&user.user_id),
                "user_role_changed",
                serde_json::json!({"user_id": id, "role": role.as_str()}),
                ip.as_string().as_deref(),
            )
            .await;
    }
    load_user(&state.db, &id).await?.map(Json).ok_or_else(|| ApiError::not_found("That person no longer exists."))
}

/// `DELETE /api/users/:id` (owner). Removes their passkeys, sessions and devices too.
pub async fn delete_user(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    user.require_owner()?;
    let target = load_user(&state.db, &id).await?.ok_or_else(|| ApiError::not_found("That person no longer exists."))?;
    let done = sqlx::query(
        "DELETE FROM users WHERE id = ? AND (role = 'member' OR (SELECT count(*) FROM users WHERE role = 'owner') > 1)",
    )
    .bind(&id)
    .execute(&state.db)
    .await?;
    if done.rows_affected() == 0 {
        return Err(last_owner());
    }
    state
        .auth
        .audit(
            Some(&user.user_id),
            "user_removed",
            serde_json::json!({"user_id": id, "display_name": target.display_name}),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}

// ───────────────────────────── invites ─────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateInvite {
    pub role: Option<Role>,
    pub expires_in_hours: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CreatedInvite {
    pub id: String,
    pub url: String,
    pub code: String,
    pub role: Role,
    pub expires_at: i64,
}

/// `POST /api/invites` (owner).
pub async fn create_invite(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    Json(body): Json<CreateInvite>,
) -> ApiResult<Json<CreatedInvite>> {
    user.require_owner()?;
    let hours = body.expires_in_hours.unwrap_or(INVITE_DEFAULT_HOURS);
    if !(1..=INVITE_MAX_HOURS).contains(&hours) {
        return Err(ApiError::bad_request("Invites can last from 1 hour to 30 days."));
    }
    let role = body.role.unwrap_or(Role::Member);
    let code = codes::random_b64::<16>();
    let id = new_id();
    let now = now();
    let expires_at = now + hours * 3600;
    sqlx::query("INSERT INTO invites (id, code_hash, role, created_by, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(codes::sha256_hex(&code))
        .bind(role.as_str())
        .bind(&user.user_id)
        .bind(now)
        .bind(expires_at)
        .execute(&state.db)
        .await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "invite_created",
            serde_json::json!({"invite_id": id, "role": role.as_str(), "expires_at": expires_at}),
            ip.as_string().as_deref(),
        )
        .await;
    let url = format!("{}/invite/{code}", super::public_base(&state.config).unwrap_or_default());
    Ok(Json(CreatedInvite { id, url, code, role, expires_at }))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct InviteView {
    pub id: String,
    pub role: Role,
    pub created_by: Option<String>,
    pub created_by_name: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub used_at: Option<i64>,
    pub used_by: Option<String>,
    pub used_by_name: Option<String>,
    /// `active`, `used` or `expired`.
    #[sqlx(skip)]
    pub status: &'static str,
}

/// `GET /api/invites` (owner): newest first. Codes are never shown again.
pub async fn list_invites(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<InviteView>>> {
    user.require_owner()?;
    let mut invites: Vec<InviteView> = sqlx::query_as(
        "SELECT i.id, i.role, i.created_by, c.display_name AS created_by_name, i.created_at, i.expires_at, \
                i.used_at, i.used_by, u.display_name AS used_by_name \
         FROM invites i LEFT JOIN users c ON c.id = i.created_by LEFT JOIN users u ON u.id = i.used_by \
         ORDER BY i.created_at DESC, i.id DESC",
    )
    .fetch_all(&state.db)
    .await?;
    let now = now();
    for invite in &mut invites {
        invite.status = if invite.used_at.is_some() {
            "used"
        } else if invite.expires_at <= now {
            "expired"
        } else {
            "active"
        };
    }
    Ok(Json(invites))
}

/// `DELETE /api/invites/:id` (owner).
pub async fn delete_invite(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    user.require_owner()?;
    let done = sqlx::query("DELETE FROM invites WHERE id = ?").bind(&id).execute(&state.db).await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("That invite no longer exists."));
    }
    state
        .auth
        .audit(Some(&user.user_id), "invite_revoked", serde_json::json!({"invite_id": id}), ip.as_string().as_deref())
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub struct InviteInfo {
    pub valid: bool,
    pub role: Option<Role>,
    pub expires_at: Option<i64>,
}

/// `GET /api/invites/:code` (public): whether an invite link still works. Reveals nothing else.
pub async fn invite_info(State(state): State<AppState>, ip: ClientIp, Path(code): Path<String>) -> ApiResult<Json<InviteInfo>> {
    state.rate_limiter.check(&ip.key("invite"))?;
    Ok(Json(match valid_invite(&state.db, &code).await? {
        Some((_, role, expires_at)) => InviteInfo { valid: true, role: Some(role), expires_at: Some(expires_at) },
        None => InviteInfo { valid: false, role: None, expires_at: None },
    }))
}

// ───────────────────────────── audit ─────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuditUser {
    pub id: String,
    /// None when the person has since been removed.
    pub display_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub at: i64,
    pub user: Option<AuditUser>,
    pub event: String,
    pub detail: serde_json::Value,
    pub ip: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuditPage {
    pub items: Vec<AuditEntry>,
    pub next_cursor: Option<String>,
}

type AuditRow = (i64, i64, Option<String>, Option<String>, String, String, Option<String>);

/// `GET /api/audit?limit&cursor` (owner): newest first.
pub async fn list_audit(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<AuditQuery>,
) -> ApiResult<Json<AuditPage>> {
    user.require_owner()?;
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let before = match query.cursor.as_deref().filter(|c| !c.is_empty()) {
        Some(c) => Some(c.parse::<i64>().map_err(|_| ApiError::bad_request("That page link is not valid."))?),
        None => None,
    };
    let rows: Vec<AuditRow> = sqlx::query_as(
        "SELECT a.id, a.at, a.user_id, u.display_name, a.event, a.detail, a.ip \
         FROM audit_log a LEFT JOIN users u ON u.id = a.user_id \
         WHERE (? IS NULL OR a.id < ?) ORDER BY a.id DESC LIMIT ?",
    )
    .bind(before)
    .bind(before)
    .bind(limit + 1)
    .fetch_all(&state.db)
    .await?;
    let more = rows.len() as i64 > limit;
    let items: Vec<AuditEntry> = rows
        .into_iter()
        .take(limit as usize)
        .map(|(id, at, user_id, display_name, event, detail, ip)| AuditEntry {
            id,
            at,
            user: user_id.map(|id| AuditUser { id, display_name }),
            event,
            detail: serde_json::from_str(&detail).unwrap_or_else(|_| serde_json::json!({})),
            ip,
        })
        .collect();
    let next_cursor = if more { items.last().map(|e| e.id.to_string()) } else { None };
    Ok(Json(AuditPage { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(clean_name("  Ana  ", "Name").unwrap(), "Ana");
        assert!(clean_name("", "Name").is_err());
        assert!(clean_name("   ", "Name").is_err());
        assert!(clean_name(&"x".repeat(61), "Name").is_err());
        assert_eq!(clean_name(&"é".repeat(60), "Name").unwrap().chars().count(), 60);
        assert!(clean_name("a\u{0007}b", "Name").is_err());
    }

    #[tokio::test]
    async fn setup_code_lifecycle() {
        let db = stash_core::db::open_memory().await.unwrap();
        let config = std::sync::Arc::new(Config::for_tests(std::path::Path::new("/tmp/s"), std::path::Path::new("/tmp/c")));
        let auth = AuthService::new(config, db.clone()).unwrap();
        let first = auth.ensure_setup_code().await.unwrap().unwrap();
        assert!(valid_setup_code(&db, &first).await.unwrap().is_some());
        assert!(valid_setup_code(&db, &first.to_lowercase().replace('-', " ")).await.unwrap().is_some());
        // A restart replaces the code.
        let second = auth.ensure_setup_code().await.unwrap().unwrap();
        assert_ne!(first, second);
        assert!(valid_setup_code(&db, &first).await.unwrap().is_none());
        assert!(valid_setup_code(&db, &second).await.unwrap().is_some());
        assert!(valid_setup_code(&db, "AAAA-BBBB-CCCC").await.unwrap().is_none());
        // Stored hashed only.
        let (stored,): (String,) = sqlx::query_as("SELECT code_hash FROM recovery_codes").fetch_one(&db).await.unwrap();
        assert_eq!(stored, codes::sha256_hex(&second));
        // Once someone exists there is no setup code.
        sqlx::query("INSERT INTO users (id, display_name, role, webauthn_id, created_at) VALUES ('u', 'Ana', 'owner', 'w', 0)")
            .execute(&db)
            .await
            .unwrap();
        assert!(auth.ensure_setup_code().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn owner_reset_link() {
        let db = stash_core::db::open_memory().await.unwrap();
        let config = Config::for_tests(std::path::Path::new("/tmp/s"), std::path::Path::new("/tmp/c"));
        assert!(create_owner_reset_link(&db, &config).await.is_err());
        sqlx::query("INSERT INTO users (id, display_name, role, webauthn_id, created_at) VALUES ('u', 'Ana', 'owner', 'w', 0)")
            .execute(&db)
            .await
            .unwrap();
        let link = create_owner_reset_link(&db, &config).await.unwrap();
        let code = link.strip_prefix("http://localhost:8788/recover/").unwrap();
        assert_eq!(code.len(), 22);
        assert!(valid_reset_code(&db, code).await.unwrap().is_some());
        assert!(valid_reset_code(&db, "nope").await.unwrap().is_none());
        audit(&db, None, "test", serde_json::json!({"a": 1}), Some("192.0.2.1")).await;
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM audit_log").fetch_one(&db).await.unwrap();
        assert_eq!(n, 2);
    }
}
