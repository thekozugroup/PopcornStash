//! Passkey authentication, sessions, device tokens, invites, pairing and the
//! request extractor every protected handler uses (section 7).

pub mod codes;
pub mod handlers;
pub mod pairing;
pub mod passkeys;
pub mod sessions;
#[cfg(test)]
mod tests;
pub mod tokens;
pub mod users;

use std::sync::Arc;

use axum::Router;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::{Deserialize, Serialize};
use stash_core::config::Config;
use stash_core::db::Db;
use stash_core::models::Role;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Token scopes (section 7). `Full` implies every other scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    #[serde(rename = "full")]
    Full,
    #[serde(rename = "downloads:create")]
    DownloadsCreate,
    #[serde(rename = "downloads:read")]
    DownloadsRead,
    #[serde(rename = "library:read")]
    LibraryRead,
}

impl Scope {
    pub const ALL: [Scope; 4] = [
        Scope::Full,
        Scope::DownloadsCreate,
        Scope::DownloadsRead,
        Scope::LibraryRead,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Full => "full",
            Scope::DownloadsCreate => "downloads:create",
            Scope::DownloadsRead => "downloads:read",
            Scope::LibraryRead => "library:read",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }

    /// Scopes from the JSON array stored in `api_tokens.scopes`. Unknown entries are ignored.
    pub fn parse_json(value: &str) -> Vec<Scope> {
        serde_json::from_str::<Vec<String>>(value)
            .unwrap_or_default()
            .iter()
            .filter_map(|s| Self::parse(s))
            .collect()
    }

    /// JSON array text for `api_tokens.scopes`.
    pub fn to_json(scopes: &[Scope]) -> String {
        serde_json::to_string(&scopes.iter().map(|s| s.as_str()).collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".into())
    }
}

/// `User` (section 8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserView {
    pub id: String,
    pub display_name: String,
    pub role: Role,
    pub created_at: i64,
}

/// The public URL without a trailing slash, used to build links.
pub fn public_base(config: &Config) -> Option<String> {
    config
        .public_url
        .as_ref()
        .map(|u| u.as_str().trim_end_matches('/').to_string())
}

/// The authenticated caller. Extracting it rejects unauthenticated requests with 401.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
    pub display_name: String,
    pub role: Role,
    pub scopes: Vec<Scope>,
    /// Set for cookie and bearer sessions.
    pub session_id: Option<String>,
    /// Set for device tokens.
    pub token_id: Option<String>,
}

impl AuthUser {
    pub fn has(&self, scope: Scope) -> bool {
        self.scopes.contains(&Scope::Full) || self.scopes.contains(&scope)
    }

    pub fn require(&self, scope: Scope) -> ApiResult<()> {
        if self.has(scope) {
            Ok(())
        } else {
            Err(ApiError::forbidden(
                "This device is not allowed to do that.",
            ))
        }
    }

    pub fn is_owner(&self) -> bool {
        self.role == Role::Owner && self.has(Scope::Full)
    }

    /// The caller as the library access rule sees them (section 7).
    pub fn viewer(&self) -> stash_core::library::access::Viewer {
        stash_core::library::access::Viewer {
            user_id: self.user_id.clone(),
            role: self.role,
            full: self.scopes.contains(&Scope::Full),
        }
    }

    /// The caller with the library rows that decide what they see.
    pub async fn access(
        &self,
        db: &stash_core::db::Db,
    ) -> ApiResult<stash_core::library::access::Access> {
        Ok(stash_core::library::access::Access::load(db, &self.viewer()).await?)
    }

    pub fn require_owner(&self) -> ApiResult<()> {
        if self.is_owner() {
            Ok(())
        } else {
            Err(ApiError::owner_only())
        }
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        sessions::authenticate(parts, state).await
    }
}

/// Like [`AuthUser`] but never rejects.
#[derive(Debug, Clone)]
pub struct MaybeAuthUser(pub Option<AuthUser>);

impl FromRequestParts<AppState> for MaybeAuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match sessions::authenticate(parts, state).await {
            Ok(user) => Ok(Self(Some(user))),
            Err(e) if e.code == crate::error::ErrorCode::Unauthorized => Ok(Self(None)),
            Err(e) => Err(e),
        }
    }
}

/// Owns the WebAuthn relying party and in-flight ceremony state.
pub struct AuthService {
    pub config: Arc<Config>,
    pub db: Db,
    pub(crate) inner: passkeys::PasskeyState,
}

impl AuthService {
    pub fn new(config: Arc<Config>, db: Db) -> anyhow::Result<Self> {
        let inner = passkeys::PasskeyState::new(&config)?;
        Ok(Self { config, db, inner })
    }

    /// When no users exist, make sure a valid setup code exists and return it for logging.
    pub async fn ensure_setup_code(&self) -> ApiResult<Option<String>> {
        users::ensure_setup_code(self).await
    }

    /// Append to the audit log. Never fails the caller.
    pub async fn audit(
        &self,
        user_id: Option<&str>,
        event: &str,
        detail: serde_json::Value,
        ip: Option<&str>,
    ) {
        users::audit(&self.db, user_id, event, detail, ip).await
    }
}

/// One-time owner recovery link for `popcornstash admin reset-owner`.
pub async fn create_owner_reset_link(db: &Db, config: &Config) -> anyhow::Result<String> {
    users::create_owner_reset_link(db, config).await
}

/// All auth, account, device, people and audit routes.
pub fn routes() -> Router<AppState> {
    handlers::routes()
}
