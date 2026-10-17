//! Explicit owner folder organization. Moves preserve item IDs, facts and playback history.
use std::collections::HashSet;
use std::path::Path;

use super::{Library, cleanup::Root, defs, repo, scanner};
use crate::events::{Event, LibraryChange};
use crate::models::{ItemStatus, Place};
use crate::{CoreError, Result, now};
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
        let _moves = self.lock_moves().await;
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
        let _moves = self.lock_moves().await;
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
        let _moves = self.lock_moves().await;
        let def = defs::load_one(&self.db, id)
            .await?
            .ok_or(CoreError::NotFound)?;
        let target = join(def.folder(), &body.folder);
        let root = Root::open(self.layout.root())?;
        root.directory(&target)?; // A destination must already exist.
        let mut result = BatchResult::default();
        for id in &body.item_ids {
            match self
                .move_library_item(&root, def.id(), def.folder(), &target, id)
                .await
            {
                Ok(()) => result.completed.push(id.clone()),
                Err(e) => result.failed.push(Failure {
                    id: id.clone(),
                    message: e.to_string(),
                }),
            }
        }
        Ok(result)
    }

    async fn move_library_item(
        &self,
        root: &Root,
        library: &str,
        source_folder: &str,
        target: &str,
        id: &str,
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
        if !root.metadata(&item.rel_path)?.is_file() {
            return Err(CoreError::invalid("Choose a regular file."));
        }
        let mut tx = crate::db::begin_write(&self.db).await?;
        root.rename(&item.rel_path, &dest).map_err(folder_error)?; // Atomic, no overwrite, no symlinks.
        let write = async {
            sqlx::query("UPDATE items SET rel_path = ?, updated_at = ? WHERE id = ?")
                .bind(&dest)
                .bind(now())
                .bind(id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await
        }
        .await;
        if let Err(e) = write {
            root.rename(&dest, &item.rel_path)?;
            return Err(e.into());
        }
        self.events.publish(Event::Library {
            r#type: LibraryChange::Updated,
            item_id: id.to_string(),
            before: Some(item.facts()),
        });
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
