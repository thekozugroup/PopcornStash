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
    /// A refusal with a sentence for people and, when one request field is at fault,
    /// that field's name as the request spells it (`name`, `folder.name`, `library_id`).
    #[error("{message}")]
    Rejected {
        kind: Rejection,
        field: Option<String>,
        message: String,
    },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T, E = CoreError> = std::result::Result<T, E>;

/// How a [`CoreError::Rejected`] answers: 400, 404 or 409.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    BadRequest,
    NotFound,
    Conflict,
}

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

    /// 400 naming the field at fault.
    pub fn bad_field(field: &str, msg: impl Into<String>) -> Self {
        Self::rejected(Rejection::BadRequest, Some(field), msg)
    }

    /// 409 naming the field at fault.
    pub fn conflict_field(field: &str, msg: impl Into<String>) -> Self {
        Self::rejected(Rejection::Conflict, Some(field), msg)
    }

    /// 404 naming the field at fault.
    pub fn not_found_field(field: &str, msg: impl Into<String>) -> Self {
        Self::rejected(Rejection::NotFound, Some(field), msg)
    }

    pub fn rejected(kind: Rejection, field: Option<&str>, msg: impl Into<String>) -> Self {
        Self::Rejected {
            kind,
            field: field.map(str::to_string),
            message: msg.into(),
        }
    }

    /// The answer for a library that does not exist or that the caller cannot see.
    pub fn library_not_found(field: Option<&str>) -> Self {
        Self::rejected(
            Rejection::NotFound,
            field,
            crate::library::defs::LIBRARY_NOT_FOUND,
        )
    }
}
