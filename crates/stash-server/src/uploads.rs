//! Resumable chunked browser uploads (section 8, /api/uploads).
//!
//! The browser creates an upload, then PUTs chunks at increasing offsets into
//! `Downloads/In Progress/.uploads/<id>.part`. A dropped connection keeps what
//! arrived; the next PUT at a stale offset answers 409 with `received` so the client
//! continues from there. Completing hands the part file to the download manager, which
//! moves it straight to `Completed/<name>` and sorts it.

use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::body::Body;
use dashmap::DashMap;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use stash_core::db::Db;
use stash_core::models::Download;
use stash_core::paths::{self, StashLayout};
use stash_core::{new_id, now};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::auth::AuthUser;
use crate::downloads::DownloadManager;
use crate::error::{ApiError, ApiResult, ErrorCode};

pub const CHUNK_SIZE: u64 = 16 * 1024 * 1024;
pub const MAX_CHUNK: usize = 64 * 1024 * 1024;
/// Free space kept on the stash beyond the file itself.
pub const SPACE_MARGIN: u64 = 1024 * 1024 * 1024;
/// Partial uploads untouched for this long are deleted.
pub const STALE_AFTER_S: i64 = 7 * 86_400;
/// A chunk body that sends nothing for this long counts as a dropped connection.
pub const CHUNK_IDLE: Duration = Duration::from_secs(120);

const MSG_NOT_FOUND: &str = "That upload does not exist.";
const MSG_NO_SPACE: &str = "There is not enough free space on the stash for this file.";
const MSG_WRITE: &str = "The upload could not be saved. Check that the stash has free space.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUpload {
    pub filename: String,
    pub size: i64,
    pub mime: Option<String>,
    pub last_modified: Option<i64>,
}

