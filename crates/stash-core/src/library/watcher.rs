//! inotify watcher plus periodic rescans.

use std::sync::Arc;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::Library;

pub fn spawn(lib: Arc<Library>, cancel: CancellationToken) -> JoinHandle<()> {
    let _ = (lib, cancel);
    todo!("library agent: notify watcher, 10s stability check, rescan every 30 min")
}
