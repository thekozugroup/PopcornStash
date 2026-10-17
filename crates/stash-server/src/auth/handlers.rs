//! HTTP handlers for /api/auth, /api/me, /api/tokens, /api/pairing, /api/users, /api/invites, /api/audit.
//!
//! Passkey ceremonies live here; sessions, tokens, pairing and people handlers
//! live in their own modules and are wired up in [`routes`].

use axum::body::Bytes;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Path, Request, State};
use axum::http::StatusCode;
use axum::http::header::{self, HeaderMap};
use axum::response::{AppendHeaders, IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use stash_core::models::Role;
use stash_core::{new_id, now};
use webauthn_rs::prelude::{
    CredentialID, DiscoverableKey, Passkey, PublicKeyCredential, RegisterPublicKeyCredential,
};

use super::passkeys::{self, Flow};
use super::sessions::{self, NewSession, SessionKind};
use super::users::{self, clean_name};
use super::{AuthUser, Scope, UserView, pairing, tokens};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::security::ClientIp;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/state", get(sessions::auth_state))
        .route("/api/auth/activity", post(sessions::activity))
        .route("/api/auth/setup/start", post(setup_start))
        .route("/api/auth/setup/finish", post(setup_finish))
        .route("/api/auth/login/start", post(login_start))
        .route("/api/auth/login/finish", post(login_finish))
        .route("/api/auth/invite/start", post(invite_start))
        .route("/api/auth/invite/finish", post(invite_finish))
        .route("/api/auth/recover/start", post(recover_start))
        .route("/api/auth/recover/finish", post(recover_finish))
        .route("/api/auth/logout", post(sessions::logout))
        .route("/api/me", get(sessions::me))
        .route(
            "/api/me/passkeys",
            get(list_passkeys).post(add_passkey_start),
        )
        .route("/api/me/passkeys/start", post(add_passkey_start))
        .route("/api/me/passkeys/finish", post(add_passkey_finish))
        .route(
            "/api/me/passkeys/{id}",
            patch(rename_passkey).delete(delete_passkey),
        )
        .route("/api/me/sessions", get(sessions::list_sessions))
        .route("/api/me/sessions/{id}", delete(sessions::revoke_session))
        .route(
            "/api/tokens",
            get(tokens::list_tokens).post(tokens::create_token),
        )
        .route("/api/tokens/{id}", delete(tokens::delete_token))
        .route("/api/pairing/request", post(pairing::request))
        .route("/api/pairing/poll", post(pairing::poll))
        .route("/api/pairing/pending", get(pairing::pending))
        .route("/api/pairing/approve", post(pairing::approve))
        .route("/api/pairing/deny", post(pairing::deny))
        .route("/api/users", get(users::list_users))
        .route(
            "/api/users/{id}",
            patch(users::update_user).delete(users::delete_user),
        )
        .route(
            "/api/invites",
            get(users::list_invites).post(users::create_invite),
        )
        .route(
            "/api/invites/{key}",
            get(users::invite_info).delete(users::delete_invite),
        )
        .route("/api/audit", get(users::list_audit))
}

// ───────────────────────────── shared pieces ─────────────────────────────

/// JSON body whose rejection is an [`ApiError`] with a plain sentence.
pub struct JsonBody<T>(pub T);