/// `POST /api/uploads` response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UploadCreated {
    pub id: String,
    pub chunk_size: u64,
    pub received: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadStatus {
    pub id: String,
    pub filename: String,
    pub size: i64,
    pub received: i64,
    pub state: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct UploadRow {
    id: String,
    user_id: String,
    filename: String,
    size: i64,
    received: i64,
    state: String,
}

impl UploadRow {
    fn status(&self) -> UploadStatus {
        UploadStatus {
            id: self.id.clone(),
            filename: self.filename.clone(),
            size: self.size,
            received: self.received,
            state: self.state.clone(),
        }
    }
}

fn received_conflict(message: &str, received: i64) -> ApiError {
    ApiError::conflict(message).with_extra(serde_json::json!({ "received": received }))
}

/// Bytes available to unprivileged writers on the filesystem holding `path`.
pub fn free_space(path: &std::path::Path) -> std::io::Result<u64> {
    let stat = nix::sys::statvfs::statvfs(path).map_err(std::io::Error::from)?;
    #[allow(clippy::useless_conversion)]
    let free = u64::from(stat.blocks_available()).saturating_mul(u64::from(stat.fragment_size()));
    Ok(free)
}

pub struct UploadService {
    pub db: Db,
    pub layout: Arc<StashLayout>,
    pub downloads: Arc<DownloadManager>,
    /// Serializes writes, completion and cancellation per upload.
    locks: DashMap<String, Arc<Mutex<()>>>,
    /// Free space kept beyond each new upload (bytes). Lowered by tests.
    space_margin: AtomicU64,
    /// A chunk whose body sends nothing for this long is treated as a dropped
    /// connection (milliseconds). Lowered by tests.
    chunk_idle_ms: AtomicU64,
}

impl UploadService {
    pub fn new(db: Db, layout: Arc<StashLayout>, downloads: Arc<DownloadManager>) -> Self {
        Self {
            db,
            layout,
            downloads,
            locks: DashMap::new(),
            space_margin: AtomicU64::new(SPACE_MARGIN),
            chunk_idle_ms: AtomicU64::new(CHUNK_IDLE.as_millis() as u64),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_space_margin(&self, bytes: u64) {
        self.space_margin.store(bytes, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn set_chunk_idle(&self, idle: Duration) {
        self.chunk_idle_ms
            .store(idle.as_millis() as u64, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn lock_count(&self) -> usize {
        self.locks.len()
    }

    fn part_path(&self, id: &str) -> PathBuf {
        self.layout.uploads_tmp().join(format!("{id}.part"))
    }

    fn lock_for(&self, id: &str) -> Arc<Mutex<()>> {
        self.locks.entry(id.to_string()).or_default().clone()
    }

    /// Load an upload the caller may touch: their own, or any when they are the owner.
    async fn load(&self, user: &AuthUser, id: &str) -> ApiResult<UploadRow> {
        let row: Option<UploadRow> = sqlx::query_as(
            "SELECT id, user_id, filename, size, received, state FROM uploads WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.db)
        .await?;
        match row {
            Some(row) if row.user_id == user.user_id || user.is_owner() => Ok(row),
            // Someone else's upload looks exactly like a missing one.
            _ => Err(ApiError::not_found(MSG_NOT_FOUND)),
        }
    }

    /// `POST /api/uploads`: reserve space and create an empty part file.
    pub async fn create(&self, user: &AuthUser, request: CreateUpload) -> ApiResult<UploadCreated> {
        let filename = request.filename.trim();
        if filename.is_empty() {
            return Err(ApiError::bad_request("The file needs a name."));
        }
        if filename.len() > 1024 {
            return Err(ApiError::bad_request("The file name is too long."));
        }
        if request.size < 0 {
            return Err(ApiError::bad_request("The file size is not valid."));
        }
        let dir = self.layout.ensure_uploads_dir().await?;
        let free = {
            let dir = dir.clone();
            tokio::task::spawn_blocking(move || free_space(&dir))
                .await
                .map_err(|e| ApiError::from(anyhow::Error::from(e)))??
        };
        let margin = self.space_margin.load(Ordering::Relaxed);
        let needed = (request.size as u64).saturating_add(margin);
        if free < needed {
            return Err(ApiError::new(ErrorCode::Conflict, MSG_NO_SPACE));
        }
        let id = new_id();
        let mime = request
            .mime
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty() && m.len() <= 255);
        tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits())
            .open(self.part_path(&id))
            .await?;
        let t = now();
        let inserted = sqlx::query(
            "INSERT INTO uploads (id, user_id, filename, size, received, mime, last_modified, state, created_at, updated_at)
             VALUES (?, ?, ?, ?, 0, ?, ?, 'receiving', ?, ?)",
        )
        .bind(&id)
        .bind(&user.user_id)
        .bind(filename)
        .bind(request.size)
        .bind(mime)
        .bind(request.last_modified)
        .bind(t)
        .bind(t)
        .execute(&self.db)
        .await;
        if let Err(e) = inserted {
            let _ = tokio::fs::remove_file(self.part_path(&id)).await;
            return Err(e.into());
        }
        tracing::info!(upload = %id, size = request.size, "upload started");
        Ok(UploadCreated {
            id,
            chunk_size: CHUNK_SIZE,
            received: 0,
        })
    }

    /// Load an upload while holding its lock (the caller's clone of it). An id that does
    /// not exist, or is not the caller's, leaves no lock entry behind unless another
    /// request is waiting on it, so the map does not grow with every id anyone asks about.
    async fn load_locked(&self, user: &AuthUser, id: &str) -> ApiResult<UploadRow> {
        let loaded = self.load(user, id).await;
        if loaded.is_err() {
            // The map's reference plus the caller's: nobody else is waiting.
            self.locks
                .remove_if(id, |_, lock| Arc::strong_count(lock) <= 2);
        }
        loaded
    }

    /// Open the part file for writing without following a link planted in its place.
    async fn open_part(&self, id: &str) -> std::io::Result<tokio::fs::File> {
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits())
            .open(self.part_path(id))
            .await?;
        if !file.metadata().await?.is_file() {
            return Err(std::io::Error::other(
                "the partial upload is not a regular file",
            ));
        }
        Ok(file)
    }

    /// The part file holds fewer bytes than the database counted (lost on a crash or
    /// power cut): record what is really there and ask the client to continue from it.
    async fn reconcile_received(&self, id: &str, on_disk: u64) -> ApiResult<ApiError> {
        let received = i64::try_from(on_disk).unwrap_or(i64::MAX);
        tracing::warn!(upload = %id, received, "the partial upload is shorter than recorded");
        sqlx::query("UPDATE uploads SET received = ?, updated_at = ? WHERE id = ?")
            .bind(received)
            .bind(now())
            .bind(id)
            .execute(&self.db)
            .await?;
        Ok(received_conflict(
            "Part of the upload was lost. Continue from where the server is.",
            received,
        ))
    }

    /// `PUT /api/uploads/:id?offset=N`: append one chunk. `offset` must equal what the
    /// server already has. Bytes that arrive before a dropped (or silent) connection are kept.
    pub async fn write_chunk(
        &self,
        user: &AuthUser,
        id: &str,
        offset: i64,
        content_length: Option<u64>,
        body: Body,
    ) -> ApiResult<i64> {
        let lock = self.lock_for(id);
        let _guard = lock.lock().await;
        let row = self.load_locked(user, id).await?;
        if row.state != "receiving" {
            return Err(received_conflict(
                "This upload is not accepting data any more.",
                row.received,
            ));
        }
        if offset != row.received {
            return Err(received_conflict(
                "The upload continues from a different point.",
                row.received,
            ));
        }
        if content_length.is_some_and(|len| len > MAX_CHUNK as u64) {
            return Err(ApiError::new(
                ErrorCode::PayloadTooLarge,
                "Each piece of an upload must be 64 MiB or smaller.",
            ));
        }
        let room = (row.size - row.received).max(0) as u64;
        if content_length.is_some_and(|len| len > room) {
            return Err(ApiError::bad_request(
                "This piece goes past the end of the file.",
            ));
        }

        let opened = async {
            let mut file = self.open_part(id).await?;
            let on_disk = file.metadata().await?.len();
            if on_disk < offset as u64 {
                return Ok(Err(on_disk));
            }
            file.set_len(offset as u64).await?;
            file.seek(SeekFrom::Start(offset as u64)).await?;
            std::io::Result::Ok(Ok(file))
        }
        .await;
        let file = match opened {
            Ok(Ok(file)) => file,
            Ok(Err(on_disk)) => return Err(self.reconcile_received(id, on_disk).await?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(upload = %id, "the partial upload file is missing");
                self.mark(id, "cancelled").await?;
                return Err(ApiError::conflict("This upload was lost. Start it again."));
            }
            Err(e) => {
                tracing::warn!(error = %e, upload = %id, "could not open the partial upload");
                return Err(ApiError::new(ErrorCode::Internal, MSG_WRITE));
            }
        };
        let idle = Duration::from_millis(self.chunk_idle_ms.load(Ordering::Relaxed));
        let mut writer = BufWriter::with_capacity(1 << 20, file);
        let mut stream = body.into_data_stream();
        let mut written: u64 = 0;
        let mut failure: Option<ApiError> = None;
        loop {
            // A client that vanished without closing the connection would otherwise
            // hold this upload's lock for good.
            let next = match tokio::time::timeout(idle, stream.next()).await {
                Ok(next) => next,
                Err(_) => {
                    tracing::info!(upload = %id, "an upload chunk stalled; keeping what arrived");
                    failure = Some(ApiError::bad_request(
                        "The connection went quiet during the upload. Try again.",
                    ));
                    break;
                }
            };
            let Some(chunk) = next else {
                break;
            };
            let bytes = match chunk {
                Ok(bytes) => bytes,
                Err(e) => {
                    failure = Some(if is_length_limit(&e) {
                        ApiError::new(
                            ErrorCode::PayloadTooLarge,
                            "Each piece of an upload must be 64 MiB or smaller.",
                        )
                    } else {
                        ApiError::bad_request(
                            "The connection dropped during the upload. Try again.",
                        )
                    });
                    break;
                }
            };
            if written + bytes.len() as u64 > room {
                failure = Some(ApiError::bad_request(
                    "This piece goes past the end of the file.",
                ));
                break;
            }
            if let Err(e) = writer.write_all(&bytes).await {
                tracing::warn!(error = %e, upload = %id, "could not write an upload chunk");
                failure = Some(ApiError::new(ErrorCode::Internal, MSG_WRITE));
                break;
            }
            written += bytes.len() as u64;
        }
        if let Err(e) = writer.flush().await {
            tracing::warn!(error = %e, upload = %id, "could not flush an upload chunk");
            // Nothing past the last confirmed offset can be trusted.
            written = 0;
            failure.get_or_insert(ApiError::new(ErrorCode::Internal, MSG_WRITE));
        }
        let received = offset + written as i64;
        sqlx::query("UPDATE uploads SET received = ?, updated_at = ? WHERE id = ?")
            .bind(received)
            .bind(now())
            .bind(id)
            .execute(&self.db)
            .await?;
        match failure {
            Some(error) => Err(error.with_extra(serde_json::json!({ "received": received }))),
            None => Ok(received),
        }
    }

    /// `GET /api/uploads/:id`.
    pub async fn status(&self, user: &AuthUser, id: &str) -> ApiResult<UploadStatus> {
        Ok(self.load(user, id).await?.status())
    }

    /// `POST /api/uploads/:id/complete`: every byte arrived; hand the file to the
    /// download manager, which moves it straight to Completed and queues the sort.
    pub async fn complete(&self, user: &AuthUser, id: &str) -> ApiResult<Download> {
        let lock = self.lock_for(id);
        let _guard = lock.lock().await;
        let row = self.load_locked(user, id).await?;
        if row.state != "receiving" {
            return Err(ApiError::conflict("This upload has already finished."));
        }
        if row.received != row.size {
            return Err(received_conflict(
                "The upload is not finished yet.",
                row.received,
            ));
        }
        let synced = async {
            let file = self.open_part(id).await?;
            let on_disk = file.metadata().await?.len();
            if on_disk < row.size as u64 {
                return Ok(Err(on_disk));
            }
            file.set_len(row.size as u64).await?;
            file.sync_all().await?;
            std::io::Result::Ok(Ok(()))
        }
        .await;
        match synced {
            Ok(Ok(())) => {}
            Ok(Err(on_disk)) => return Err(self.reconcile_received(id, on_disk).await?),
            Err(e) => {
                tracing::warn!(error = %e, upload = %id, "could not finish the partial upload");
                return Err(ApiError::new(ErrorCode::Internal, MSG_WRITE));
            }
        }

        let rel = format!("{}/{id}.part", paths::UPLOADS_TMP);
        let download = self
            .downloads
            .upload_finished(&user.user_id, &row.filename, &rel, row.size)
            .await?;
        self.mark(id, "complete").await?;
        self.locks.remove(id);
        tracing::info!(upload = %id, download = %download.id, "upload finished");
        Ok(download)
    }

    /// `DELETE /api/uploads/:id`: stop receiving and delete the partial file.
    pub async fn cancel(&self, user: &AuthUser, id: &str) -> ApiResult<()> {
        let lock = self.lock_for(id);
        let _guard = lock.lock().await;
        let row = self.load_locked(user, id).await?;
        if row.state == "receiving" {
            remove_if_exists(&self.part_path(id)).await;
            self.mark(id, "cancelled").await?;
            tracing::info!(upload = %id, "upload cancelled");
        }
        self.locks.remove(id);
        Ok(())
    }

    async fn mark(&self, id: &str, state: &str) -> ApiResult<()> {
        sqlx::query("UPDATE uploads SET state = ?, updated_at = ? WHERE id = ?")
            .bind(state)
            .bind(now())
            .bind(id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    /// Delete partial uploads untouched for `max_age_s`, forget finished ones after the
    /// same time, and remove part files no upload owns. Returns how many were deleted.
    pub async fn cleanup(&self, max_age_s: i64) -> ApiResult<u64> {
        let cutoff = now() - max_age_s;
        let stale: Vec<(String,)> =
            sqlx::query_as("SELECT id FROM uploads WHERE state = 'receiving' AND updated_at < ?")
                .bind(cutoff)
                .fetch_all(&self.db)
                .await?;
        let mut removed = 0;
        for (id,) in stale {
            let lock = self.lock_for(&id);
            let _guard = lock.lock().await;
            remove_if_exists(&self.part_path(&id)).await;
            self.mark(&id, "cancelled").await?;
            self.locks.remove(&id);
            removed += 1;
        }
        sqlx::query("DELETE FROM uploads WHERE state != 'receiving' AND updated_at < ?")
            .bind(cutoff)
            .execute(&self.db)
            .await?;
        // Part files without a receiving upload (crash between steps) older than a day.
        let receiving: std::collections::HashSet<String> =
            sqlx::query_as::<_, (String,)>("SELECT id FROM uploads WHERE state = 'receiving'")
                .fetch_all(&self.db)
                .await?
                .into_iter()
                .map(|(id,)| id)
                .collect();
        if let Ok(mut entries) = tokio::fs::read_dir(self.layout.uploads_tmp()).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(id) = name.strip_suffix(".part") else {
                    continue;
                };
                if receiving.contains(id) {
                    continue;
                }
                let old = entry
                    .metadata()
                    .await
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|age| age > Duration::from_secs(86_400));
                if old {
                    remove_if_exists(&entry.path()).await;
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            tracing::info!(removed, "removed abandoned uploads");
        }
        Ok(removed)
    }

    /// Move partial uploads from the folder older versions used (inside `Downloads/In
    /// Progress`, which the torrent engine may write) into the private one. Only regular
    /// files this user owns are moved; a link or a file someone else put there is left
    /// alone. The old folder is removed once empty.
    pub async fn migrate_legacy(&self) {
        use std::os::unix::fs::MetadataExt;
        let legacy = self.layout.root().join(paths::LEGACY_UPLOADS_TMP);
        let Ok(meta) = tokio::fs::symlink_metadata(&legacy).await else {
            return;
        };
        if !meta.is_dir() {
            return;
        }
        let dir = match self.layout.ensure_uploads_dir().await {
            Ok(dir) => dir,
            Err(e) => {
                tracing::warn!(error = %e, "could not prepare the partial uploads folder");
                return;
            }
        };
        let me = nix::unistd::geteuid().as_raw();
        let mut moved = 0u32;
        if let Ok(mut entries) = tokio::fs::read_dir(&legacy).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                let Some(text) = name.to_str() else { continue };
                if !text.ends_with(".part") || text.starts_with('.') {
                    continue;
                }
                let ours = tokio::fs::symlink_metadata(entry.path())
                    .await
                    .is_ok_and(|m| m.is_file() && m.uid() == me && m.nlink() == 1);
                let target = dir.join(text);
                if !ours || tokio::fs::symlink_metadata(&target).await.is_ok() {
                    continue;
                }
                if tokio::fs::rename(entry.path(), &target).await.is_err() {
                    continue;
                }
                // Checked again in the private folder: a file swapped in meanwhile goes.
                let still_ours = tokio::fs::symlink_metadata(&target)
                    .await
                    .is_ok_and(|m| m.is_file() && m.uid() == me && m.nlink() == 1);
                if !still_ours {
                    let _ = tokio::fs::remove_file(&target).await;
                    continue;
                }
                use std::os::unix::fs::PermissionsExt;
                let _ = tokio::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
                    .await;
                moved += 1;
            }
        }
        if moved > 0 {
            tracing::info!(moved, "moved partial uploads to their private folder");
        }
        let _ = tokio::fs::remove_dir(&legacy).await;
    }

    /// Remove partial uploads untouched for 7 days.
    pub fn spawn_cleanup(self: &Arc<Self>, cancel: CancellationToken) {
        let service = self.clone();
        tokio::spawn(async move {
            service.migrate_legacy().await;
            loop {
                if let Err(e) = service.cleanup(STALE_AFTER_S).await {
                    tracing::warn!(error = %e, "upload cleanup failed");
                }
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(3600)) => {}
                }
            }
        });
    }
}

async fn remove_if_exists(path: &std::path::Path) {
    if let Err(e) = tokio::fs::remove_file(path).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(error = %e, path = %path.display(), "could not delete a partial upload");
    }
}

/// True when a body error came from the request size limit.
fn is_length_limit(error: &axum::Error) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(e) = current {
        if e.downcast_ref::<http_body_util::LengthLimitError>()
            .is_some()
        {
            return true;
        }
        current = e.source();
    }
    false
}
