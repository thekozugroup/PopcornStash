//! Opt-in public tracker feed, fetched by the sandboxed engine through its VPN.
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use url::Url;

pub const SOURCE: &str =
    "https://raw.githubusercontent.com/ngosang/trackerslist/master/trackers_best.txt";
const CACHE: &str = "public-trackers.json";
const MAX_BYTES: usize = 64 * 1024;
const MAX_TRACKERS: usize = 20;
const REFRESH_SECS: i64 = 24 * 60 * 60;
const RETRY: Duration = Duration::from_secs(60 * 60);

#[derive(Default, Serialize, Deserialize)]
struct CachedList {
    updated_at: i64,
    trackers: Vec<String>,
}

#[derive(Default)]
pub struct PublicTrackers {
    cache: CachedList,
    last_attempt: Option<Instant>,
    fetching: bool,
}

impl PublicTrackers {
    pub fn load(dir: &Path) -> Self {
        let cache = std::fs::read(dir.join(CACHE))
            .ok()
            .filter(|bytes| bytes.len() <= MAX_BYTES)
            .and_then(|bytes| serde_json::from_slice::<CachedList>(&bytes).ok())
            .and_then(|cache| {
                let trackers = parse(&cache.trackers.join("\n")).ok()?;
                Some(CachedList { trackers, ..cache })
            })
            .unwrap_or_default();
        Self {
            cache,
            ..Self::default()
        }
    }

    pub fn trackers(&self) -> HashSet<Url> {
        self.cache
            .trackers
            .iter()
            .filter_map(|s| Url::parse(s).ok())
            .collect()
    }

    /// Reserve one fetch; failed requests wait an hour and retain the last good list.
    pub fn begin_refresh(&mut self, now: i64) -> bool {
        if self.fetching
            || (self.cache.updated_at <= now
                && now.saturating_sub(self.cache.updated_at) < REFRESH_SECS)
            || self.last_attempt.is_some_and(|at| at.elapsed() < RETRY)
        {
            return false;
        }
        self.fetching = true;
        self.last_attempt = Some(Instant::now());
        true
    }

    pub fn finish_refresh(&mut self, dir: &Path, result: anyhow::Result<Vec<String>>) -> bool {
        self.fetching = false;
        let Ok(trackers) = result else {
            tracing::warn!("public tracker list unavailable; keeping the previous list");
            return false;
        };
        let changed = self.cache.trackers != trackers;
        self.cache = CachedList {
            updated_at: crate::sys::now_unix(),
            trackers,
        };
        if let Ok(bytes) = serde_json::to_vec(&self.cache)
            && let Err(error) = crate::sys::write_atomic(&dir.join(CACHE), &bytes, 0o600)
        {
            tracing::warn!(%error, "could not save the public tracker list");
        }
        tracing::info!(
            count = self.cache.trackers.len(),
            "public tracker list refreshed"
        );
        changed
    }
}

fn parse(text: &str) -> anyhow::Result<Vec<String>> {
    anyhow::ensure!(text.len() <= MAX_BYTES, "tracker list too large");
    let mut seen = HashSet::new();
    let trackers: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.len() > 2048 {
                return None;
            }
            let url = Url::parse(line).ok()?;
            if !matches!(url.scheme(), "udp" | "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return None;
            }
            let normalized = url.to_string();
            seen.insert(normalized.clone()).then_some(normalized)
        })
        .take(MAX_TRACKERS)
        .collect();
    anyhow::ensure!(!trackers.is_empty(), "tracker list has no usable entries");
    Ok(trackers)
}

pub async fn fetch(interface: Option<&str>, source: &str) -> anyhow::Result<Vec<String>> {
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("PopcornStash/", env!("CARGO_PKG_VERSION")));
    if let Some(interface) = interface {
        builder = builder.interface(interface);
    }
    let mut response = builder
        .build()?
        .get(source)
        .send()
        .await?
        .error_for_status()?;
    anyhow::ensure!(response.status().is_success(), "tracker list redirected");
    if let Some(size) = response.content_length() {
        anyhow::ensure!(size <= MAX_BYTES as u64, "tracker list too large");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len().saturating_add(chunk.len()) <= MAX_BYTES,
            "tracker list too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    parse(std::str::from_utf8(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_is_validated_deduplicated_and_bounded() {
        let list = parse("\nhttps://tracker.example/announce\nhttps://tracker.example/announce\nftp://bad.example/x\nhttps://user:pass@bad.example/announce\n<html>unavailable</html>\nudp://udp.example:80/announce\n").unwrap();
        assert_eq!(
            list,
            [
                "https://tracker.example/announce",
                "udp://udp.example:80/announce"
            ]
        );
        assert!(parse("<html>unavailable</html>").is_err());
        assert!(parse(&"x".repeat(MAX_BYTES + 1)).is_err());
        assert_eq!(
            parse(
                &(0..50)
                    .map(|i| format!("https://t{i}.example/announce\n"))
                    .collect::<String>()
            )
            .unwrap()
            .len(),
            20
        );
    }

    #[test]
    fn failed_refresh_preserves_cache_across_restart_and_throttles_retry() {
        let dir = tempfile::tempdir().unwrap();
        let mut feed = PublicTrackers::default();
        assert!(feed.begin_refresh(crate::sys::now_unix()));
        assert!(!feed.begin_refresh(crate::sys::now_unix()));
        assert!(feed.finish_refresh(dir.path(), parse("https://tracker.example/announce")));
        assert!(!feed.finish_refresh(dir.path(), Err(anyhow::anyhow!("offline"))));
        assert_eq!(feed.trackers().len(), 1);
        assert!(!feed.begin_refresh(crate::sys::now_unix() + REFRESH_SECS + 1));
        let restored = PublicTrackers::load(dir.path());
        assert_eq!(restored.trackers(), feed.trackers());
    }

    #[tokio::test]
    async fn http_errors_redirects_and_oversized_bodies_never_replace_the_list() {
        use axum::{Router, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route(
                "/ok",
                get(|| async { "udp://tracker.example:80/announce\n" }),
            )
            .route(
                "/bad",
                get(|| async { (axum::http::StatusCode::SERVICE_UNAVAILABLE, "offline") }),
            )
            .route(
                "/redirect",
                get(|| async { axum::response::Redirect::temporary("/ok") }),
            )
            .route("/large", get(|| async { "x".repeat(MAX_BYTES + 1) }));
        let task = tokio::spawn(async {
            axum::serve(listener, app).await.unwrap();
        });
        assert_eq!(fetch(None, &format!("{base}/ok")).await.unwrap().len(), 1);
        for route in ["bad", "redirect", "large"] {
            assert!(fetch(None, &format!("{base}/{route}")).await.is_err());
        }
        assert!(
            fetch(Some("no-such-pst-if"), &format!("{base}/ok"))
                .await
                .is_err()
        );
        task.abort();
    }
}