impl<S, T> FromRequest<S> for JsonBody<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(JsonRejection::MissingJsonContentType(_)) => {
                Err(ApiError::bad_request("Send the request as JSON."))
            }
            Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => Err(
                ApiError::new(ErrorCode::PayloadTooLarge, "That request is too large."),
            ),
            Err(rejection) => {
                tracing::debug!(error = %rejection.body_text(), "rejected request body");
                Err(ApiError::bad_request(
                    "The request was not understood. Check the details and try again.",
                ))
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FlowStarted {
    pub flow_id: String,
    /// `CreationChallengeResponse` or `RequestChallengeResponse`, ready for `navigator.credentials`.
    pub options: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct SignedIn {
    pub user: UserView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// `Passkey` (section 8).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PasskeyView {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

fn user_agent(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
}

fn wrong_flow() -> ApiError {
    ApiError::bad_request("This passkey request does not match. Start again.")
}

/// `Cache-Control: no-store` for responses that carry a secret.
pub const NO_STORE: (header::HeaderName, &str) = (header::CACHE_CONTROL, "no-store");

fn with_cookie(state: &AppState, session: &NewSession, body: impl Serialize) -> Response {
    (
        AppendHeaders([(
            header::SET_COOKIE,
            sessions::session_cookie(&state.config, &session.token),
        )]),
        [NO_STORE],
        Json(body),
    )
        .into_response()
}

async fn audit_failure(
    state: &AppState,
    ip: &ClientIp,
    flow: &str,
    reason: &str,
    user_id: Option<&str>,
) {
    state
        .auth
        .audit(
            user_id,
            "auth_failed",
            serde_json::json!({"flow": flow, "reason": reason}),
            ip.as_string().as_deref(),
        )
        .await;
}

/// Store a verified passkey for `user_id`.
pub async fn insert_passkey(
    conn: &mut SqliteConnection,
    user_id: &str,
    name: &str,
    passkey: &Passkey,
) -> ApiResult<PasskeyView> {
    let id = new_id();
    let now = now();
    let json = serde_json::to_string(passkey).map_err(|e| {
        tracing::error!(error = %e, "could not encode a passkey");
        ApiError::internal()
    })?;
    let result = sqlx::query(
        "INSERT INTO passkeys (id, user_id, name, credential_id, passkey_json, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(name)
    .bind(passkeys::credential_id_text(passkey.cred_id().as_ref()))
    .bind(json)
    .bind(now)
    .execute(&mut *conn)
    .await;
    match result {
        Ok(_) => Ok(PasskeyView {
            id,
            name: name.to_string(),
            created_at: now,
            last_used_at: None,
        }),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(ApiError::conflict(
            "This passkey is already registered. Use it to sign in.",
        )),
        Err(e) => Err(e.into()),
    }
}

async fn create_owner_or_member(
    conn: &mut SqliteConnection,
    display_name: &str,
    role: Role,
    webauthn_id: &str,
) -> ApiResult<UserView> {
    let user = UserView {
        id: new_id(),
        display_name: display_name.to_string(),
        role,
        created_at: now(),
    };
    sqlx::query("INSERT INTO users (id, display_name, role, webauthn_id, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&user.id)
        .bind(&user.display_name)
        .bind(role.as_str())
        .bind(webauthn_id)
        .bind(user.created_at)
        .execute(&mut *conn)
        .await?;
    Ok(user)
}

// ───────────────────────────── first-run setup ─────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SetupStart {
    pub setup_code: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterFinish {
    pub flow_id: String,
    pub credential: RegisterPublicKeyCredential,
    pub passkey_name: Option<String>,
}

fn already_set_up() -> ApiError {
    ApiError::conflict("PopcornStash already has an owner. Sign in with your passkey instead.")
}

/// `POST /api/auth/setup/start` `{setup_code, display_name}`.
pub async fn setup_start(
    State(state): State<AppState>,
    ip: ClientIp,
    JsonBody(body): JsonBody<SetupStart>,
) -> ApiResult<Json<FlowStarted>> {
    state.rate_limiter.check(&ip.key("auth"))?;
    let pk = &state.auth.inner;
    pk.webauthn()?;
    if users::user_count(&state.db).await? > 0 {
        return Err(already_set_up());
    }
    let display_name = clean_name(&body.display_name, "Name")?;
    let Some(code_hash) = users::valid_setup_code(&state.db, &body.setup_code).await? else {
        audit_failure(&state, &ip, "setup", "wrong_setup_code", None).await;
        return Err(ApiError::forbidden(
            "That setup code is not right. Copy it from the container log, or run popcornstash admin setup-code.",
        ));
    };
    let webauthn_id = passkeys::new_user_handle();
    let (options, reg) =
        pk.start_registration(webauthn_id, &display_name, &display_name, Vec::new())?;
    let flow_id = pk.begin(Flow::Setup {
        reg: Box::new(reg),
        code_hash,
        display_name,
        webauthn_id,
    })?;
    Ok(Json(FlowStarted { flow_id, options }))
}

/// `POST /api/auth/setup/finish` `{flow_id, credential, passkey_name?}` → `{user}` + cookie.
pub async fn setup_finish(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    JsonBody(body): JsonBody<RegisterFinish>,
) -> ApiResult<Response> {
    state.rate_limiter.check(&ip.key("auth"))?;
    let pk = &state.auth.inner;
    let Flow::Setup {
        reg,
        code_hash,
        display_name,
        webauthn_id,
    } = pk.take(&body.flow_id)?
    else {
        return Err(wrong_flow());
    };
    let passkey = match pk.finish_registration(&body.credential, &reg) {
        Ok(p) => p,
        Err(e) => {
            audit_failure(&state, &ip, "setup", "passkey_rejected", None).await;
            return Err(e);
        }
    };
    let name = passkeys::passkey_name(body.passkey_name.as_deref(), user_agent(&headers))?;
    let now = now();
    let mut tx = stash_core::db::begin_write(&state.db).await?;
    // Claim the code first: this takes the write lock, so two finishes cannot both create an owner.
    let claimed = sqlx::query(
        "UPDATE recovery_codes SET used_at = ? WHERE code_hash = ? AND kind = 'setup' AND used_at IS NULL AND expires_at > ?",
    )
    .bind(now)
    .bind(&code_hash)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    if claimed.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(ApiError::forbidden(
            "That setup code has expired. Restart the container to get a new one.",
        ));
    }
    let (existing,): (i64,) = sqlx::query_as("SELECT count(*) FROM users")
        .fetch_one(&mut *tx)
        .await?;
    if existing > 0 {
        tx.rollback().await?;
        return Err(already_set_up());
    }
    let user = create_owner_or_member(
        &mut tx,
        &display_name,
        Role::Owner,
        &webauthn_id.to_string(),
    )
    .await?;
    let passkey_view = insert_passkey(&mut tx, &user.id, &name, &passkey).await?;
    tx.commit().await?;
    let ip_text = ip.as_string();
    let session = sessions::create_session(
        &state.db,
        &user.id,
        SessionKind::Cookie,
        user_agent(&headers),
        ip_text.as_deref(),
    )
    .await?;
    state
        .auth
        .audit(
            Some(&user.id),
            "setup_completed",
            serde_json::json!({"passkey_id": passkey_view.id}),
            ip_text.as_deref(),
        )
        .await;
    tracing::info!("owner account created");
    Ok(with_cookie(
        &state,
        &session,
        SignedIn { user, token: None },
    ))
}

// ───────────────────────────── sign-in ─────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct LoginStart {
    /// Keep `mediation: "conditional"` for autofill-style sign-in.
    #[serde(default)]
    pub conditional: bool,
}

#[derive(Debug, Deserialize)]
pub struct LoginFinish {
    pub flow_id: String,
    pub credential: PublicKeyCredential,
    #[serde(default)]
    pub want_token: bool,
    pub device_name: Option<String>,
}

/// `POST /api/auth/login/start` `{}` → discoverable `RequestChallengeResponse`.
pub async fn login_start(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<FlowStarted>> {
    state.rate_limiter.check(&ip.key("auth"))?;
    let body: LoginStart = if body.iter().all(u8::is_ascii_whitespace) {
        LoginStart::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|_| ApiError::bad_request("The request was not understood."))?
    };
    let pk = &state.auth.inner;
    pk.webauthn()?;
    if users::user_count(&state.db).await? == 0 {
        return Err(ApiError::new(
            ErrorCode::SetupRequired,
            "PopcornStash has no owner yet. Finish setup first.",
        ));
    }
    let (options, auth) = pk.start_login(body.conditional)?;
    let native = !headers.contains_key("origin") && !headers.contains_key("referer");
    let flow_id = pk.begin(Flow::Login {
        auth: Box::new(auth),
        native,
    })?;
    tracing::info!(native, "passkey sign-in started");
    Ok(Json(FlowStarted { flow_id, options }))
}

type PasskeyRow = (String, String);

/// `POST /api/auth/login/finish` `{flow_id, credential, want_token?, device_name?}` → `{user, token?}` + cookie unless `want_token`.
pub async fn login_finish(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    JsonBody(body): JsonBody<LoginFinish>,
) -> ApiResult<Response> {
    state.rate_limiter.check(&ip.key("auth"))?;
    let pk = &state.auth.inner;
    let Flow::Login { auth, .. } = pk.take(&body.flow_id)? else {
        return Err(wrong_flow());
    };
    let (handle, _cred_id) = match pk.identify(&body.credential) {
        Ok(found) => found,
        Err(e) => {
            audit_failure(&state, &ip, "login", "no_user_handle", None).await;
            return Err(e);
        }
    };
    let user_id: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE webauthn_id = ?")
        .bind(handle.to_string())
        .fetch_optional(&state.db)
        .await?;
    let Some((user_id,)) = user_id else {
        audit_failure(&state, &ip, "login", "unknown_user", None).await;
        return Err(passkeys::not_recognized());
    };
    let rows: Vec<PasskeyRow> =
        sqlx::query_as("SELECT id, passkey_json FROM passkeys WHERE user_id = ?")
            .bind(&user_id)
            .fetch_all(&state.db)
            .await?;
    let mut keys: Vec<(String, Passkey)> = Vec::with_capacity(rows.len());
    for (id, json) in rows {
        match serde_json::from_str::<Passkey>(&json) {
            Ok(key) => keys.push((id, key)),
            Err(e) => {
                tracing::warn!(passkey_id = %id, error = %e, "skipping an unreadable passkey")
            }
        }
    }
    let discoverable: Vec<DiscoverableKey> =
        keys.iter().map(|(_, k)| DiscoverableKey::from(k)).collect();
    let result = match pk.finish_login(&body.credential, *auth, &discoverable) {
        Ok(r) => r,
        Err(e) => {
            audit_failure(&state, &ip, "login", "passkey_rejected", Some(&user_id)).await;
            return Err(e);
        }
    };
    let now = now();
    let mut used_passkey = None;
    for (id, mut key) in keys {
        let Some(changed) = key.update_credential(&result) else {
            continue;
        };
        if changed {
            let json = serde_json::to_string(&key).map_err(|e| {
                tracing::error!(error = %e, "could not encode a passkey");
                ApiError::internal()
            })?;
            sqlx::query("UPDATE passkeys SET passkey_json = ?, last_used_at = ? WHERE id = ?")
                .bind(json)
                .bind(now)
                .bind(&id)
                .execute(&state.db)
                .await?;
        } else {
            sqlx::query("UPDATE passkeys SET last_used_at = ? WHERE id = ?")
                .bind(now)
                .bind(&id)
                .execute(&state.db)
                .await?;
        }
        used_passkey = Some(id);
        break;
    }
    let user = users::load_user(&state.db, &user_id)
        .await?
        .ok_or_else(passkeys::not_recognized)?;
    let ip_text = ip.as_string();
    let detail = serde_json::json!({"passkey_id": used_passkey, "token": body.want_token});
    if body.want_token {
        let device = match body
            .device_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(name) => Some(clean_name(name, "Device name")?),
            None => user_agent(&headers).map(str::to_string),
        };
        let session = sessions::create_session(
            &state.db,
            &user.id,
            SessionKind::Bearer,
            device.as_deref(),
            ip_text.as_deref(),
        )
        .await?;
        state
            .auth
            .audit(Some(&user.id), "login", detail, ip_text.as_deref())
            .await;
        return Ok((
            [NO_STORE],
            Json(SignedIn {
                user,
                token: Some(session.token),
            }),
        )
            .into_response());
    }
    let session = sessions::create_session(
        &state.db,
        &user.id,
        SessionKind::Cookie,
        user_agent(&headers),
        ip_text.as_deref(),
    )
    .await?;
    state
        .auth
        .audit(Some(&user.id), "login", detail, ip_text.as_deref())
        .await;
    Ok(with_cookie(
        &state,
        &session,
        SignedIn { user, token: None },
    ))
}

// ───────────────────────────── invites ─────────────────────────────

#[derive(Debug, Deserialize)]
pub struct InviteStart {
    pub code: String,
    pub display_name: String,
}

fn invite_gone() -> ApiError {
    ApiError::not_found("This invite link has expired or was already used. Ask for a new one.")
}

/// `POST /api/auth/invite/start` `{code, display_name}`.
pub async fn invite_start(
    State(state): State<AppState>,
    ip: ClientIp,
    JsonBody(body): JsonBody<InviteStart>,
) -> ApiResult<Json<FlowStarted>> {
    state.rate_limiter.check(&ip.key("auth"))?;
    let pk = &state.auth.inner;
    pk.webauthn()?;
    let display_name = clean_name(&body.display_name, "Name")?;
    let Some((invite_id, role, _)) = users::valid_invite(&state.db, &body.code).await? else {
        audit_failure(&state, &ip, "invite", "invalid_invite", None).await;
        return Err(invite_gone());
    };
    let webauthn_id = passkeys::new_user_handle();
    let (options, reg) =
        pk.start_registration(webauthn_id, &display_name, &display_name, Vec::new())?;
    let flow_id = pk.begin(Flow::Invite {
        reg: Box::new(reg),
        invite_id,
        role,
        display_name,
        webauthn_id,
    })?;
    Ok(Json(FlowStarted { flow_id, options }))
}

/// `POST /api/auth/invite/finish` `{flow_id, credential, passkey_name?}` → `{user}` + cookie.
pub async fn invite_finish(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    JsonBody(body): JsonBody<RegisterFinish>,
) -> ApiResult<Response> {
    state.rate_limiter.check(&ip.key("auth"))?;
    let pk = &state.auth.inner;
    let Flow::Invite {
        reg,
        invite_id,
        role,
        display_name,
        webauthn_id,
    } = pk.take(&body.flow_id)?
    else {
        return Err(wrong_flow());
    };
    let passkey = match pk.finish_registration(&body.credential, &reg) {
        Ok(p) => p,
        Err(e) => {
            audit_failure(&state, &ip, "invite", "passkey_rejected", None).await;
            return Err(e);
        }
    };
    let name = passkeys::passkey_name(body.passkey_name.as_deref(), user_agent(&headers))?;
    let now = now();
    let mut tx = stash_core::db::begin_write(&state.db).await?;
    let claimed = sqlx::query(
        "UPDATE invites SET used_at = ? WHERE id = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(now)
    .bind(&invite_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    if claimed.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(invite_gone());
    }
    let user =
        create_owner_or_member(&mut tx, &display_name, role, &webauthn_id.to_string()).await?;
    insert_passkey(&mut tx, &user.id, &name, &passkey).await?;
    sqlx::query("UPDATE invites SET used_by = ? WHERE id = ?")
        .bind(&user.id)
        .bind(&invite_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let ip_text = ip.as_string();
    let session = sessions::create_session(
        &state.db,
        &user.id,
        SessionKind::Cookie,
        user_agent(&headers),
        ip_text.as_deref(),
    )
    .await?;
    state
        .auth
        .audit(
            Some(&user.id),
            "invite_accepted",
            serde_json::json!({"invite_id": invite_id, "role": role.as_str()}),
            ip_text.as_deref(),
        )
        .await;
    Ok(with_cookie(
        &state,
        &session,
        SignedIn { user, token: None },
    ))
}

// ───────────────────────────── owner recovery ─────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RecoverStart {
    pub code: String,
}

fn recovery_gone() -> ApiError {
    ApiError::not_found(
        "This recovery link has expired or was already used. Run popcornstash admin reset-owner for a new one.",
    )
}

/// `POST /api/auth/recover/start` `{code}`: register a new passkey for the first owner.
pub async fn recover_start(
    State(state): State<AppState>,
    ip: ClientIp,
    JsonBody(body): JsonBody<RecoverStart>,
) -> ApiResult<Json<FlowStarted>> {
    state.rate_limiter.check(&ip.key("auth"))?;
    let pk = &state.auth.inner;
    pk.webauthn()?;
    let Some(code_id) = users::valid_reset_code(&state.db, &body.code).await? else {
        audit_failure(&state, &ip, "recover", "invalid_code", None).await;
        return Err(recovery_gone());
    };
    let owner: Option<(String, String, String)> = sqlx::query_as(
        "SELECT id, display_name, webauthn_id FROM users WHERE role = 'owner' ORDER BY created_at, id LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;
    let (user_id, display_name, webauthn_id) = owner.ok_or_else(recovery_gone)?;
    let handle = passkeys::parse_user_handle(&webauthn_id).ok_or_else(|| {
        tracing::error!("the owner's WebAuthn user handle is not a UUID");
        ApiError::internal()
    })?;
    // No exclude list: the point is to replace passkeys that no longer work.
    let (options, reg) = pk.start_registration(handle, &display_name, &display_name, Vec::new())?;
    let flow_id = pk.begin(Flow::Recover {
        reg: Box::new(reg),
        code_id,
        user_id,
    })?;
    Ok(Json(FlowStarted { flow_id, options }))
}

/// `POST /api/auth/recover/finish` `{flow_id, credential, passkey_name?}` → `{user}` + cookie.
pub async fn recover_finish(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    JsonBody(body): JsonBody<RegisterFinish>,
) -> ApiResult<Response> {
    state.rate_limiter.check(&ip.key("auth"))?;
    let pk = &state.auth.inner;
    let Flow::Recover {
        reg,
        code_id,
        user_id,
    } = pk.take(&body.flow_id)?
    else {
        return Err(wrong_flow());
    };
    let passkey = match pk.finish_registration(&body.credential, &reg) {
        Ok(p) => p,
        Err(e) => {
            audit_failure(&state, &ip, "recover", "passkey_rejected", Some(&user_id)).await;
            return Err(e);
        }
    };
    let name = passkeys::passkey_name(body.passkey_name.as_deref(), user_agent(&headers))?;
    let now = now();
    let mut tx = stash_core::db::begin_write(&state.db).await?;
    let claimed = sqlx::query(
        "UPDATE recovery_codes SET used_at = ? WHERE id = ? AND kind = 'owner_reset' AND used_at IS NULL AND expires_at > ?",
    )
    .bind(now)
    .bind(&code_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    if claimed.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(recovery_gone());
    }
    let passkey_view = insert_passkey(&mut tx, &user_id, &name, &passkey).await?;
    tx.commit().await?;
    let user = users::load_user(&state.db, &user_id)
        .await?
        .ok_or_else(recovery_gone)?;
    let ip_text = ip.as_string();
    let session = sessions::create_session(
        &state.db,
        &user.id,
        SessionKind::Cookie,
        user_agent(&headers),
        ip_text.as_deref(),
    )
    .await?;
    state
        .auth
        .audit(
            Some(&user.id),
            "owner_recovered",
            serde_json::json!({"passkey_id": passkey_view.id}),
            ip_text.as_deref(),
        )
        .await;
    Ok(with_cookie(
        &state,
        &session,
        SignedIn { user, token: None },
    ))
}

// ───────────────────────────── my passkeys ─────────────────────────────

/// `GET /api/me/passkeys`.
pub async fn list_passkeys(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<PasskeyView>>> {
    user.require(Scope::Full)?;
    let rows = sqlx::query_as(
        "SELECT id, name, created_at, last_used_at FROM passkeys WHERE user_id = ? ORDER BY created_at, id",
    )
    .bind(&user.user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// `POST /api/me/passkeys/start` (also `POST /api/me/passkeys`): add another passkey.
pub async fn add_passkey_start(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
) -> ApiResult<Json<FlowStarted>> {
    user.require(Scope::Full)?;
    state.rate_limiter.check(&ip.key("auth"))?;
    let pk = &state.auth.inner;
    pk.webauthn()?;
    let (webauthn_id,): (String,) = sqlx::query_as("SELECT webauthn_id FROM users WHERE id = ?")
        .bind(&user.user_id)
        .fetch_one(&state.db)
        .await?;
    let handle = passkeys::parse_user_handle(&webauthn_id).ok_or_else(|| {
        tracing::error!("a WebAuthn user handle is not a UUID");
        ApiError::internal()
    })?;
    let existing: Vec<(String,)> =
        sqlx::query_as("SELECT passkey_json FROM passkeys WHERE user_id = ?")
            .bind(&user.user_id)
            .fetch_all(&state.db)
            .await?;
    let exclude: Vec<CredentialID> = existing
        .iter()
        .filter_map(|(json,)| serde_json::from_str::<Passkey>(json).ok())
        .map(|k| k.cred_id().clone())
        .collect();
    let (options, reg) =
        pk.start_registration(handle, &user.display_name, &user.display_name, exclude)?;
    let flow_id = pk.begin(Flow::Add {
        reg: Box::new(reg),
        user_id: user.user_id.clone(),
    })?;
    Ok(Json(FlowStarted { flow_id, options }))
}

/// `POST /api/me/passkeys/finish` `{flow_id, credential, passkey_name?}` → `Passkey`.
pub async fn add_passkey_finish(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    headers: HeaderMap,
    JsonBody(body): JsonBody<RegisterFinish>,
) -> ApiResult<Json<PasskeyView>> {
    user.require(Scope::Full)?;
    state.rate_limiter.check(&ip.key("auth"))?;
    let pk = &state.auth.inner;
    let Flow::Add { reg, user_id } = pk.take(&body.flow_id)? else {
        return Err(wrong_flow());
    };
    if user_id != user.user_id {
        return Err(wrong_flow());
    }
    let passkey = match pk.finish_registration(&body.credential, &reg) {
        Ok(p) => p,
        Err(e) => {
            audit_failure(
                &state,
                &ip,
                "add_passkey",
                "passkey_rejected",
                Some(&user.user_id),
            )
            .await;
            return Err(e);
        }
    };
    let name = passkeys::passkey_name(body.passkey_name.as_deref(), user_agent(&headers))?;
    let mut conn = state.db.acquire().await?;
    let view = insert_passkey(&mut conn, &user.user_id, &name, &passkey).await?;
    drop(conn);
    state
        .auth
        .audit(
            Some(&user.user_id),
            "passkey_added",
            serde_json::json!({"passkey_id": view.id}),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(Json(view))
}

#[derive(Debug, Deserialize)]
pub struct RenamePasskey {
    pub name: String,
}

fn passkey_gone() -> ApiError {
    ApiError::not_found("That passkey no longer exists.")
}

/// `PATCH /api/me/passkeys/:id` `{name}` → `Passkey`.
pub async fn rename_passkey(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<RenamePasskey>,
) -> ApiResult<Json<PasskeyView>> {
    user.require(Scope::Full)?;
    let name = clean_name(&body.name, "Passkey name")?;
    let row: Option<PasskeyView> = sqlx::query_as(
        "UPDATE passkeys SET name = ? WHERE id = ? AND user_id = ? RETURNING id, name, created_at, last_used_at",
    )
    .bind(&name)
    .bind(&id)
    .bind(&user.user_id)
    .fetch_optional(&state.db)
    .await?;
    row.map(Json).ok_or_else(passkey_gone)
}

/// `DELETE /api/me/passkeys/:id`: refuses to remove the last passkey (409).
pub async fn delete_passkey(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    user.require(Scope::Full)?;
    let done = sqlx::query(
        "DELETE FROM passkeys WHERE id = ? AND user_id = ? AND (SELECT count(*) FROM passkeys WHERE user_id = ?) > 1",
    )
    .bind(&id)
    .bind(&user.user_id)
    .bind(&user.user_id)
    .execute(&state.db)
    .await?;
    if done.rows_affected() == 0 {
        let exists: Option<(String,)> =
            sqlx::query_as("SELECT id FROM passkeys WHERE id = ? AND user_id = ?")
                .bind(&id)
                .bind(&user.user_id)
                .fetch_optional(&state.db)
                .await?;
        return Err(match exists {
            Some(_) => {
                ApiError::conflict("This is your only passkey. Add another one before removing it.")
            }
            None => passkey_gone(),
        });
    }
    state
        .auth
        .audit(
            Some(&user.user_id),
            "passkey_removed",
            serde_json::json!({"passkey_id": id}),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}
