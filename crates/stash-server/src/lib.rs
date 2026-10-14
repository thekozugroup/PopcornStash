//! The PopcornStash web server: API, passkey auth, downloads and the embedded UI.

pub mod api;
pub mod app;
pub mod auth;
pub mod downloads;
pub mod error;
pub mod live;
pub mod nordvpn;
pub mod security;
pub mod state;
pub mod supervisor_client;
pub mod torrentd_client;
pub mod uploads;
pub mod web;

pub use app::{build_state, router, serve};
pub use error::{ApiError, ApiResult};
pub use state::AppState;
