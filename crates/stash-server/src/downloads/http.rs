//! Resumable web downloads with reqwest (docs/architecture.md section 5, Web downloads).
//!
//! A worker pool limited by `max_concurrent_downloads` streams each link into
//! `Downloads/In Progress/<name>.part`, resuming with `Range`, then renames it and
//! moves it into `Downloads/Completed/` for the sorter.

use std::error::Error as StdError;
use std::io::SeekFrom;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{self, HeaderMap, HeaderValue};
use stash_core::models::DownloadRow;
use stash_core::now;
use stash_core::paths;
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::manager::{
    DownloadManager, ForwardedHeaders, MAX_TORRENT_FILE, MSG_LINK_EXPIRED, MSG_VPN_PORTS,
    proxy_reaches,
};
use super::source;
use super::ssrf::{self, GuardedResolver};
use crate::error::{ApiError, ApiResult};

/// Attempts per download before it fails (network errors and 5xx).
const MAX_ATTEMPTS: u32 = 3;
const MAX_REDIRECTS: usize = 10;
const MSG_REDIRECTS: &str = "The link redirected too many times.";
const MSG_UNREACHABLE: &str = "The server for that link could not be reached.";
const MSG_WRITE: &str = "The download could not be saved. Check that the stash has free space.";
const MSG_CHANGED: &str =
    "The file on the server changed since the download started. Add the link again.";
const PROGRESS_EVENT_EVERY: Duration = Duration::from_secs(1);
const PROGRESS_DB_EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy)]
struct TooManyRedirects;

impl std::fmt::Display for TooManyRedirects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(MSG_REDIRECTS)
    }
}

impl StdError for TooManyRedirects {}

fn chain_has<T: StdError + 'static>(error: &(dyn StdError + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(e) = current {
        if e.downcast_ref::<T>().is_some() {
            return true;
        }
        current = e.source();
    }
    false
}

/// Spawn the loop that starts queued web downloads.
pub(crate) fn spawn_worker_loop(manager: Arc<DownloadManager>, cancel: CancellationToken) {
    tokio::spawn(async move {
        loop {
            if cancel.is_cancelled() {
                break;
            }
            if let Err(e) = claim_and_spawn(&manager, &cancel).await {
                tracing::warn!(error = %e, "could not start queued web downloads");
            }
            let tick = manager.tuning().worker_tick;
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = manager.http_wake.notified() => {}
                _ = tokio::time::sleep(tick) => {}
            }
        }
        tracing::debug!("web download workers stopped");
    });
}

async fn claim_and_spawn(
    manager: &Arc<DownloadManager>,
    cancel: &CancellationToken,
) -> ApiResult<()> {
    // Re-read every loop so a settings change applies without a restart.
    let max = manager.settings.get().await.max_concurrent_downloads.max(1) as usize;
    let free = max.saturating_sub(manager.running.len());
    if free == 0 {
        return Ok(());
    }
    let queued: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM downloads WHERE kind = 'http' AND state = 'queued' ORDER BY created_at, id LIMIT ?",
    )
    .bind(i64::try_from(free + manager.running.len()).unwrap_or(i64::MAX))
    .fetch_all(&manager.db)
    .await?;
    let mut started = 0;
    for (id,) in queued {
        if started >= free {
            break;
        }
        // A paused-then-resumed download whose old worker is still winding down waits,
        // and so does one whose `queued` event is not out yet.
        if manager.running.contains_key(&id) || manager.adding.contains(&id) {
            continue;
        }
        // Register the stop token before claiming, so a pause that lands in between
        // always finds it.
        let token = cancel.child_token();
        manager.running.insert(id.clone(), token.clone());
        let claimed = sqlx::query(
            "UPDATE downloads SET state = 'downloading', error = NULL, down_bps = 0, eta_s = NULL, updated_at = ?
             WHERE id = ? AND state = 'queued'",
        )
        .bind(now())
        .bind(&id)
        .execute(&manager.db)
        .await
        .map(|r| r.rows_affected());
        let claimed = match claimed {
            Ok(n) => n,
            Err(e) => {
                manager.running.remove(&id);
                return Err(e.into());
            }
        };
        if claimed == 0 {
            manager.running.remove(&id);
            continue;
        }
        started += 1;
        let _ = manager.publish_id(&id).await;
        let manager = manager.clone();
        tokio::spawn(async move {
            run(&manager, &id, &token).await;
            manager.running.remove(&id);
            manager.http_wake.notify_one();
        });
    }
    Ok(())
}

