//! /api routes: trash (see docs/architecture.md section 8).

use axum::Router;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
}
