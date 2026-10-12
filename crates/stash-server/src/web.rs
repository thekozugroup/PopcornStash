//! The embedded single-page app, PWA files and `/.well-known/assetlinks.json`.

use axum::Router;

use crate::state::AppState;

/// Fallback routes for everything that is not `/api`.
pub fn routes() -> Router<AppState> {
    Router::new()
}
