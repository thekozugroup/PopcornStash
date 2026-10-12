//! Session cookies and bearer authentication.

use axum::http::request::Parts;

use super::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;

/// Resolve the caller from the session cookie or `Authorization: Bearer`.
pub async fn authenticate(parts: &mut Parts, state: &AppState) -> ApiResult<AuthUser> {
    let _ = (parts, state);
    todo!("auth agent")
}
