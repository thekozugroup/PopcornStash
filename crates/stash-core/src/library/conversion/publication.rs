use super::{JOURNAL_COLUMNS, Journal, Library, current, process};
use crate::events::LibraryChange;
use crate::library::{
    cleanup::{Root, fingerprint},
    organizer, repo, scanner,
};
use crate::{CoreError, Result, db, now};
use sqlx::Connection;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use tokio_util::sync::CancellationToken;

// Publication journals and the catalog commit use FULL durability. Never delete
// a backup based on a write that only reached the SQLite page cache.
async fn durable(lib: &Library, job: &Journal, state: &str) -> Result<()> {
    let mut conn = lib.db.acquire().await?;
    sqlx::query("PRAGMA synchronous=FULL")
        .execute(&mut *conn)
        .await?;
    let result = sqlx::query(
        "UPDATE conversion_jobs SET state=?,source_hash=?,output_hash=?,updated_at=? WHERE id=?",
    )
    .bind(state)
    .bind(&job.source_hash)
    .bind(&job.output_hash)
    .bind(now())
    .bind(&job.id)
    .execute(&mut *conn)
    .await;
    // Keeping this connection at FULL is deliberate: it is safe for other writers.
    result?;
    Ok(())
}

pub(super) async fn publish(
    lib: &Library,
    job: &mut Journal,
    before: &str,
    root: &Root,
    cancel: &CancellationToken,
) -> Result<()> {
    let _guard = lib.lock_moves().await;
    if cancel.is_cancelled()
        || !current(lib, job).await?
        || fingerprint(&root.metadata(&job.source_path)?) != before
    {
        return Err(CoreError::conflict(
            "The video or its library changed. The original was kept.",
        ));
    }
    let stage = job
        .stage
        .as_deref()
        .ok_or_else(|| CoreError::invalid("Missing conversion journal."))?;
    let original = root.file(&job.source_path)?;
    let metadata = original.metadata()?;
    if metadata.nlink() != 1 {
        return Err(CoreError::conflict(
            "This video has hard links. The original was kept to protect other uses.",
        ));
    }
    let output = root.file(&format!("{stage}/output.mkv"))?;
    let output_meta = output.metadata()?;
    if metadata.uid() != output_meta.uid() || metadata.gid() != output_meta.gid() {
        nix::unistd::fchown(
            &output,
            Some(nix::unistd::Uid::from_raw(metadata.uid())),
            Some(nix::unistd::Gid::from_raw(metadata.gid())),
        )
        .map_err(std::io::Error::from)?;
    }
    output.set_permissions(std::fs::Permissions::from_mode(metadata.mode() & 0o777))?;
    output.sync_all()?;
    root.directory(stage)?.sync_all()?;
    durable(lib, job, "publishing").await?;
    job.state = "publishing".into();
    root.rename_trash(&job.source_path, &format!("{stage}/original"))?;
    if !matches(
        root,
        &format!("{stage}/original"),
        job.source_hash.as_deref(),
        lib,
    )
    .await?
    {
        root.rename_trash(&format!("{stage}/original"), &job.source_path)?;
        return Err(CoreError::conflict(
            "The file changed at replacement. It was restored unchanged.",
        ));
    }
    sync_parent(root, &job.source_path)?;
    root.directory(stage)?.sync_all()?;
    root.rename_trash(&format!("{stage}/output.mkv"), &job.target_path)?;
    sync_parent(root, &job.source_path)?;
    sync_parent(root, &job.target_path)?;
    root.directory(stage)?.sync_all()?;
    let meta = root.metadata(&job.target_path)?;
    let path = lib.layout.abs(&job.target_path)?;
    let quick_hash = scanner::quick_hash_async(&path).await;
    if quick_hash.is_none() {
        return Err(CoreError::external(
            "Could not index the verified video. The original will be restored.",
        ));
    }
    let id = job.item_id.as_deref().ok_or(CoreError::NotFound)?;
    let previous = repo::get(&lib.db, id).await?.ok_or(CoreError::NotFound)?;
    let mut conn = lib.db.acquire().await?;
    sqlx::query("PRAGMA synchronous=FULL")
        .execute(&mut *conn)
        .await?;
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    let updated = sqlx::query("UPDATE items SET rel_path=?,size=?,mtime=?,quick_hash=?,mime='video/x-matroska',media_json=NULL,duration_s=NULL,width=NULL,height=NULL,thumb_state='pending',updated_at=? WHERE id=? AND rel_path=? AND status='ready' AND library_id=?")
        .bind(&job.target_path).bind(i64::try_from(meta.len()).unwrap_or(i64::MAX)).bind(meta.mtime()).bind(quick_hash).bind(now()).bind(id).bind(&job.source_path).bind(&job.library_id).execute(&mut *tx).await?;
    if updated.rows_affected() != 1 {
        return Err(CoreError::conflict(
            "The library entry changed. The original will be restored.",
        ));
    }
    sqlx::query("UPDATE conversion_jobs SET state='complete',progress=1,saved_bytes=?,updated_at=? WHERE id=?")
        .bind(i64::try_from(metadata.len().saturating_sub(meta.len())).unwrap_or(i64::MAX)).bind(now()).bind(&job.id).execute(&mut *tx).await?;
    tx.commit().await?;
    drop(conn);
    job.state = "complete".into();
    // Cache failures must never change the committed publication into a rollback.
    organizer::remove_cached_media(lib, id).await;
    scanner::enqueue_media_jobs(lib, id).await?;
    scanner::publish_moved(lib, LibraryChange::Updated, id, previous.facts());
    Ok(())
}

