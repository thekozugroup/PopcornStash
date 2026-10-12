//! Download manager: web links, torrents (via the engine) and finished uploads,
//! all flowing In Progress → Completed → sort (section 5).

pub mod http;
pub mod manager;
pub mod torrents;

pub use manager::{AddDownloadRequest, DownloadFilter, DownloadManager, ForwardedHeaders};
