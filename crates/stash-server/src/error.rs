//! API error type: `{"error": {"code", "message"}}` (section 8).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use stash_core::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    RateLimited,
    SetupRequired,
    PasskeysUnavailable,
    VpnUnavailable,
    PayloadTooLarge,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::BadRequest => "bad_request",
            ErrorCode::Unauthorized => "unauthorized",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Conflict => "conflict",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::SetupRequired => "setup_required",
            ErrorCode::PasskeysUnavailable => "passkeys_unavailable",
            ErrorCode::VpnUnavailable => "vpn_unavailable",
            ErrorCode::PayloadTooLarge => "payload_too_large",
            ErrorCode::Internal => "internal",
        }
    }

    pub fn status(self) -> StatusCode {
        match self {
            ErrorCode::BadRequest => StatusCode::BAD_REQUEST,
            ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
            ErrorCode::Forbidden => StatusCode::FORBIDDEN,
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::Conflict => StatusCode::CONFLICT,
            ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::SetupRequired => StatusCode::CONFLICT,
            ErrorCode::PasskeysUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::VpnUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// An error returned to API clients. Messages are plain sentences safe to show users;
/// internal details are logged, never returned.
#[derive(Debug, Clone)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    /// Optional extra fields merged into the error body (e.g. `received` for upload offset conflicts).
    pub extra: Option<serde_json::Value>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), extra: None }
    }
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::BadRequest, message)
    }
    pub fn unauthorized() -> Self {
        Self::new(ErrorCode::Unauthorized, "Sign in to continue.")
    }
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Forbidden, message)
    }
    pub fn owner_only() -> Self {
        Self::forbidden("Only the owner can do that.")
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }
    pub fn rate_limited() -> Self {
        Self::new(ErrorCode::RateLimited, "Too many attempts. Wait a minute and try again.")
    }
    pub fn internal() -> Self {
        Self::new(ErrorCode::Internal, "Something went wrong on the server. Check the container logs.")
    }
    pub fn with_extra(mut self, extra: serde_json::Value) -> Self {
        self.extra = Some(extra);
        self
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut error = serde_json::json!({ "code": self.code.as_str(), "message": self.message });
        if let (Some(serde_json::Value::Object(extra)), serde_json::Value::Object(map)) = (self.extra, &mut error) {
            map.extend(extra);
        }
        (self.code.status(), Json(serde_json::json!({ "error": error }))).into_response()
    }
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::NotFound => ApiError::not_found("That item no longer exists."),
            CoreError::Invalid(msg) => ApiError::bad_request(msg),
            CoreError::Conflict(msg) => ApiError::conflict(msg),
            CoreError::PathEscape(_) => ApiError::bad_request("That path is outside the stash."),
            other => {
                tracing::error!(error = %other, "internal error");
                ApiError::internal()
            }
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        CoreError::from(e).into()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!(error = %format!("{e:#}"), "internal error");
        ApiError::internal()
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        CoreError::from(e).into()
    }
}