fn sync_parent(root: &Root, rel: &str) -> Result<()> {
    let parent = std::path::Path::new(rel)
        .parent()
        .and_then(|p| p.to_str())
        .ok_or_else(|| CoreError::invalid("Invalid video folder."))?;
    root.directory(parent)?.sync_all()?;
    Ok(())
}
async fn matches(root: &Root, rel: &str, expected: Option<&str>, lib: &Library) -> Result<bool> {
    let Some(expected) = expected else {
        return Ok(false);
    };
    let file = match root.file(rel) {
        Ok(file) => file,
        Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let path = process::descriptor_path(&file, &lib.layout.abs(rel)?);
    Ok(process::hash(&path).await? == expected)
}

pub(super) async fn recover(lib: &Library) -> Result<()> {
    let _guard = lib.lock_moves().await;
    let jobs = sqlx::query_as::<_, Journal>(&format!("SELECT {JOURNAL_COLUMNS} FROM conversion_jobs WHERE stage IS NOT NULL ORDER BY created_at,id")).fetch_all(&lib.db).await?;
    let root = Root::open(lib.layout.root())?;
    for job in jobs {
        let stage = job
            .stage
            .as_deref()
            .ok_or_else(|| CoreError::invalid("Missing conversion stage."))?;
        let backup = format!("{stage}/original");
        let output = format!("{stage}/output.mkv");
        // An app/power failure after rmdir but before clearing the journal is
        // harmless. Recheck the authoritative file before clearing that journal.
        if let Err(CoreError::Io(e)) = root.directory(stage)
            && e.kind() == std::io::ErrorKind::NotFound
        {
            let (rel, hash) = if job.state == "complete" {
                (&job.target_path, &job.output_hash)
            } else {
                (&job.source_path, &job.source_hash)
            };
            if !matches(&root, rel, hash.as_deref(), lib).await? {
                return Err(CoreError::conflict(
                    "A conversion journal could not be reconciled. Files were kept.",
                ));
            }
            sqlx::query("UPDATE conversion_jobs SET stage=NULL,state=CASE WHEN state='complete' THEN state ELSE 'cancelled' END WHERE id=?").bind(&job.id).execute(&lib.db).await?;
            continue;
        }
        if job.state == "complete" {
            if !matches(&root, &job.target_path, job.output_hash.as_deref(), lib).await? {
                return Err(CoreError::conflict(
                    "A converted video changed before cleanup. Its original backup has been retained for recovery.",
                ));
            }
            let backup_exists = root.metadata(&backup).is_ok();
            if backup_exists && !matches(&root, &backup, job.source_hash.as_deref(), lib).await? {
                return Err(CoreError::conflict(
                    "An original backup changed. Both files have been kept for recovery.",
                ));
            }
            root.remove_conversion_leaf(&backup)?;
        } else if root.metadata(&backup).is_ok() {
            if !matches(&root, &backup, job.source_hash.as_deref(), lib).await? {
                return Err(CoreError::conflict(
                    "Original backup verification failed. Recovery stopped without deleting files.",
                ));
            }
            // The public target may only be reclaimed if its exact verified bytes
            // are still there. A new file placed there by the owner is never touched.
            if root.metadata(&job.target_path).is_ok() {
                if !matches(&root, &job.target_path, job.output_hash.as_deref(), lib).await? {
                    return Err(CoreError::conflict(
                        "A file occupies the conversion destination. Recovery kept both copies.",
                    ));
                }
                root.rename_trash(&job.target_path, &output)?;
            }
            if root.metadata(&job.source_path).is_ok() {
                return Err(CoreError::conflict(
                    "The original name is occupied. The backup was kept for recovery.",
                ));
            }
            root.rename_trash(&backup, &job.source_path)?;
            sync_parent(&root, &job.source_path)?;
        } else if job.state == "publishing"
            && !matches(&root, &job.source_path, job.source_hash.as_deref(), lib).await?
        {
            return Err(CoreError::conflict(
                "An interrupted conversion needs recovery. No files were deleted.",
            ));
        }
        root.remove_conversion_leaf(&output)?;
        root.remove_conversion_dir(stage)?;
        let mut tx = db::begin_write(&lib.db).await?;
        sqlx::query("UPDATE conversion_jobs SET stage=NULL,state=CASE WHEN state IN ('encoding','verifying','publishing') THEN 'cancelled' ELSE state END,message=CASE WHEN state IN ('encoding','verifying','publishing') THEN 'Interrupted conversion. The original was kept; retry when ready.' ELSE message END,updated_at=? WHERE id=?")
            .bind(now()).bind(&job.id).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    // Jobs interrupted before a staging directory was allocated are safe to retry.
    sqlx::query("UPDATE conversion_jobs SET state='cancelled',message='Interrupted before conversion. The original was kept.' WHERE stage IS NULL AND state IN ('encoding','verifying')").execute(&lib.db).await?;
    Ok(())
}
