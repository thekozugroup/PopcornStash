//! Metadata-first torrents stay paused until an explicit, durable file selection.
use super::*;
const FILE: &str = "reviews.json";

pub(super) fn load(dir: &Path) -> anyhow::Result<HashSet<String>> {
    let bytes = match std::fs::read(dir.join(FILE)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(e) => return Err(e.into()),
    };
    let ids: HashSet<String> = serde_json::from_slice(&bytes)?;
    for id in &ids {
        parse_id(id).map_err(|_| anyhow::anyhow!("Invalid torrent ID in saved reviews"))?;
    }
    Ok(ids)
}

impl Engine {
    pub(super) fn needs_review(&self, id: &str) -> bool {
        lock(&self.reviews).contains(id)
    }

    pub(super) fn write_review(&self, id: &str, required: bool) -> Result<(), BackendError> {
        let mut saved = lock(&self.reviews);
        let mut next = saved.clone();
        if required {
            next.insert(id.into());
        } else {
            next.remove(id);
        }
        if *saved == next {
            return Ok(());
        }
        // Sorted output is stable and private; commit in-memory state only after disk.
        let sorted: std::collections::BTreeSet<_> = next.iter().collect();
        let bytes = serde_json::to_vec(&sorted)
            .map_err(|_| BackendError::Internal("Torrent review could not be saved.".into()))?;
        sys::write_atomic(&self.state_dir.join(FILE), &bytes, 0o600)
            .map_err(|_| BackendError::Internal("Torrent review could not be saved.".into()))?;
        *saved = next;
        Ok(())
    }

    pub(super) fn reconcile_reviews(&self) -> anyhow::Result<()> {
        if lock(&self.reviews).is_empty() {
            return Ok(());
        }
        let bytes = match std::fs::read(self.session_db()) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let mut db: serde_json::Value = serde_json::from_slice(&bytes)?;
        let torrents = db
            .get_mut("torrents")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| anyhow::anyhow!("Invalid persisted torrent list"))?;
        for torrent in torrents.values_mut() {
            if torrent
                .get("info_hash")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|id| self.needs_review(id))
            {
                torrent["is_paused"] = true.into();
            }
        }
        self.write_session_db(&db).map_err(anyhow::Error::msg)
    }
}
