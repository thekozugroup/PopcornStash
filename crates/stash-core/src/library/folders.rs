//! Explicit owner folder organization. Moves preserve item IDs, facts and playback history.
use std::collections::HashSet;
use std::path::Path;

use super::{Library, cleanup::Root, defs, repo, scanner};
use crate::events::Event;
use crate::models::{ItemStatus, Place};
use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};

pub const MAX_BATCH: usize = 500;

#[derive(Debug, Serialize)]
pub struct FolderList {
    /// Library-relative path; empty means the library root.
    pub folder: String,
    pub folders: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFolder {
    #[serde(default)]
    pub parent: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct MoveItems {
    pub item_ids: Vec<String>,
    #[serde(default)]
    pub folder: String,
}

#[derive(Debug, Deserialize)]
pub struct ItemIds {
    pub item_ids: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct BatchResult {
    pub completed: Vec<String>,
    pub failed: Vec<Failure>,
}

#[derive(Debug, Serialize)]
pub struct Failure {
    pub id: String,
    pub message: String,
}

pub fn validate_folder(folder: &str) -> Result<()> {
    if folder.len() > 1500 || folder.split('/').count() > 32 {
        return Err(CoreError::invalid("Choose a shorter folder path."));
    }
    if !folder.is_empty() {
        for name in folder.split('/') {
            validate_name(name)?;
        }
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.trim() != name
        || name.len() > 240
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
        || name.starts_with('.')
        || scanner::is_junk_name(name)
    {
        return Err(CoreError::invalid(
            "Use a folder name without slashes, leading dots or surrounding spaces.",
        ));
    }
    Ok(())
}

fn join(base: &str, child: &str) -> String {
    if base.is_empty() {
        child.to_string()
    } else if child.is_empty() {
        base.to_string()
    } else {
        format!("{base}/{child}")
    }
}

pub fn validate_ids(ids: &[String]) -> Result<()> {
    let unique: HashSet<_> = ids.iter().collect();
    if ids.is_empty()
        || ids.len() > MAX_BATCH
        || unique.len() != ids.len()
        || ids.iter().any(|id| id.is_empty() || id.len() > 100)
    {
        return Err(CoreError::invalid(
            "Select between 1 and 500 different files.",
        ));
    }
    Ok(())
}

impl Library {
    pub async fn library_folders(&self, id: &str, folder: &str) -> Result<FolderList> {
        validate_folder(folder)?;
        let _moves = self.lock_moves().await?;
        let def = defs::load_one(&self.db, id)
            .await?
            .ok_or(CoreError::NotFound)?;
        let root = Root::open(self.layout.root())?;
        let dir = root.directory(&join(def.folder(), folder))?;
        let mut entries = nix::dir::Dir::from_fd(dir.into()).map_err(std::io::Error::from)?;
        let mut folders = Vec::new();
        for entry in entries.iter() {
            let entry = entry.map_err(std::io::Error::from)?;
            let Ok(name) = entry.file_name().to_str() else {
                continue;
            };
            if validate_name(name).is_err() {
                continue;
            }
            let child = join(folder, name);
            // Opening through anchored descriptors excludes links and mount crossings.
            if root.directory(&join(def.folder(), &child)).is_ok() {
                folders.push(child);
                if folders.len() > 5000 {
                    return Err(CoreError::invalid(
                        "Too many subfolders. Choose a narrower folder.",
                    ));
                }
            }
        }
        folders.sort_by_key(|p| p.to_lowercase());
        Ok(FolderList {
            folder: folder.to_string(),
            folders,
        })
    }

    pub async fn create_library_folder(&self, id: &str, body: &CreateFolder) -> Result<()> {
        validate_folder(&body.parent)?;
        validate_name(&body.name)?;
        let child = join(&body.parent, &body.name);
        validate_folder(&child)?;
        let _moves = self.lock_moves().await?;
        let def = defs::load_one(&self.db, id)
            .await?
            .ok_or(CoreError::NotFound)?;
        Root::open(self.layout.root())?
            .mkdir(&join(def.folder(), &child))
            .map_err(folder_error)?;
        self.events.publish(Event::Libraries);
        Ok(())
    }

    pub async fn move_library_items(&self, id: &str, body: &MoveItems) -> Result<BatchResult> {
        validate_ids(&body.item_ids)?;
        validate_folder(&body.folder)?;
        let _moves = self.lock_moves().await?;
        let def = defs::load_one(&self.db, id)
            .await?
            .ok_or(CoreError::NotFound)?;
        let target = join(def.folder(), &body.folder);
        let root = Root::open(self.layout.root())?;
        root.directory(&target)?; // A destination must already exist.
        let group = if let Some(top) = body.folder.split('/').next().filter(|s| !s.is_empty()) {
            let groups = self.download_folders(id).await?;
            let matches: Vec<_> = groups
                .iter()
                .filter(|g| g.name.to_lowercase() == top.to_lowercase())
                .collect();
            if matches.len() > 1 {
                return Err(CoreError::conflict(
                    "More than one download folder has this name. Rename the conflicting folders first.",
                ));
            }
            Some((
                matches
                    .first()
                    .map_or_else(|| format!("folder:{top}"), |g| g.key.clone()),
                top.to_string(),
            ))
        } else {
            None
        };
        let mut result = BatchResult::default();
        for id in &body.item_ids {
            if self
                .flatten_pending
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                self.recover_flattened_files_locked().await?;
            }
            match self
                .move_library_item(
                    &root,
                    def.id(),
                    def.folder(),
                    &target,
                    id,
                    group.as_ref().map(|(k, n)| (k.as_str(), n.as_str())),
                )
                .await
            {
                Ok(()) => result.completed.push(id.clone()),
                Err(e) => result.failed.push(Failure {
                    id: id.clone(),
                    message: e.to_string(),
                }),
            }
        }
        self.events.publish(Event::Libraries);
        Ok(result)
    }

    async fn move_library_item(
        &self,
        root: &Root,
        library: &str,
        source_folder: &str,
        target: &str,
        id: &str,
        group: Option<(&str, &str)>,
    ) -> Result<()> {
        let item = repo::get(&self.db, id).await?.ok_or(CoreError::NotFound)?;
        if !item.rel_path.starts_with(&format!("{source_folder}/"))
            || item.library_id.as_deref() != Some(library)
            || item.place != Place::Library
            || item.status != ItemStatus::Ready
        {
            return Err(CoreError::conflict(
                "This file is no longer ready in this library. Refresh and select it again.",
            ));
        }
        let name = Path::new(&item.rel_path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(CoreError::NotFound)?;
        let dest = join(target, name);
        if dest == item.rel_path {
            return Ok(());
        }
        match root.metadata(&dest) {
            Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(CoreError::conflict(
                    "A file already uses this name. No file was overwritten.",
                ));
            }
            Err(e) => return Err(e),
        }
        let busy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM downloads d WHERE d.state IN ('queued','downloading','paused','seeding','sorting') AND (d.id=? OR EXISTS(SELECT 1 FROM download_items di WHERE di.download_id=d.id AND di.item_id=?))) OR EXISTS(SELECT 1 FROM conversion_jobs WHERE item_id=? AND (state IN ('encoding','verifying','publishing') OR stage IS NOT NULL))")
            .bind(&item.source_download_id).bind(id).bind(id).fetch_one(&self.db).await?;
        if busy {
            return Err(CoreError::conflict(
                "This file is downloading, seeding or converting. Try again after it finishes.",
            ));
        }
        let meta = root.metadata(&item.rel_path)?;
        if !meta.is_file() {
            return Err(CoreError::invalid("Choose a regular file."));
        }
        use std::os::unix::fs::MetadataExt;
        if meta.len() != item.size as u64 || meta.mtime() != item.mtime {
            return Err(CoreError::conflict(
                "This file changed since indexing. Refresh the library first.",
            ));
        }
        let file = super::flatten_downloads::FlattenFile {
            item_id: id.into(),
            from: item.rel_path.clone(),
            to: dest,
            kind: "item".into(),
            fingerprint: super::cleanup::fingerprint(&meta),
        };
        let result = self
            .organized_move_one(root, library, group, id, &[&file])
            .await
            .map_err(folder_error);
        // Recover interrupted per-item work before allowing the next file to move.
        self.recover_flattened_files_locked().await?;
        result?;
        Ok(())
    }

    pub async fn trash_inbox_items(&self, ids: &[String], user: &str) -> Result<BatchResult> {
        validate_ids(ids)?;
        let mut result = BatchResult::default();
        for id in ids {
            match super::organizer::trash_inbox_item(self, id, user).await {
                Ok(()) => result.completed.push(id.clone()),
                Err(e) => result.failed.push(Failure {
                    id: id.clone(),
                    message: e.to_string(),
                }),
            }
        }
        Ok(result)
    }
}

fn folder_error(error: CoreError) -> CoreError {
    match error {
        CoreError::Io(ref e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            CoreError::conflict(
                "A file or folder already uses this name. Choose another folder or rename the existing file.",
            )
        }
        CoreError::Io(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
            CoreError::conflict("The file or folder moved. Refresh before trying again.")
        }
        other => other,
    }
}
