//! Explicit, previewed physical moves. Never overwrites user files or schedules work.
pub(super) mod recovery;
use super::{
    Library,
    cleanup::{Root, fingerprint},
    defs,
    download_folders::{GROUP_KEY, validate_key},
    folders::{BatchResult, Failure},
    organizer, repo, scanner,
};
use crate::{
    CoreError, Result,
    events::{Event, LibraryChange},
    models::ItemRow,
    now,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    os::unix::fs::MetadataExt,
    path::Path,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlattenRequest {
    pub key: String,
    /// Omitted previews only. A matching preview is required to move anything.
    pub token: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct FlattenPreview {
    pub key: String,
    pub folder: String,
    pub files: Vec<FlattenFile>,
    pub blocked: Vec<String>,
    pub token: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlattenFile {
    pub item_id: String,
    /// Stash-relative paths, visible only to full owners.
    pub from: String,
    pub to: String,
    pub kind: String,
    pub fingerprint: String,
}

impl Library {
    pub async fn preview_download_flatten(&self, id: &str, key: &str) -> Result<FlattenPreview> {
        let _moves = self.lock_moves().await?;
        self.download_flatten_plan(id, key).await
    }

    async fn download_flatten_plan(&self, id: &str, key: &str) -> Result<FlattenPreview> {
        validate_key(key)?;
        let def = defs::load_one(&self.db, id)
            .await?
            .ok_or(CoreError::NotFound)?;
        if def.row.content_type.as_str() != "video" {
            return Err(CoreError::invalid("Choose a video library."));
        }
        let groups = self.download_folders(id).await?;
        let group = groups
            .iter()
            .find(|g| g.key == key)
            .ok_or(CoreError::NotFound)?;
        super::folders::validate_folder(&group.name)?;
        if group.name.is_empty() || group.name.contains('/') {
            return Err(CoreError::invalid(
                "Rename this download folder to one safe folder name first.",
            ));
        }
        let target = format!("{}/{}", def.folder(), group.name);
        let mut plan = FlattenPreview {
            key: key.into(),
            folder: target.clone(),
            files: Vec::new(),
            blocked: Vec::new(),
            token: String::new(),
        };
        if groups
            .iter()
            .any(|g| g.key != key && g.name.to_lowercase() == group.name.to_lowercase())
        {
            plan.blocked
                .push("Another download folder has this name. Rename this folder first.".into());
        }
        let sql = format!(
            "SELECT i.* FROM items i JOIN libraries l ON l.id=i.library_id WHERE i.library_id=? AND i.status='ready' AND i.kind='video' AND i.place='library' AND {GROUP_KEY}=? ORDER BY i.id LIMIT 501"
        );
        let items = sqlx::query_as::<_, ItemRow>(&sql)
            .bind(id)
            .bind(key)
            .fetch_all(&self.db)
            .await?;
        if items.len() > 500 {
            return Err(CoreError::invalid(
                "Flatten up to 500 videos at a time. Split this download folder first.",
            ));
        }
        let root = Root::open(self.layout.root())?;
        root.directory(def.folder())?;
        match root.metadata(&target) {
            Ok(meta) if meta.is_dir() => {
                root.directory(&target)?;
            }
            Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => plan
                .blocked
                .push("The destination is unavailable or is not a regular folder.".into()),
        }
        let mut directories = HashMap::new();
        let mut destinations = HashSet::new();
        for item in items {
            let busy:bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM downloads d WHERE d.state IN ('queued','downloading','paused','seeding','sorting') AND (d.id=? OR EXISTS(SELECT 1 FROM download_items di WHERE di.download_id=d.id AND di.item_id=?))) OR EXISTS(SELECT 1 FROM conversion_jobs WHERE item_id=? AND (state IN ('encoding','verifying','publishing') OR stage IS NOT NULL))")
                .bind(&item.source_download_id).bind(&item.id).bind(&item.id).fetch_one(&self.db).await?;
            if busy {
                plan.blocked.push(format!(
                    "{} is downloading, seeding or converting. Try again after it finishes.",
                    item.title
                ));
            }
            if !item.rel_path.starts_with(&format!("{}/", def.folder())) {
                return Err(CoreError::conflict(
                    "A video left this library. Refresh the folder.",
                ));
            }
            let path = Path::new(&item.rel_path);
            let parent = path
                .parent()
                .and_then(Path::to_str)
                .ok_or(CoreError::NotFound)?;
            if !directories.contains_key(parent) {
                directories.insert(parent.to_string(), directory_files(&root, parent)?);
            }
            let names = &directories[parent];
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or(CoreError::NotFound)?;
            let mut choices = vec![(name.to_string(), "video")];
            for sidecar in names.iter().filter(|n| scanner::is_sidecar(Path::new(n))) {
                if let Some(ours) = organizer::sidecar_claim(sidecar, name) {
                    let theirs = names
                        .iter()
                        .filter(|n| {
                            !scanner::is_sidecar(Path::new(n))
                                && super::classify::media_kind(Path::new(n))
                                    != crate::models::MediaKind::Other
                        })
                        .filter_map(|n| organizer::sidecar_claim(sidecar, n))
                        .max()
                        .unwrap_or(0);
                    if ours >= theirs {
                        choices.push((sidecar.clone(), "sidecar"));
                    }
                }
            }
            for (file, kind) in choices {
                let from = format!("{parent}/{file}");
                let to = format!("{target}/{file}");
                let meta = match root.metadata(&from) {
                    Ok(m) if m.is_file() => m,
                    _ => {
                        plan.blocked.push(format!(
                            "{from} changed or cannot be moved safely. Refresh the preview."
                        ));
                        continue;
                    }
                };
                if from != to {
                    if !destinations.insert(to.to_lowercase()) {
                        plan.blocked.push(format!(
                            "More than one file would use {to}. Rename the conflicting files first."
                        ));
                    }
                    match root.metadata(&to) {
                        Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
                        _ => plan.blocked.push(format!(
                            "{to} already exists or cannot be checked. No file will be overwritten."
                        )),
                    }
                } else if !destinations.insert(to.to_lowercase()) {
                    plan.blocked.push(format!(
                        "More than one file would use {to}. Rename the conflicting files first."
                    ));
                }
                // An indexed companion needs its own identity update; never move it implicitly.
                if kind == "sidecar" {
                    let indexed: bool =
                        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM items WHERE rel_path=?)")
                            .bind(&from)
                            .fetch_one(&self.db)
                            .await?;
                    if indexed {
                        plan.blocked.push(format!(
                            "{from} is a separate library item. Organize it separately first."
                        ));
                    }
                }
                if kind == "video" && (meta.len() != item.size as u64 || meta.mtime() != item.mtime)
                {
                    plan.blocked.push(format!(
                        "{from} changed since indexing. Refresh the library first."
                    ));
                }
                if plan.files.len() >= 2000 {
                    return Err(CoreError::invalid(
                        "Flatten up to 2,000 files including sidecars. Split this folder first.",
                    ));
                }
                plan.files.push(FlattenFile {
                    item_id: item.id.clone(),
                    from,
                    to,
                    kind: kind.into(),
                    fingerprint: fingerprint(&meta),
                });
            }
        }
        plan.files
            .sort_by(|a, b| a.from.cmp(&b.from).then(a.item_id.cmp(&b.item_id)));
        plan.blocked.sort();
        plan.blocked.dedup();
        plan.token = hex::encode(Sha256::digest(
            serde_json::to_vec(&plan).map_err(|e| CoreError::invalid(e.to_string()))?,
        ));
        Ok(plan)
    }

    pub async fn flatten_download(&self, id: &str, body: &FlattenRequest) -> Result<BatchResult> {
        let _moves = self.lock_moves().await?;
        let plan = self.download_flatten_plan(id, &body.key).await?;
        if body.token.as_deref() != Some(plan.token.as_str()) {
            return Err(CoreError::conflict(
                "The files changed. Review a fresh preview before moving them.",
            ));
        }
        if !plan.blocked.is_empty() {
            return Err(CoreError::conflict(
                "Resolve the preview's blocked files before flattening.",
            ));
        }
        let root = Root::open(self.layout.root())?;
        let mut result = BatchResult::default();
        let ids: HashSet<_> = plan.files.iter().map(|f| f.item_id.as_str()).collect();
        let mut ids: Vec<_> = ids.into_iter().collect();
        ids.sort();
        for item_id in ids {
            let files: Vec<_> = plan.files.iter().filter(|f| f.item_id == item_id).collect();
            match self
                .flatten_one(&root, id, &body.key, item_id, &files)
                .await
            {
                Ok(()) => result.completed.push(item_id.to_string()),
                Err(e) => result.failed.push(Failure {
                    id: item_id.into(),
                    message: e.to_string(),
                }),
            }
            // Unresolved journals block every later file mutation, including scans.
            self.recover_flattened_files_locked().await?;
        }
        self.events.publish(Event::Libraries);
        Ok(result)
    }

    async fn flatten_one(
        &self,
        root: &Root,
        library_id: &str,
        key: &str,
        id: &str,
        files: &[&FlattenFile],
    ) -> Result<()> {
        let item = repo::get(&self.db, id).await?.ok_or(CoreError::NotFound)?;
        if files.iter().all(|f| f.from == f.to) {
            return Ok(());
        }
        use sqlx::Connection;
        self.flatten_pending
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let target = Path::new(&files[0].to)
            .parent()
            .and_then(Path::to_str)
            .ok_or(CoreError::NotFound)?;
        root.prepare_flatten_destination(target)?;
        let stage = root.new_trash_dir("flatten")?;
        let journal = recovery::Journal::new(id, library_id, key, files, root)?;
        root.write_flatten_journal(
            &stage,
            &serde_json::to_vec(&journal).map_err(|e| CoreError::invalid(e.to_string()))?,
        )?;
        let mut conn = self.db.acquire().await?;
        sqlx::query("PRAGMA synchronous=FULL")
            .execute(&mut *conn)
            .await?;
        let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<()> = async {
            for (index, file) in files.iter().enumerate() {
                if fingerprint(&root.metadata(&file.from)?) != file.fingerprint {
                    return Err(CoreError::conflict("A file changed. Refresh the preview."));
                }
                if file.from == file.to { continue; }
                let private = format!("{stage}/file-{index}");
                root.rename_trash(&file.from, &private)?;
                recovery::sync_parent(root, &file.from)?;
                recovery::sync_parent(root, &private)?;
                journal.files[index].check(root, &private)?;
            }
            for (index, file) in files.iter().enumerate() {
                if file.from == file.to { continue; }
                let private = format!("{stage}/file-{index}");
                root.rename_trash(&private, &file.to)?;
                recovery::sync_parent(root, &private)?;
                recovery::sync_parent(root, &file.to)?;
                journal.files[index].check(root, &file.to)?;
            }
            let video = files.iter().find(|f| f.kind == "video").ok_or(CoreError::NotFound)?;
            let name = Path::new(target).file_name().and_then(|s| s.to_str()).ok_or(CoreError::NotFound)?;
            sqlx::query("INSERT OR IGNORE INTO library_download_folders(library_id,group_key,name) VALUES(?,?,?)").bind(library_id).bind(key).bind(name).execute(&mut *tx).await?;
            sqlx::query("UPDATE items SET rel_path=?,updated_at=? WHERE id=?").bind(&video.to).bind(now()).bind(id).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO library_download_members(item_id,library_id,group_key) VALUES(?,?,?) ON CONFLICT(item_id) DO UPDATE SET library_id=excluded.library_id,group_key=excluded.group_key").bind(id).bind(library_id).bind(key).execute(&mut *tx).await?;
            Ok(())
        }.await;
        if let Err(error) = result {
            let rollback = tx.rollback().await;
            drop(conn);
            rollback?;
            recovery::recover_one(self, root, &stage, &journal).await?;
            return Err(error);
        }
        if let Err(error) = tx.commit().await {
            drop(conn);
            recovery::recover_one(self, root, &stage, &journal).await?;
            return Err(error.into());
        }
        drop(conn);
        recovery::recover_one(self, root, &stage, &journal).await?;
        self.events.publish(Event::Library {
            r#type: LibraryChange::Updated,
            item_id: id.into(),
            before: Some(item.facts()),
        });
        Ok(())
    }
}

/// Read only anchored regular files. No symlink traversal or unbounded directories.
fn directory_files(root: &Root, parent: &str) -> Result<Vec<String>> {
    let fd = root.directory(parent)?;
    let mut entries = nix::dir::Dir::from_fd(fd.into()).map_err(std::io::Error::from)?;
    let mut names = Vec::new();
    let mut count = 0;
    for entry in entries.iter() {
        count += 1;
        if count > 5000 {
            return Err(CoreError::invalid(
                "This source folder has too many entries. Organize a smaller folder first.",
            ));
        }
        let entry = entry.map_err(std::io::Error::from)?;
        let Ok(name) = entry.file_name().to_str() else {
            continue;
        };
        if name == "." || name == ".." || scanner::is_junk_name(name) {
            continue;
        }
        match root.metadata(&format!("{parent}/{name}")) {
            Ok(m) if m.is_file() => names.push(name.to_string()),
            // Do not silently strand a matching sidecar that cannot be read safely.
            Err(e) if scanner::is_sidecar(Path::new(name)) => return Err(e),
            _ => {}
        }
    }
    names.sort();
    Ok(names)
}
