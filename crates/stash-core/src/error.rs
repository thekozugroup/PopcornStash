use std::path::PathBuf;

/// Errors raised by the core crate. The server maps these onto API errors.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("path escapes its root: {0}")]
    PathEscape(PathBuf),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    External(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T, E = CoreError> = std::result::Result<T, E>;

impl CoreError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    pub fn external(msg: impl Into<String>) -> Self {
        Self::External(msg.into())
    }
}
