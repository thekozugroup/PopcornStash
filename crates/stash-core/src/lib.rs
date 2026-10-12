//! Shared building blocks for PopcornStash: configuration, database, models,
//! the stash folder layout, the library (indexing and sorting), media
//! processing and the background job queue.

pub mod config;
pub mod db;
pub mod error;
pub mod events;
pub mod internal_api;
pub mod jobs;
pub mod library;
pub mod media;
pub mod models;
pub mod paths;
pub mod settings;

pub use error::{CoreError, Result};

/// Current unix time in seconds.
pub fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// A new time-ordered identifier (UUIDv7, hyphenated).
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Application version, shared by every binary role.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
