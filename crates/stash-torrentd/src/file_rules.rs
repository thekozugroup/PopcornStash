//! Persistent rules evaluated entirely against the torrent file list.
use super::*;
use stash_core::internal_api::TorrentFileRules;

const RULES_FILE: &str = "file-rules.json";
pub(super) type RuleBook = BTreeMap<String, TorrentFileRules>;

pub(super) fn load(dir: &Path) -> anyhow::Result<RuleBook> {
    let bytes = match std::fs::read(dir.join(RULES_FILE)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(RuleBook::new()),
        Err(e) => return Err(e.into()),
    };
    let book: RuleBook = serde_json::from_slice(&bytes)?;
    for (id, rules) in &book {
        parse_id(id).map_err(|_| anyhow::anyhow!("Invalid torrent ID in saved file rules"))?;
        rules.validate().map_err(anyhow::Error::msg)?;
    }
    Ok(book)
}

impl Engine {
    /// Reconcile the policy with persisted selection before librqbit opens sockets.
    /// This also closes a crash between saving a policy and updating its selection.
    pub(super) fn reconcile_rules(&self) -> anyhow::Result<()> {
        let book = lock(&self.rules).clone();
        if book.is_empty() {
            return Ok(());
        }
        let path = self.session_db();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let mut db: serde_json::Value = serde_json::from_slice(&bytes)?;
        let torrents = db
            .get_mut("torrents")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| anyhow::anyhow!("Invalid persisted torrent list"))?;
        let mut changed = false;
        for torrent in torrents.values_mut() {
            let Some(id) = torrent.get("info_hash").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if !book.contains_key(id) {
                continue;
            }
            let bytes = std::fs::read(
                self.state_dir
                    .join(SESSION_DIR)
                    .join(format!("{id}.torrent")),
            )?;
            let indices = self.rule_indices(&bytes, id, None)?.unwrap_or_default();
            let paused = indices.is_empty();
            let indices = serde_json::to_value(indices)?;
            if torrent.get("only_files") != Some(&indices) {
                torrent["only_files"] = indices;
                changed = true;
            }
            if paused && torrent.get("is_paused") != Some(&serde_json::Value::Bool(true)) {
                torrent["is_paused"] = true.into();
                changed = true;
            }
        }
        if changed {
            sys::write_atomic(&path, &serde_json::to_vec(&db)?, 0o600)?;
        }
        Ok(())
    }

    fn write_rules(&self, id: &str, rules: Option<TorrentFileRules>) -> Result<(), BackendError> {
        let mut book = lock(&self.rules);
        let mut next = book.clone();
        if let Some(rules) = rules {
            next.insert(id.into(), rules);
        } else {
            next.remove(id);
        }
        let bytes = serde_json::to_vec(&next)
            .map_err(|_| BackendError::Internal("File rules could not be saved.".into()))?;
        sys::write_atomic(&self.state_dir.join(RULES_FILE), &bytes, 0o600)
            .map_err(|_| BackendError::Internal("File rules could not be saved.".into()))?;
        *book = next;
        Ok(())
    }

    pub(super) fn remember_rules(
        &self,
        id: &str,
        rules: Option<TorrentFileRules>,
    ) -> Result<(), BackendError> {
        if let Some(rules) = rules {
            rules.validate().map_err(BackendError::BadRequest)?;
            self.write_rules(id, Some(rules))?;
        }
        Ok(())
    }

    pub(super) fn forget_rules(&self, id: &str) -> Result<(), BackendError> {
        let exists = lock(&self.rules).contains_key(id);
        if exists {
            self.write_rules(id, None)?;
        }
        Ok(())
    }

    pub(super) fn rule_indices(
        &self,
        bytes: &[u8],
        id: &str,
        types: Option<&[TorrentFileKind]>,
    ) -> anyhow::Result<Option<Vec<usize>>> {
        let Some(rules) = lock(&self.rules).get(id).cloned() else {
            return selected_indices(bytes, types);
        };
        let meta = librqbit::torrent_from_bytes(bytes)?;
        let info = meta.info.data.validate()?;
        Ok(Some(
            info.iter_file_details()
                .enumerate()
                .filter_map(|(i, f)| {
                    (!f.attrs().padding && rules.matches(&f.filename.to_string(), f.len))
                        .then_some(i)
                })
                .collect(),
        ))
    }

    pub(super) async fn apply_rules(
        &self,
        id: String,
        rules: TorrentFileRules,
    ) -> Result<(), BackendError> {
        rules.validate().map_err(BackendError::BadRequest)?;
        let _operation = self.pending_ops.lock().await;
        let session = self
            .live_session()
            .await
            .ok_or_else(|| BackendError::Blocked(MSG_VPN_WAIT.into()))?;
        let handle = session
            .get(TorrentIdOrHash::Hash(parse_id(&id)?))
            .ok_or(BackendError::NotFound)?;
        let files = self.files(id.clone()).await?;
        let indices: HashSet<_> = files
            .files
            .iter()
            .filter(|f| rules.matches(&f.path, f.size))
            .map(|f| f.index)
            .collect();
        let old = lock(&self.rules).get(&id).cloned();
        self.remember_rules(&id, Some(rules))?;
        if session.update_only_files(&handle, &indices).await.is_err() {
            self.write_rules(&id, old)?;
            return Err(BackendError::BadRequest(
                "The torrent is checking. Try applying the rules again shortly.".into(),
            ));
        }
        if indices.is_empty() && !handle.is_paused() {
            session
                .pause(&handle)
                .await
                .map_err(|_| BackendError::Internal("The torrent could not be paused.".into()))?;
        }
        Ok(())
    }
}