enum Outcome {
    /// The `.part` file is complete.
    Done(DownloadRow),
    /// Paused, removed or shutting down: keep the `.part`, leave the state alone.
    Stopped(Option<DownloadRow>),
    Failed(String),
}

async fn run(manager: &Arc<DownloadManager>, id: &str, token: &CancellationToken) {
    match download(manager, id, token).await {
        Outcome::Done(row) => {
            if let Err(message) = finalize(manager, &row).await {
                fail_if_downloading(manager, id, &message).await;
            }
        }
        Outcome::Stopped(row) => {
            if let Some(row) = row {
                let _ = persist_progress(manager, &row, true).await;
                let _ = manager.publish_id(id).await;
            }
        }
        Outcome::Failed(message) => {
            tracing::info!(download = %id, reason = %message, "web download failed");
            fail_if_downloading(manager, id, &message).await;
        }
    }
}

/// Record a failure unless the user paused or removed the download meanwhile.
async fn fail_if_downloading(manager: &DownloadManager, id: &str, message: &str) {
    let result = sqlx::query(
        "UPDATE downloads SET state = 'failed', error = ?, down_bps = 0, eta_s = NULL, updated_at = ?
         WHERE id = ? AND state = 'downloading'",
    )
    .bind(message)
    .bind(now())
    .bind(id)
    .execute(&manager.db)
    .await;
    if let Err(e) = result {
        tracing::warn!(error = %e, download = %id, "could not record a failed web download");
    }
    let _ = manager.publish_id(id).await;
}

/// Build the client for one download: guarded DNS and redirect checks when direct,
/// the VPN proxy (with redirects checked by hand) otherwise.
pub(crate) fn build_client(
    manager: &DownloadManager,
    proxy: Option<reqwest::Proxy>,
) -> reqwest::Result<reqwest::Client> {
    let guard = manager.guard.clone();
    let proxied = proxy.is_some();
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        if proxied {
            // Through the proxy the target is resolved remotely, so every hop comes
            // back to `download()`, which checks it with a local lookup first.
            return attempt.stop();
        }
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error(TooManyRedirects);
        }
        if !guard.redirect_ok(attempt.url()) {
            return attempt.error(ssrf::BlockedDestination);
        }
        attempt.follow()
    });
    let builder = reqwest::Client::builder()
        .user_agent(concat!("PopcornStash/", env!("CARGO_PKG_VERSION")))
        .redirect(policy)
        .referer(false)
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(120));
    let builder = match proxy {
        Some(proxy) => builder.proxy(proxy),
        None => builder.no_proxy().dns_resolver(Arc::new(GuardedResolver {
            guard: manager.guard.clone(),
        })),
    };
    builder.build()
}

/// Same scheme, host and port: the browser's cookies for one may go to the other.
fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// Redirects through the VPN proxy are followed by hand, so drop the forwarded
/// browser cookies once a hop leaves the origin they came from, as reqwest does for
/// direct downloads. They are not sent again for the rest of that chain.
fn strip_cross_origin(headers: &mut HeaderMap, from: &Url, to: &Url) {
    if !same_origin(from, to) {
        headers.remove(header::COOKIE);
    }
}

fn request_headers(headers: &ForwardedHeaders) -> HeaderMap {
    let mut map = HeaderMap::new();
    let mut put = |name: header::HeaderName, value: &Option<String>| {
        if let Some(Ok(mut value)) = value.as_deref().map(HeaderValue::from_str) {
            value.set_sensitive(true);
            map.insert(name, value);
        }
    };
    put(header::COOKIE, &headers.cookie);
    put(header::REFERER, &headers.referer);
    put(header::USER_AGENT, &headers.user_agent);
    map
}

/// What one request/stream attempt ended with.
enum Step {
    Done,
    Redirect(Url),
    Stopped,
    Fatal(String),
    Retry {
        message: String,
        progressed: bool,
    },
    /// The `.part` was unusable and has been truncated; try again from zero.
    Restart,
}

fn classify_error(error: &reqwest::Error) -> Step {
    if ssrf::is_blocked_error(error) {
        return Step::Fatal(ssrf::MSG_BLOCKED.to_string());
    }
    if chain_has::<TooManyRedirects>(error) || error.is_redirect() {
        return Step::Fatal(MSG_REDIRECTS.to_string());
    }
    if error.is_builder() {
        return Step::Fatal("That link is not valid.".to_string());
    }
    Step::Retry {
        message: MSG_UNREACHABLE.to_string(),
        progressed: false,
    }
}

