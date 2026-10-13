//! Device/API tokens (`pst_…`).
//!
//! Tokens are created by device pairing or by hand in Settings → Devices. The
//! plaintext is returned once; the database keeps its SHA-256 and an 8-character
//! display prefix.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use stash_core::db::Db;
use stash_core::{new_id, now};

use super::codes;
use super::handlers::JsonBody;
use super::{AuthUser, Scope};
use crate::error::{ApiError, ApiResult};
use crate::security::ClientIp;
use crate::state::AppState;

/// Client kinds accepted for tokens and pairing.
pub const CLIENT_KINDS: [&str; 3] = ["chrome-extension", "android", "other"];

/// `Token` (section 8).
#[derive(Debug, Clone, Serialize)]
pub struct TokenView {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub client_kind: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

/// `Token & {token}`: the only response that contains the plaintext.
#[derive(Debug, Serialize)]
pub struct CreatedToken {
    #[serde(flatten)]
    pub info: TokenView,
    pub token: String,
}

type TokenRow = (String, String, String, String, String, i64, Option<i64>);

fn view((id, name, prefix, scopes, client_kind, created_at, last_used_at): TokenRow) -> TokenView {
    TokenView {
        id,
        name,
        prefix,
        scopes: Scope::parse_json(&scopes)
            .iter()
            .map(|s| s.as_str().to_string())
            .collect(),
        client_kind,
        created_at,
        last_used_at,
    }
}

/// Validate requested scopes: at least one, all known, duplicates removed, `full` alone.
pub fn validate_scopes(requested: &[String]) -> ApiResult<Vec<Scope>> {
    let mut scopes = Vec::new();
    for s in requested {
        let scope = Scope::parse(s.trim()).ok_or_else(|| {
            ApiError::bad_request(format!("{s:?} is not a permission PopcornStash knows."))
        })?;
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    if scopes.is_empty() {
        return Err(ApiError::bad_request(
            "Choose at least one permission for this device.",
        ));
    }
    if scopes.contains(&Scope::Full) {
        return Ok(vec![Scope::Full]);
    }
    Ok(scopes)
}

/// Insert a token row and return it with its plaintext.
pub async fn insert_token(
    conn: &mut SqliteConnection,
    user_id: &str,
    name: &str,
    scopes: &[Scope],
    client_kind: &str,
) -> ApiResult<CreatedToken> {
    let token = codes::new_token();
    let id = new_id();
    let prefix = codes::token_display_prefix(&token);
    let now = now();
    sqlx::query(
        "INSERT INTO api_tokens (id, user_id, name, token_hash, prefix, scopes, client_kind, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(name)
    .bind(codes::sha256_hex(&token))
    .bind(&prefix)
    .bind(Scope::to_json(scopes))
    .bind(client_kind)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(CreatedToken {
        info: TokenView {
            id,
            name: name.to_string(),
            prefix,
            scopes: scopes.iter().map(|s| s.as_str().to_string()).collect(),
            client_kind: client_kind.to_string(),
            created_at: now,
            last_used_at: None,
        },
        token,
    })
}

async fn list_for(db: &Db, user_id: &str) -> ApiResult<Vec<TokenView>> {
    let rows: Vec<TokenRow> = sqlx::query_as(
        "SELECT id, name, prefix, scopes, client_kind, created_at, last_used_at FROM api_tokens \
         WHERE user_id = ? ORDER BY created_at DESC, id DESC",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(view).collect())
}

/// `GET /api/tokens`: the caller's devices.
pub async fn list_tokens(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<TokenView>>> {
    user.require(Scope::Full)?;
    Ok(Json(list_for(&state.db, &user.user_id).await?))
}

#[derive(Debug, Deserialize)]
pub struct CreateToken {
    pub name: String,
    pub scopes: Vec<String>,
    /// Owner only: create a limited token for someone else (never `full`).
    pub user_id: Option<String>,
    pub client_kind: Option<String>,
}

/// `POST /api/tokens`: `{name, scopes}` → `Token & {token}`.
pub async fn create_token(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    JsonBody(body): JsonBody<CreateToken>,
) -> ApiResult<impl IntoResponse> {
    user.require(Scope::Full)?;
    let name = super::users::clean_name(&body.name, "Device name")?;
    let scopes = validate_scopes(&body.scopes)?;
    let owner_id = match body.user_id.as_deref() {
        None => user.user_id.clone(),
        Some(id) if id == user.user_id => user.user_id.clone(),
        Some(id) => {
            if !user.is_owner() {
                return Err(ApiError::forbidden(
                    "You can only create devices for yourself.",
                ));
            }
            // A full-access token would let the owner act as that person, down to adding
            // a passkey to their account. Only limited devices can be set up for others.
            if scopes.contains(&Scope::Full) {
                return Err(ApiError::forbidden(
                    "Only the person who uses a device can give it full access. Choose fewer permissions.",
                ));
            }
            super::users::load_user(&state.db, id)
                .await?
                .ok_or_else(|| ApiError::not_found("That person no longer exists."))?
                .id
        }
    };
    let client_kind = body.client_kind.as_deref().unwrap_or("other");
    if !CLIENT_KINDS.contains(&client_kind) {
        return Err(ApiError::bad_request(
            "Device type must be chrome-extension, android or other.",
        ));
    }
    let mut conn = state.db.acquire().await?;
    let created = insert_token(&mut conn, &owner_id, &name, &scopes, client_kind).await?;
    drop(conn);
    state
        .auth
        .audit(
            Some(&user.user_id),
            "token_created",
            serde_json::json!({
                "token_id": created.info.id,
                "for_user": owner_id,
                "name": created.info.name,
                "scopes": created.info.scopes,
            }),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(([super::handlers::NO_STORE], Json(created)))
}

/// `DELETE /api/tokens/:id`: your own device, or anyone's for the owner.
pub async fn delete_token(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    user.require(Scope::Full)?;
    let done = sqlx::query("DELETE FROM api_tokens WHERE id = ? AND (user_id = ? OR ?)")
        .bind(&id)
        .bind(&user.user_id)
        .bind(user.is_owner())
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("That device is already signed out."));
    }
    state
        .auth
        .audit(
            Some(&user.user_id),
            "token_revoked",
            serde_json::json!({"token_id": id}),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn scope_validation() {
        assert_eq!(
            validate_scopes(&s(&["downloads:create"])).unwrap(),
            vec![Scope::DownloadsCreate]
        );
        assert_eq!(
            validate_scopes(&s(&[
                "downloads:create",
                "downloads:read",
                "downloads:create"
            ]))
            .unwrap(),
            vec![Scope::DownloadsCreate, Scope::DownloadsRead]
        );
        assert_eq!(
            validate_scopes(&s(&["library:read", "full"])).unwrap(),
            vec![Scope::Full]
        );
        assert!(validate_scopes(&s(&[])).is_err());
        assert!(validate_scopes(&s(&["admin"])).is_err());
    }

    #[test]
    fn scope_logic() {
        let user = |scopes: Vec<Scope>, role| AuthUser {
            user_id: "u".into(),
            display_name: "Ana".into(),
            role,
            scopes,
            session_id: None,
            token_id: Some("t".into()),
        };
        let ext = user(
            vec![Scope::DownloadsCreate, Scope::DownloadsRead],
            stash_core::models::Role::Owner,
        );
        assert!(ext.has(Scope::DownloadsCreate));
        assert!(!ext.has(Scope::LibraryRead));
        assert!(ext.require(Scope::Full).is_err());
        // An owner's limited device is not an owner session.
        assert!(!ext.is_owner());
        assert!(ext.require_owner().is_err());
        let full = user(vec![Scope::Full], stash_core::models::Role::Owner);
        assert!(full.has(Scope::LibraryRead) && full.is_owner());
        let member = user(vec![Scope::Full], stash_core::models::Role::Member);
        assert!(member.has(Scope::DownloadsCreate));
        assert!(member.require_owner().is_err());
        assert_eq!(
            Scope::to_json(&[Scope::DownloadsCreate, Scope::LibraryRead]),
            r#"["downloads:create","library:read"]"#
        );
        assert_eq!(Scope::parse_json(r#"["full","bogus"]"#), vec![Scope::Full]);
        assert_eq!(
            serde_json::to_value(Scope::DownloadsRead).unwrap(),
            "downloads:read"
        );
    }
}
