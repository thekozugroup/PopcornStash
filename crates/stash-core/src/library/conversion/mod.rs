//! Opt-in library conversion. One local encoder at a time; payload downloads are
//! never candidates. Verified replacements retain item identity and user metadata.
mod process;
mod publication;
#[cfg(test)]
mod tests;

use super::{
    Library,
    cleanup::{Root, fingerprint},
    defs, repo,
};
use crate::{
    CoreError, Result,
    db::Db,
    models::{ItemRow, ItemStatus, MediaKind},
    new_id, now,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub enabled: bool,
    pub codec: String,
    pub quality: String,
    pub compute: String,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: false,
            codec: "av1".into(),
            quality: "balanced".into(),
            compute: "auto".into(),
        }
    }
}
impl Policy {
    fn validate(&self) -> Result<()> {
        if !["av1", "hevc"].contains(&self.codec.as_str())
            || !["high", "balanced", "small"].contains(&self.quality.as_str())
            || !["auto", "hardware", "cpu"].contains(&self.compute.as_str())
        {
            return Err(CoreError::invalid(
                "Choose AV1 or H.265, a quality preset and an available compute option.",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Encoder {
    pub codec: String,
    pub name: String,
    pub label: String,
    pub device: Option<String>,
    pub hardware: bool,
}
#[derive(Debug, Serialize, FromRow)]
pub struct JobView {
    pub id: String,
    pub item_id: Option<String>,
    pub title: String,
    pub state: String,
    pub progress: f64,
    pub encoder: Option<String>,
    pub message: Option<String>,
    pub saved_bytes: i64,
}
#[derive(Debug, Serialize)]
pub struct View {
    pub policy: Policy,
    pub encoders: Vec<Encoder>,
    pub pending: i64,
    pub jobs: Vec<JobView>,
}

async fn policy(db: &Db, id: &str) -> Result<(Policy, i64)> {
    let p = sqlx::query_as::<_, Policy>(
        "SELECT enabled,codec,quality,compute FROM library_conversion WHERE library_id=?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .unwrap_or_default();
    let revision = sqlx::query_scalar("SELECT revision FROM library_conversion WHERE library_id=?")
        .bind(id)
        .fetch_optional(db)
        .await?
        .unwrap_or(0);
    Ok((p, revision))
}

pub async fn view(lib: &Library, id: &str) -> Result<View> {
    require_video(lib, id).await?;
    let (policy, revision) = policy(&lib.db, id).await?;
    let jobs = sqlx::query_as("SELECT j.id,j.item_id,COALESCE(i.title,'Unavailable video') AS title,j.state,j.progress,j.encoder,j.message,j.saved_bytes FROM conversion_jobs j LEFT JOIN items i ON i.id=j.item_id WHERE j.library_id=? ORDER BY j.created_at DESC,j.id DESC LIMIT 30")
        .bind(id).fetch_all(&lib.db).await?;
    let pending = sqlx::query_scalar("SELECT COUNT(*) FROM items i WHERE i.library_id=? AND i.kind='video' AND i.status='ready' AND NOT EXISTS (SELECT 1 FROM conversion_jobs j WHERE j.item_id=i.id AND j.revision=? AND j.source_version=CAST(i.size AS TEXT)||':'||CAST(i.mtime AS TEXT)||':'||COALESCE(i.quick_hash,''))")
        .bind(id).bind(revision).fetch_one(&lib.db).await?;
    Ok(View {
        policy,
        encoders: process::encoders().await.clone(),
        pending,
        jobs,
    })
}

async fn require_video(lib: &Library, id: &str) -> Result<()> {
    let def = defs::load_one(&lib.db, id)
        .await?
        .ok_or(CoreError::NotFound)?;
    if def.row.content_type.as_str() != "video" {
        return Err(CoreError::invalid(
            "Automatic conversion is available for video libraries.",
        ));
    }
    Ok(())
}

pub async fn save(lib: &Library, id: &str, p: Policy) -> Result<View> {
    p.validate()?;
    let _guard = lib.lock_moves().await;
    require_video(lib, id).await?;
    // Only changed policies create a new pass. Polls/retries never reset history.
    let (previous, _) = policy(&lib.db, id).await?;
    if previous != p {
        sqlx::query("INSERT INTO library_conversion(library_id,enabled,codec,quality,compute) VALUES(?,?,?,?,?) ON CONFLICT(library_id) DO UPDATE SET enabled=excluded.enabled,codec=excluded.codec,quality=excluded.quality,compute=excluded.compute,revision=revision+1")
            .bind(id).bind(p.enabled).bind(&p.codec).bind(&p.quality).bind(&p.compute).execute(&lib.db).await?;
    }
    drop(_guard);
    view(lib, id).await
}

pub async fn retry(lib: &Library, id: &str) -> Result<View> {
    require_video(lib, id).await?;
    sqlx::query("DELETE FROM conversion_jobs WHERE library_id=? AND state IN ('failed','cancelled') AND stage IS NULL")
        .bind(id).execute(&lib.db).await?;
    view(lib, id).await
}

/// Run before scanning: finish cleanup or restore the original from each durable journal.
pub async fn recover(lib: &Library) -> Result<()> {
    if let Err(error) = publication::recover(lib).await {
        let message = format!("Recovery needs attention. Original backups were kept: {error}");
        sqlx::query("UPDATE conversion_jobs SET message=? WHERE stage IS NOT NULL")
            .bind(message)
            .execute(&lib.db)
            .await?;
        return Err(error);
    }
    Ok(())
}

pub fn start(lib: Arc<Library>, cancel: CancellationToken) {
    tokio::spawn(async move {
        loop {
            if cancel.is_cancelled() {
                break;
            }
            if let Err(e) = run_one(&lib, &cancel).await {
                tracing::warn!(error=%e, "library conversion stopped safely");
            }
            tokio::select! { _ = cancel.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(10)) => {} }
        }
    });
}

fn version(item: &ItemRow) -> String {
    format!(
        "{}:{}:{}",
        item.size,
        item.mtime,
        item.quick_hash.as_deref().unwrap_or("")
    )
}

pub(super) async fn current(lib: &Library, job: &Journal) -> Result<bool> {
    let (p, revision) = policy(&lib.db, job.library_id.as_deref().unwrap_or("")).await?;
    let item = repo::get(&lib.db, job.item_id.as_deref().unwrap_or("")).await?;
    Ok(p.enabled
        && revision == job.revision
        && item.is_some_and(|i| {
            i.status == ItemStatus::Ready
                && i.library_id == job.library_id
                && i.rel_path == job.source_path
                && version(&i) == job.source_version
        }))
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct Journal {
    id: String,
    item_id: Option<String>,
    library_id: Option<String>,
    revision: i64,
    source_version: String,
    source_path: String,
    target_path: String,
    state: String,
    stage: Option<String>,
    source_hash: Option<String>,
    output_hash: Option<String>,
}
const JOURNAL_COLUMNS: &str = "id,item_id,library_id,revision,source_version,source_path,target_path,state,stage,source_hash,output_hash";

pub(super) async fn state(
    lib: &Library,
    id: &str,
    state: &str,
    message: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE conversion_jobs SET state=?,message=?,updated_at=? WHERE id=?")
        .bind(state)
        .bind(message)
        .bind(now())
        .bind(id)
        .execute(&lib.db)
        .await?;
    Ok(())
}

pub async fn run_one(lib: &Library, cancel: &CancellationToken) -> Result<bool> {
    // A failed recovery is a hard stop; never work around an unresolved journal.
    recover(lib).await?;
    let item = sqlx::query_as::<_, ItemRow>("SELECT i.* FROM items i JOIN library_conversion p ON p.library_id=i.library_id WHERE p.enabled=1 AND i.kind='video' AND i.status='ready' AND i.place='library' AND NOT EXISTS (SELECT 1 FROM downloads d WHERE d.state IN ('queued','downloading','paused','seeding','sorting') AND (d.id=i.source_download_id OR EXISTS (SELECT 1 FROM download_items di WHERE di.download_id=d.id AND di.item_id=i.id))) AND NOT EXISTS (SELECT 1 FROM conversion_jobs j WHERE j.item_id=i.id AND j.revision=p.revision AND j.source_version=CAST(i.size AS TEXT)||':'||CAST(i.mtime AS TEXT)||':'||COALESCE(i.quick_hash,'')) ORDER BY i.added_at,i.id LIMIT 1")
        .fetch_optional(&lib.db).await?;
    let Some(item) = item else {
        return Ok(false);
    };
    if item.kind != MediaKind::Video {
        return Ok(false);
    }
    let library_id = item.library_id.clone().ok_or(CoreError::NotFound)?;
    let (p, revision) = policy(&lib.db, &library_id).await?;
    let id = new_id();
    let target = std::path::Path::new(&item.rel_path)
        .with_extension("mkv")
        .to_string_lossy()
        .into_owned();
    sqlx::query("INSERT OR IGNORE INTO conversion_jobs(id,item_id,library_id,revision,source_version,source_path,target_path,state,created_at,updated_at) VALUES(?,?,?,?,?,?,?,'encoding',?,?)")
        .bind(&id).bind(&item.id).bind(&library_id).bind(revision).bind(version(&item)).bind(&item.rel_path).bind(&target).bind(now()).bind(now()).execute(&lib.db).await?;
    let mut job = sqlx::query_as::<_, Journal>(&format!(
        "SELECT {JOURNAL_COLUMNS} FROM conversion_jobs WHERE id=?"
    ))
    .bind(&id)
    .fetch_one(&lib.db)
    .await?;
    let result = convert(lib, &item, &p, &mut job, cancel).await;
    if let Err(e) = result
        && job.state != "complete"
    {
        state(
            lib,
            &id,
            if cancel.is_cancelled() || !current(lib, &job).await.unwrap_or(false) {
                "cancelled"
            } else {
                "failed"
            },
            Some(&e.to_string()),
        )
        .await?;
    }
    // Publication owns recovery until it is completely reconciled.
    recover(lib).await?;
    Ok(true)
}

async fn convert(
    lib: &Library,
    item: &ItemRow,
    p: &Policy,
    job: &mut Journal,
    cancel: &CancellationToken,
) -> Result<()> {
    let root = Root::open(lib.layout.root())?;
    let source = root.file(&item.rel_path)?;
    let before = fingerprint(&source.metadata()?);
    let source_path = process::descriptor_path(&source, &lib.layout.abs(&item.rel_path)?);
    let facts = process::probe(&source_path).await?;
    if let Some(reason) = process::skip_reason(&facts, &p.codec) {
        state(lib, &job.id, "skipped", Some(&reason)).await?;
        return Ok(());
    }
    if job.target_path != job.source_path && root.metadata(&job.target_path).is_ok() {
        return Err(CoreError::conflict(
            "An MKV with this name already exists. The original was kept.",
        ));
    }
    let encoder = process::choose_encoder(p).await?;
    job.source_hash = Some(process::hash(&source_path).await?);
    let stage = root.new_trash_dir("conversion")?;
    root.directory(".popcornstash/Trash")?.sync_all()?;
    job.stage = Some(stage.clone());
    let journal_written =
        sqlx::query("UPDATE conversion_jobs SET stage=?,source_hash=?,encoder=? WHERE id=?")
            .bind(&stage)
            .bind(&job.source_hash)
            .bind(&encoder.label)
            .bind(&job.id)
            .execute(&lib.db)
            .await;
    if let Err(error) = journal_written {
        root.remove_conversion_dir(&stage)?;
        return Err(error.into());
    }
    let folder = root.directory(&stage)?;
    let free = nix::sys::statvfs::fstatvfs(&folder).map_err(std::io::Error::from)?;
    if free.blocks_available().saturating_mul(free.fragment_size())
        < source.metadata()?.len().saturating_add(256 * 1024 * 1024)
    {
        return Err(CoreError::external(
            "Not enough free space to keep both versions during verification. The original was kept.",
        ));
    }
    let output = process::descriptor_path(&folder, &lib.layout.abs(&stage)?).join("output.mkv");
    process::encode(lib, job, p, &encoder, &source_path, &output, &facts, cancel).await?;
    state(lib, &job.id, "verifying", None).await?;
    process::verify(lib, job, &source_path, &output, &facts, &p.codec, cancel).await?;
    let output_file = root.file(&format!("{stage}/output.mkv"))?;
    output_file.sync_all()?;
    if output_file.metadata()?.len() >= source.metadata()?.len() {
        state(
            lib,
            &job.id,
            "skipped",
            Some("The converted file was not smaller. The original was kept."),
        )
        .await?;
        return Ok(());
    }
    job.output_hash = Some(process::hash(&output).await?);
    if process::hash(&source_path).await?.as_str() != job.source_hash.as_deref().unwrap_or("")
        || fingerprint(&root.metadata(&item.rel_path)?) != before
    {
        return Err(CoreError::conflict(
            "The video changed during conversion. The original was kept.",
        ));
    }
    publication::publish(lib, job, &before, &root, cancel).await
}
