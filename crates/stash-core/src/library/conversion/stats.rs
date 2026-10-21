use crate::{Result, db::Db};
use serde::Serialize;
use sqlx::FromRow;
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Savings {
    pub videos: i64,
    pub original_bytes: i64,
    pub current_bytes: i64,
    pub saved_bytes: i64,
    pub percent: f64,
}
impl Savings {
    pub fn add(&mut self, other: &Self) {
        self.videos += other.videos;
        self.current_bytes += other.current_bytes;
        self.saved_bytes += other.saved_bytes;
        self.finish();
    }
    fn finish(&mut self) {
        self.original_bytes = self.current_bytes.saturating_add(self.saved_bytes);
        self.percent = if self.original_bytes > 0 {
            self.saved_bytes as f64 * 100.0 / self.original_bytes as f64
        } else {
            0.0
        };
    }
}
#[derive(FromRow)]
struct Item {
    id: String,
    size: i64,
    mtime: i64,
    quick_hash: Option<String>,
}
#[derive(FromRow)]
struct Replacement {
    item_id: String,
    source_version: String,
    output_version: Option<String>,
    saved_bytes: i64,
}

/// Count retained library files, following only an unbroken chain of verified versions.
/// Moving an item between libraries keeps its savings; edits/deletion break the chain.
pub async fn savings(db: &Db, id: &str) -> Result<Savings> {
    let mut tx = db.begin().await?;
    let items = sqlx::query_as::<_, Item>("SELECT id,size,mtime,quick_hash FROM items WHERE library_id=? AND kind='video' AND status='ready' AND place='library'")
        .bind(id).fetch_all(&mut *tx).await?;
    let jobs = sqlx::query_as::<_, Replacement>("SELECT j.item_id,j.source_version,j.output_version,j.saved_bytes FROM conversion_jobs j JOIN items i ON i.id=j.item_id WHERE i.library_id=? AND i.kind='video' AND i.status='ready' AND i.place='library' AND j.state='complete' ORDER BY j.updated_at DESC,j.rowid DESC")
        .bind(id).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let mut by_item: HashMap<String, Vec<Replacement>> = HashMap::new();
    for job in jobs {
        by_item.entry(job.item_id.clone()).or_default().push(job);
    }
    let mut total = Savings::default();
    for item in items {
        total.videos += 1;
        total.current_bytes = total.current_bytes.saturating_add(item.size.max(0));
        let mut expected = format!(
            "{}:{}:{}",
            item.size,
            item.mtime,
            item.quick_hash.as_deref().unwrap_or("")
        );
        for job in by_item.remove(&item.id).unwrap_or_default() {
            if job.output_version.as_deref() != Some(&expected) {
                break;
            }
            let size = |v: &str| v.split(':').next().and_then(|v| v.parse::<i64>().ok());
            let Some(before) = size(&job.source_version) else {
                break;
            };
            let Some(after) = size(&expected) else {
                break;
            };
            if job.saved_bytes <= 0 || before.checked_sub(after) != Some(job.saved_bytes) {
                break;
            }
            total.saved_bytes = total.saved_bytes.saturating_add(job.saved_bytes);
            expected = job.source_version;
        }
    }
    total.finish();
    Ok(total)
}
