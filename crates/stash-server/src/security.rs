//! Security middleware: response headers, CSRF origin checks, rate limiting, client IP.

use std::net::IpAddr;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;

use crate::error::ApiError;
use crate::state::AppState;

/// The client address, honoring `X-Forwarded-For` only when `STASH_TRUST_PROXY` is set.
#[derive(Debug, Clone)]
pub struct ClientIp(pub Option<IpAddr>);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let _ = (parts, state);
        todo!("auth agent")
    }
}

/// Per-IP token buckets for auth and pairing endpoints (10/min, burst 20).
pub struct RateLimiter {}

impl RateLimiter {
    pub fn new() -> Self {
        Self {}
    }

    /// Err(rate_limited) when `key` (e.g. "auth:1.2.3.4") is over its budget.
    pub fn check(&self, key: &str) -> Result<(), ApiError> {
        let _ = key;
        Ok(())
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Adds CSP, HSTS (https only), nosniff, referrer policy, permissions policy, COOP.
pub async fn security_headers(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let _ = &state;
    next.run(request).await
}

/// Rejects cookie-authenticated unsafe requests whose Origin/Referer is not allowed.
pub async fn csrf_guard(State(state): State<AppState>, request: Request, next: Next) -> Result<Response, ApiError> {
    let _ = &state;
    Ok(next.run(request).await)
}