fn status_message(status: StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => format!(
            "The server refused the download ({}). The link may have expired or need a sign-in.",
            status.as_u16()
        ),
        404 | 410 => format!(
            "The file is not on the server any more ({}).",
            status.as_u16()
        ),
        code => format!("The server refused the download ({code})."),
    }
}

/// Parse `Content-Range: bytes start-end/total` (or `bytes */total`).
fn content_range(headers: &HeaderMap) -> Option<(Option<u64>, Option<u64>)> {
    let value = headers.get(header::CONTENT_RANGE)?.to_str().ok()?.trim();
    let rest = value.strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let total = total.trim().parse::<u64>().ok();
    let start = match range.trim() {
        "*" => None,
        r => Some(r.split_once('-')?.0.trim().parse::<u64>().ok()?),
    };
    Some((start, total))
}

/// The file name from `Content-Disposition` (RFC 6266; `filename*` wins over `filename`).
pub fn content_disposition_filename(value: &str) -> Option<String> {
    let mut plain = None;
    let mut extended = None;
    for (key, value) in disposition_params(value) {
        match key.as_str() {
            "filename*" if extended.is_none() => extended = decode_ext_value(&value),
            "filename" if plain.is_none() => plain = Some(value),
            _ => {}
        }
    }
    extended
        .or(plain)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn disposition_params(value: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = value.chars().peekable();
    // Skip the disposition type.
    for c in chars.by_ref() {
        if c == ';' {
            break;
        }
    }
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace() || *c == ';') {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c == ';' {
                break;
            }
            key.push(c);
            chars.next();
        }
        if chars.peek() != Some(&'=') {
            continue;
        }
        chars.next();
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let mut val = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(next) = chars.next() {
                            val.push(next);
                        }
                    }
                    '"' => break,
                    c => val.push(c),
                }
            }
            while chars.peek().is_some_and(|c| *c != ';') {
                chars.next();
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ';' {
                    break;
                }
                val.push(c);
                chars.next();
            }
            val = val.trim().to_string();
        }
        out.push((key.trim().to_ascii_lowercase(), val));
    }
    out
}

/// RFC 8187 `charset'language'percent-encoded`.
fn decode_ext_value(value: &str) -> Option<String> {
    let mut parts = value.splitn(3, '\'');
    let charset = parts.next()?.trim().to_ascii_lowercase();
    let _language = parts.next()?;
    let encoded = parts.next()?;
    let bytes: Vec<u8> = percent_encoding::percent_decode_str(encoded).collect();
    match charset.as_str() {
        "iso-8859-1" | "latin1" => Some(bytes.iter().map(|&b| char::from(b)).collect()),
        _ => Some(String::from_utf8_lossy(&bytes).into_owned()),
    }
}

/// Name for a fresh download from the first response.
fn choose_name(headers: &HeaderMap, final_url: &Url, original: &Url) -> String {
    let from_header = headers
        .get(header::CONTENT_DISPOSITION)
        .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
        .and_then(|v| content_disposition_filename(&v));
    let raw = from_header
        .or_else(|| source::name_from_url(final_url))
        .or_else(|| source::name_from_url(original))
        .unwrap_or_else(|| "download".to_string());
    let mut name = paths::sanitize_filename(&raw);
    let has_extension = name.rfind('.').is_some_and(|i| i > 0 && i + 1 < name.len());
    if !has_extension {
        let mime = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if let Some(ext) = source::extension_for_mime(mime) {
            name = format!("{name}.{ext}");
        }
    }
    name
}

/// A `.part` file this server wrote: a regular file owned by this user. `In Progress`
/// is writable by the torrent engine, which could put a link or its own file (one it
/// can read) at the name; neither is ever resumed, written to or moved on.
fn own_part(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.is_file() && meta.uid() == nix::unistd::geteuid().as_raw()
}

/// Bytes already in the `.part` file (0 unless it is [`own_part`]).
async fn part_len(manager: &DownloadManager, row: &DownloadRow) -> u64 {
    let Some(rel) = row.rel_path.as_deref() else {
        return 0;
    };
    let Ok(abs) = manager.layout.abs(rel) else {
        return 0;
    };
    match tokio::fs::symlink_metadata(abs).await {
        Ok(meta) if own_part(&meta) => meta.len(),
        _ => 0,
    }
}

