use super::{Library, execute, recover, reserve};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Coordinator {
    claims: Mutex<()>,
    blocked: AtomicBool,
}

pub(super) fn start(lib: Arc<Library>, cancel: CancellationToken) {
    tokio::spawn(async move {
        // Global recovery is exclusively a startup operation. Once workers are
        // running, each can reconcile only its own journal.
        loop {
            if cancel.is_cancelled() {
                return;
            }
            match recover(&lib).await {
                Ok(()) => break,
                Err(error) => {
                    tracing::warn!(%error, "conversion recovery needs attention; no work started")
                }
            }
            tokio::select! { _ = cancel.cancelled() => return, _ = tokio::time::sleep(Duration::from_secs(10)) => {} }
        }
        let coordinator = Coordinator::default();
        // Two total jobs, with at most one CPU encoder. The additional slot is
        // reserved for a codec with a tested hardware encoder.
        tokio::join!(
            worker(&lib, &cancel, &coordinator, false),
            worker(&lib, &cancel, &coordinator, true)
        );
    });
}

async fn worker(
    lib: &Library,
    cancel: &CancellationToken,
    coordinator: &Coordinator,
    gpu_only: bool,
) {
    loop {
        let next = {
            let _claim = coordinator.claims.lock().await;
            if cancel.is_cancelled() || coordinator.blocked.load(Ordering::Acquire) {
                return;
            }
            reserve(lib, gpu_only).await
        };
        let mut worked = false;
        match next {
            Ok(Some((item, policy, job))) => {
                worked = true;
                if let Err(error) = execute(lib, item, policy, job, cancel).await {
                    // Let another already-running job finish safely, but never
                    // start more work around an unreconciled publication.
                    coordinator.blocked.store(true, Ordering::Release);
                    tracing::error!(%error, "conversion cleanup needs attention; new work stopped");
                    return;
                }
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "conversion could not claim work"),
        }
        let wait = if worked { 1 } else { 10 };
        tokio::select! { _ = cancel.cancelled() => return, _ = tokio::time::sleep(Duration::from_secs(wait)) => {} }
    }
}
