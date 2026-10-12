//! HTTP handlers for /api/auth, /api/me, /api/tokens, /api/pairing, /api/users, /api/invites, /api/audit.

use axum::Router;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
}