/// Open the `.part` file for writing: this server's own file (made private, 0600), or a
/// new one. Anything else at the name (a link, or a file the torrent engine put there)
/// is removed first and never opened; `O_NOFOLLOW` and `O_EXCL` keep a link planted in
/// between from redirecting the write.
async fn open_part(path: &std::path::Path) -> std::io::Result<tokio::fs::File> {
    use std::os::unix::fs::PermissionsExt;
    let flags = nix::fcntl::OFlag::O_NOFOLLOW.bits();
    match tokio::fs::OpenOptions::new()
        .write(true)
        .custom_flags(flags | nix::fcntl::OFlag::O_NONBLOCK.bits())
        .open(path)
        .await
    {
        Ok(file) => {
            let meta = file.metadata().await?;
            if own_part(&meta) {
                file.set_permissions(std::fs::Permissions::from_mode(0o600))
                    .await?;
                return Ok(file);
            }
            drop(file);
            tracing::warn!(path = %path.display(), "replacing a partial download this server did not write");
            remove_entry(path).await?;
        }
        // ELOOP: a symbolic link sits at the name.
        Err(e) if e.raw_os_error() == Some(nix::libc::ELOOP) => {
            tracing::warn!(path = %path.display(), "replacing a link at a partial download's name");
            remove_entry(path).await?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // A directory (EISDIR) or a FIFO without a reader (ENXIO) at the name.
        Err(e)
            if matches!(
                e.raw_os_error(),
                Some(nix::libc::EISDIR) | Some(nix::libc::ENXIO)
            ) =>
        {
            tracing::warn!(path = %path.display(), "replacing something that is not a file at a partial download's name");
            remove_entry(path).await?;
        }
        Err(e) => return Err(e),
    }
    tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(flags)
        .open(path)
        .await
}

/// Remove whatever is at `path` (a file, link or folder), without following links.
async fn remove_entry(path: &std::path::Path) -> std::io::Result<()> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(meta) if meta.is_dir() => tokio::fs::remove_dir_all(path).await,
        Ok(_) => tokio::fs::remove_file(path).await,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

async fn truncate_part(manager: &DownloadManager, row: &DownloadRow) {
    if let Some(Ok(abs)) = row.rel_path.as_deref().map(|r| manager.layout.abs(r)) {
        let _ = tokio::fs::remove_file(abs).await;
    }
}

async fn persist_progress(
    manager: &DownloadManager,
    row: &DownloadRow,
    stopped: bool,
) -> ApiResult<()> {
    let (down_bps, eta) = if stopped {
        (0, None)
    } else {
        (row.down_bps, row.eta_s)
    };
    sqlx::query(
        "UPDATE downloads SET name = ?, rel_path = ?, done_bytes = ?, total_bytes = ?, progress = ?, down_bps = ?,
                eta_s = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&row.name)
    .bind(&row.rel_path)
    .bind(row.done_bytes)
    .bind(row.total_bytes)
    .bind(row.progress)
    .bind(down_bps)
    .bind(eta)
    .bind(now())
    .bind(&row.id)
    .execute(&manager.db)
    .await?;
    Ok(())
}

async fn download(manager: &Arc<DownloadManager>, id: &str, token: &CancellationToken) -> Outcome {
    let Ok(mut row) = manager.load_row(id).await else {
        return Outcome::Stopped(None);
    };
    let secrets = manager.secrets.get(id).map(|s| s.clone());
    // The stored link lost its private parts; fetching it would fetch something else.
    if secrets.is_none() && source::is_redacted_source(&row.source) {
        return Outcome::Failed(MSG_LINK_EXPIRED.into());
    }
    let original = match secrets
        .as_ref()
        .map(|s| s.url.clone())
        .or_else(|| Url::parse(&row.source).ok())
    {
        Some(url) => url,
        None => return Outcome::Failed("The link for this download is not valid.".into()),
    };
    let headers = request_headers(&secrets.map(|s| s.headers).unwrap_or_default());
    let proxy = match manager.choose_proxy(&original).await {
        Ok(proxy) => proxy,
        Err(message) => return Outcome::Failed(message),
    };
    let proxied = proxy.is_some();
    let client = match build_client(manager, proxy) {
        Ok(client) => client,
        Err(e) => {
            tracing::error!(error = %e.without_url(), "could not build the download client");
            return Outcome::Failed("The download could not start. Check the server logs.".into());
        }
    };

    let mut failures = 0u32;
    let mut current = original.clone();
    let mut hops = 0usize;
    // What this hop sends; redirects to another origin lose the cookies.
    let mut hop_headers = headers.clone();
    loop {
        if token.is_cancelled() {
            return Outcome::Stopped(Some(row));
        }
        let offset = part_len(manager, &row).await;
        let step = match manager.guard.check_url(&current).await {
            Ok(()) => {
                attempt(
                    manager,
                    &client,
                    &mut row,
                    &current,
                    &original,
                    &hop_headers,
                    offset,
                    token,
                    proxied,
                )
                .await
            }
            // DNS can fail for a moment; try again like any network error.
            Err(ssrf::GuardError::Unresolved) => Step::Retry {
                message: ssrf::MSG_UNRESOLVED.to_string(),
                progressed: false,
            },
            Err(e) => Step::Fatal(e.message().to_string()),
        };
        match step {
            Step::Done => return Outcome::Done(row),
            Step::Stopped => return Outcome::Stopped(Some(row)),
            Step::Fatal(message) => return Outcome::Failed(message),
            Step::Redirect(next) => {
                hops += 1;
                if hops > MAX_REDIRECTS {
                    return Outcome::Failed(MSG_REDIRECTS.into());
                }
                strip_cross_origin(&mut hop_headers, &current, &next);
                current = next;
                continue;
            }
            Step::Restart => {
                failures += 1;
                if failures >= MAX_ATTEMPTS {
                    return Outcome::Failed(
                        "The server keeps sending a different file. Try the link again later."
                            .into(),
                    );
                }
            }
            Step::Retry {
                message,
                progressed,
            } => {
                if progressed {
                    failures = 0;
                }
                failures += 1;
                if failures >= MAX_ATTEMPTS {
                    return Outcome::Failed(message);
                }
                tracing::info!(download = %id, attempt = failures, reason = %message, "retrying web download");
            }
        }
        // Retries start again from the original link (signed redirects expire).
        current = original.clone();
        hop_headers = headers.clone();
        hops = 0;
        let delay = manager.tuning().retry_delay * 2u32.pow(failures.saturating_sub(1));
        tokio::select! {
            _ = token.cancelled() => return Outcome::Stopped(Some(row)),
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn attempt(
    manager: &DownloadManager,
    client: &reqwest::Client,
    row: &mut DownloadRow,
    current: &Url,
    original: &Url,
    headers: &HeaderMap,
    offset: u64,
    token: &CancellationToken,
    proxied: bool,
) -> Step {
    let mut request = client.get(current.clone()).headers(headers.clone());
    if offset > 0 {
        request = request.header(header::RANGE, format!("bytes={offset}-"));
    }
    let response = tokio::select! {
        _ = token.cancelled() => return Step::Stopped,
        r = request.send() => r,
    };
    let response = match response {
        Ok(r) => r,
        Err(e) => return classify_error(&e),
    };
    let status = response.status();

    if status.is_redirection() && proxied {
        let next = response
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|loc| current.join(loc).ok());
        return match next {
            // The VPN proxy refuses other ports with an answer that would read like the
            // server's own refusal.
            Some(next) if !proxy_reaches(&next) => Step::Fatal(MSG_VPN_PORTS.into()),
            Some(next) => Step::Redirect(next),
            None => Step::Fatal("The link redirected to an address that is not valid.".into()),
        };
    }
    if status == StatusCode::RANGE_NOT_SATISFIABLE && offset > 0 {
        let total = content_range(response.headers()).and_then(|(_, total)| total);
        if total.is_none_or(|t| t == offset) {
            // The `.part` already holds the whole file.
            row.done_bytes = offset as i64;
            row.total_bytes = Some(offset as i64);
            row.progress = 1.0;
            return Step::Done;
        }
        truncate_part(manager, row).await;
        return Step::Restart;
    }
    if status == StatusCode::REQUEST_TIMEOUT || status == StatusCode::TOO_MANY_REQUESTS {
        return Step::Retry {
            message: status_message(status),
            progressed: false,
        };
    }
    if status.is_client_error() {
        return Step::Fatal(status_message(status));
    }
    if status.is_server_error() {
        return Step::Retry {
            message: format!("The server had a problem ({}).", status.as_u16()),
            progressed: false,
        };
    }
    if !status.is_success() || status == StatusCode::NO_CONTENT {
        return Step::Fatal(format!(
            "The server sent an unexpected answer ({}).",
            status.as_u16()
        ));
    }

    let (start, total) = if status == StatusCode::PARTIAL_CONTENT {
        match content_range(response.headers()) {
            Some((Some(start), total)) if start == offset => (offset, total),
            _ => {
                truncate_part(manager, row).await;
                return Step::Restart;
            }
        }
    } else {
        // 200: the server ignored the Range (or none was sent); start over.
        (0, response.content_length())
    };
    // Resuming, but the server now describes a different file (a sign-in page after a
    // restart dropped the cookie, or a replaced file), or no longer says how big it is:
    // stop rather than overwrite the bytes already here.
    if offset > 0
        && let Some(before) = row.total_bytes
        && before > 0
        && total.and_then(|t| i64::try_from(t).ok()) != Some(before)
    {
        return Step::Fatal(MSG_CHANGED.into());
    }

    if row.rel_path.is_none() {
        let wanted = choose_name(response.headers(), response.url(), original);
        let name = manager.reserve_name(&wanted);
        row.rel_path = Some(format!("{}/{}.part", paths::IN_PROGRESS, name));
        row.name = name;
    }
    row.total_bytes = total.map(|t| t as i64);
    row.done_bytes = start as i64;
    row.progress = progress(row);
    if let Err(e) = persist_progress(manager, row, false).await {
        tracing::warn!(error = %e, "could not record download progress");
    }
    manager.publish_row(row);

    stream_to_part(manager, response, row, start, total, token).await
}

fn progress(row: &DownloadRow) -> f64 {
    match row.total_bytes {
        Some(total) if total > 0 => (row.done_bytes as f64 / total as f64).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

async fn stream_to_part(
    manager: &DownloadManager,
    response: reqwest::Response,
    row: &mut DownloadRow,
    start: u64,
    total: Option<u64>,
    token: &CancellationToken,
) -> Step {
    let Some(Ok(path)) = row.rel_path.as_deref().map(|r| manager.layout.abs(r)) else {
        return Step::Fatal(MSG_WRITE.into());
    };
    let opened = async {
        let mut file = open_part(&path).await?;
        if file.metadata().await?.len() < start {
            // The bytes this resume continues from are gone (the file was replaced or
            // removed meanwhile): start over rather than leave a hole.
            return Ok(None);
        }
        file.set_len(start).await?;
        file.seek(SeekFrom::Start(start)).await?;
        std::io::Result::Ok(Some(file))
    }
    .await;
    let file = match opened {
        Ok(Some(file)) => file,
        Ok(None) => {
            tracing::warn!(path = %path.display(), "the partial download lost its bytes; starting over");
            truncate_part(manager, row).await;
            return Step::Restart;
        }
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "could not open the partial download");
            return Step::Fatal(MSG_WRITE.into());
        }
    };
    let mut writer = BufWriter::with_capacity(1 << 20, file);
    let mut stream = response.bytes_stream();
    let mut done = start;
    let mut last_event = Instant::now();
    let mut last_db = Instant::now();
    let mut sample_at = Instant::now();
    let mut sample_bytes = done;

    let ended = loop {
        let next = tokio::select! {
            biased;
            _ = token.cancelled() => break Step::Stopped,
            next = stream.next() => next,
        };
        match next {
            Some(Ok(bytes)) => {
                if let Err(e) = writer.write_all(&bytes).await {
                    tracing::warn!(error = %e, path = %path.display(), "could not write the partial download");
                    break Step::Fatal(MSG_WRITE.into());
                }
                done += bytes.len() as u64;
                row.done_bytes = done as i64;
                row.progress = progress(row);
                let elapsed = sample_at.elapsed();
                if elapsed >= PROGRESS_EVENT_EVERY {
                    let bps = ((done - sample_bytes) as f64 / elapsed.as_secs_f64()) as i64;
                    row.down_bps = bps;
                    row.eta_s = match (row.total_bytes, bps) {
                        (Some(total), bps) if bps > 0 => {
                            Some((total - row.done_bytes).max(0) / bps)
                        }
                        _ => None,
                    };
                    sample_at = Instant::now();
                    sample_bytes = done;
                }
                if last_event.elapsed() >= PROGRESS_EVENT_EVERY {
                    manager.publish_row(row);
                    last_event = Instant::now();
                }
                if last_db.elapsed() >= PROGRESS_DB_EVERY {
                    if let Err(e) = persist_progress(manager, row, false).await {
                        tracing::warn!(error = %e, "could not record download progress");
                    }
                    last_db = Instant::now();
                }
            }
            Some(Err(_)) => {
                break Step::Retry {
                    message: "The connection dropped.".into(),
                    progressed: done > start,
                };
            }
            None => break Step::Done,
        }
    };

    let flushed = async {
        writer.flush().await?;
        writer.get_mut().sync_all().await
    }
    .await;
    if let Err(e) = flushed {
        tracing::warn!(error = %e, path = %path.display(), "could not flush the partial download");
        return Step::Fatal(MSG_WRITE.into());
    }
    match ended {
        Step::Done => match total {
            Some(total) if done < total => Step::Retry {
                message: "The connection closed before the download finished.".into(),
                progressed: done > start,
            },
            _ => {
                row.total_bytes = Some(done as i64);
                row.done_bytes = done as i64;
                row.progress = 1.0;
                Step::Done
            }
        },
        other => other,
    }
}

/// `.part` → `Completed/<name>` in one rename → sort. There is no intermediate name in
/// In Progress, so a restart finds either the `.part` (and resumes) or the finished file.
async fn finalize(manager: &DownloadManager, row: &DownloadRow) -> Result<(), String> {
    const MSG_MOVE: &str = "The finished download could not be moved to Downloads/Completed.";
    let part_rel = row
        .rel_path
        .as_deref()
        .ok_or_else(|| MSG_MOVE.to_string())?;
    let part = manager
        .layout
        .abs(part_rel)
        .map_err(|_| MSG_MOVE.to_string())?;
    match tokio::fs::symlink_metadata(&part).await {
        Ok(meta) if own_part(&meta) => {}
        _ => {
            tracing::warn!(download = %row.id, "the finished partial download was replaced; not moving it");
            return Err(MSG_MOVE.to_string());
        }
    }
    let moved = manager.move_to_completed(&part, &row.name).await;
    if moved.is_ok() {
        manager.release_name(&row.name);
    }
    let (dest, rel) = moved.map_err(|e| {
        tracing::warn!(error = %e, download = %row.id, "could not move the finished download");
        MSG_MOVE.to_string()
    })?;
    // Checked again where nobody else can write: something swapped in between the
    // check above and the move is removed instead of being sorted into the library.
    match tokio::fs::symlink_metadata(&dest).await {
        Ok(meta) if own_part(&meta) => {}
        _ => {
            tracing::warn!(download = %row.id, "the finished partial download was replaced; removing it");
            let _ = remove_entry(&dest).await;
            return Err(MSG_MOVE.to_string());
        }
    }
    manager.secrets.remove(&row.id);
    tracing::info!(download = %row.id, rel = %rel, "web download finished");
    manager
        .complete_and_sort(&row.id, &rel, Some(row.done_bytes))
        .await
        .map_err(|e| e.message)?;
    Ok(())
}

impl DownloadManager {
    /// Fetch a `.torrent` link (at most 10 MB) with the same destination checks as
    /// web downloads.
    pub(crate) async fn fetch_torrent_file(
        &self,
        url: &Url,
        headers: ForwardedHeaders,
    ) -> ApiResult<Vec<u8>> {
        // A server that trickles bytes must not hold the request open for long.
        let limit = self.tuning().torrent_fetch_timeout;
        tokio::time::timeout(limit, self.fetch_torrent_file_inner(url, headers))
            .await
            .map_err(|_| ApiError::bad_request("The .torrent file took too long to download."))?
    }

    async fn fetch_torrent_file_inner(
        &self,
        url: &Url,
        headers: ForwardedHeaders,
    ) -> ApiResult<Vec<u8>> {
        const MSG_FETCH: &str = "The .torrent file could not be downloaded.";
        let proxy = self
            .choose_proxy(url)
            .await
            .map_err(ApiError::bad_request)?;
        let proxied = proxy.is_some();
        let client = build_client(self, proxy).map_err(|e| {
            tracing::error!(error = %e.without_url(), "could not build the download client");
            ApiError::internal()
        })?;
        let mut headers = request_headers(&headers);
        let mut current = url.clone();
        for _ in 0..=MAX_REDIRECTS {
            self.guard
                .check_url(&current)
                .await
                .map_err(|e| ApiError::bad_request(e.message()))?;
            let response = client
                .get(current.clone())
                .headers(headers.clone())
                .send()
                .await
                .map_err(|e| match classify_error(&e) {
                    Step::Fatal(message) => ApiError::bad_request(message),
                    _ => ApiError::bad_request(MSG_FETCH),
                })?;
            let status = response.status();
            if status.is_redirection() && proxied {
                let next = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|loc| current.join(loc).ok())
                    .ok_or_else(|| ApiError::bad_request(MSG_FETCH))?;
                strip_cross_origin(&mut headers, &current, &next);
                current = next;
                if !proxy_reaches(&current) {
                    return Err(ApiError::bad_request(MSG_VPN_PORTS));
                }
                continue;
            }
            if !status.is_success() {
                return Err(ApiError::bad_request(status_message(status)));
            }
            if response
                .content_length()
                .is_some_and(|len| len > MAX_TORRENT_FILE as u64)
            {
                return Err(ApiError::bad_request(
                    "The .torrent file is larger than 10 MB.",
                ));
            }
            let mut body = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| ApiError::bad_request(MSG_FETCH))?;
                if body.len() + chunk.len() > MAX_TORRENT_FILE {
                    return Err(ApiError::bad_request(
                        "The .torrent file is larger than 10 MB.",
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            if super::torrents::torrent_name(&body).is_err() {
                return Err(ApiError::bad_request(
                    "That link did not return a valid .torrent file.",
                ));
            }
            return Ok(body);
        }
        Err(ApiError::bad_request(MSG_REDIRECTS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_names() {
        let cd = content_disposition_filename;
        assert_eq!(
            cd("attachment; filename=\"movie.mkv\"").as_deref(),
            Some("movie.mkv")
        );
        assert_eq!(
            cd("attachment; filename=plain.mp4").as_deref(),
            Some("plain.mp4")
        );
        assert_eq!(
            cd("attachment; filename=\"fallback.mp4\"; filename*=UTF-8''na%C3%AFve%20file.mp4")
                .as_deref(),
            Some("naïve file.mp4")
        );
        assert_eq!(
            cd("attachment; filename*=utf-8'en'%E2%82%AC%20rates.pdf").as_deref(),
            Some("€ rates.pdf")
        );
        assert_eq!(
            cd("attachment; filename*=iso-8859-1''caf%E9.jpg").as_deref(),
            Some("café.jpg")
        );
        assert_eq!(
            cd("attachment; filename=\"semi;colon \\\"quoted\\\".mkv\"").as_deref(),
            Some("semi;colon \"quoted\".mkv")
        );
        assert_eq!(cd("inline"), None);
        assert_eq!(cd("attachment; filename=\"\""), None);
    }

    #[test]
    fn cookies_stay_with_their_origin() {
        let url = |s: &str| Url::parse(s).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static("a=1"));
        headers.insert(
            header::REFERER,
            HeaderValue::from_static("https://a.example/"),
        );
        let a = url("https://a.example/x");
        strip_cross_origin(&mut headers, &a, &url("https://a.example:443/y?z"));
        assert!(
            headers.contains_key(header::COOKIE),
            "same origin keeps cookies"
        );
        for other in [
            "https://b.example/x",
            "http://a.example/x",
            "https://a.example:8443/x",
            "https://sub.a.example/x",
        ] {
            let mut h = headers.clone();
            strip_cross_origin(&mut h, &a, &url(other));
            assert!(!h.contains_key(header::COOKIE), "{other}");
            assert!(h.contains_key(header::REFERER), "{other}");
        }
    }

    #[test]
    fn ranges() {
        let mut h = HeaderMap::new();
        h.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_static("bytes 100-199/1000"),
        );
        assert_eq!(content_range(&h), Some((Some(100), Some(1000))));
        h.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_static("bytes */1000"),
        );
        assert_eq!(content_range(&h), Some((None, Some(1000))));
        h.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_static("bytes 0-9/*"),
        );
        assert_eq!(content_range(&h), Some((Some(0), None)));
    }

    #[test]
    fn names_from_responses() {
        let url = Url::parse("https://example.com/get/").unwrap();
        let mut h = HeaderMap::new();
        h.insert(header::CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
        assert_eq!(choose_name(&h, &url, &url), "get.mp4");
        let bare = Url::parse("https://example.com/").unwrap();
        assert_eq!(choose_name(&HeaderMap::new(), &bare, &bare), "download");
        h.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"../../x.mkv\""),
        );
        assert_eq!(choose_name(&h, &url, &url), "x.mkv");
    }
}
