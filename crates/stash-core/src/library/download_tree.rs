//! Browse source directories inside a download group without changing disk paths.
use super::{Library, defs, download_folders::GROUP_KEY, folders};
use crate::{CoreError, Result, db::Db};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TreeQuery {
    pub key: String,
    pub folder: String,
}
#[derive(Debug, Serialize)]
pub struct DownloadTree {
    pub folder: String,
    pub folders: Vec<TreeFolder>,
}
#[derive(Debug, Serialize)]
pub struct TreeFolder {
    pub name: String,
    pub folder: String,
    pub item_count: i64,
    pub size: i64,
}

/// The shared first physical directory is the download's root. Scattered groups
/// retain a virtual root at the library; no guessed directory can escape it.
pub(super) async fn base(db: &Db, id: &str, key: &str) -> Result<String> {
    super::download_folders::validate_key(key)?;
    let sql = format!(
        "SELECT i.rel_path FROM items i JOIN libraries l ON l.id=i.library_id WHERE i.library_id=? AND i.place='library' AND i.status='ready' AND i.kind='video' AND {GROUP_KEY}=? LIMIT 50001"
    );
    let paths: Vec<String> = sqlx::query_scalar(&sql)
        .bind(id)
        .bind(key)
        .fetch_all(db)
        .await?;
    if paths.len() > 50000 {
        return Err(CoreError::invalid(
            "This download has too many videos. Use All videos.",
        ));
    }
    let def = defs::load_one(db, id).await?.ok_or(CoreError::NotFound)?;
    if paths.is_empty() {
        let name: Option<String> = sqlx::query_scalar(
            "SELECT name FROM library_download_folders WHERE library_id=? AND group_key=?",
        )
        .bind(id)
        .bind(key)
        .fetch_optional(db)
        .await?;
        return Ok(name.unwrap_or_default());
    }
    let prefix = format!("{}/", def.folder());
    let mut first: Option<&str> = None;
    for path in &paths {
        let rel = path.strip_prefix(&prefix).ok_or(CoreError::NotFound)?;
        let Some((part, _)) = rel.split_once('/') else {
            return Ok(String::new());
        };
        if first.is_some_and(|f| f != part) {
            return Ok(String::new());
        }
        first = Some(part);
    }
    Ok(first.unwrap_or("").to_string())
}

pub(super) async fn prefix(db: &Db, id: &str, key: &str, folder: &str) -> Result<String> {
    folders::validate_folder(folder)?;
    let def = defs::load_one(db, id).await?.ok_or(CoreError::NotFound)?;
    let base = base(db, id, key).await?;
    Ok(format!(
        "{}/{}{}{}",
        def.folder(),
        base,
        if !base.is_empty() && !folder.is_empty() {
            "/"
        } else {
            ""
        },
        folder
    )
    .trim_end_matches('/')
    .to_string()
        + "/")
}
impl Library {
    pub async fn download_tree(&self, id: &str, key: &str, folder: &str) -> Result<DownloadTree> {
        let _moves = self.lock_moves().await?;
        if !self
            .download_folders(id)
            .await?
            .iter()
            .any(|g| g.key == key)
        {
            return Err(CoreError::NotFound);
        }
        let prefix = prefix(&self.db, id, key, folder).await?;
        let sql = format!(
            "SELECT i.rel_path,i.size FROM items i JOIN libraries l ON l.id=i.library_id WHERE i.library_id=? AND i.place='library' AND i.status='ready' AND i.kind='video' AND {GROUP_KEY}=? AND substr(i.rel_path,1,?)=? LIMIT 50001"
        );
        let rows: Vec<(String, i64)> = sqlx::query_as(&sql)
            .bind(id)
            .bind(key)
            .bind(prefix.chars().count() as i64)
            .bind(&prefix)
            .fetch_all(&self.db)
            .await?;
        if rows.len() > 50000 {
            return Err(CoreError::invalid(
                "This download has too many videos. Collapse its subfolders or use All videos.",
            ));
        }
        let mut children: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        for (path, size) in rows {
            if let Some((name, _)) = path[prefix.len()..].split_once('/') {
                let value = children.entry(name.into()).or_default();
                value.0 += 1;
                value.1 += size;
            }
        }
        // Include real empty subfolders so a newly created folder is usable.
        if !base(&self.db, id, key).await?.is_empty() {
            let physical = prefix.trim_end_matches('/');
            let root = super::cleanup::Root::open(self.layout.root())?;
            if let Ok(dir) = root.directory(physical) {
                let mut entries =
                    nix::dir::Dir::from_fd(dir.into()).map_err(std::io::Error::from)?;
                for (n, entry) in entries.iter().enumerate() {
                    if n > 5000 {
                        return Err(CoreError::invalid(
                            "Too many subfolders. Choose a narrower folder.",
                        ));
                    }
                    let entry = entry.map_err(std::io::Error::from)?;
                    let Ok(name) = entry.file_name().to_str() else {
                        continue;
                    };
                    if name.is_empty() || name.starts_with('.') || scanner_junk(name) {
                        continue;
                    }
                    if root.directory(&format!("{physical}/{name}")).is_ok() {
                        children.entry(name.into()).or_default();
                    }
                }
            }
        }
        let mut folders: Vec<_> = children
            .into_iter()
            .map(|(name, (item_count, size))| TreeFolder {
                folder: if folder.is_empty() {
                    name.clone()
                } else {
                    format!("{folder}/{name}")
                },
                name,
                item_count,
                size,
            })
            .collect();
        folders.sort_by_key(|f| f.name.to_lowercase());
        Ok(DownloadTree {
            folder: folder.into(),
            folders,
        })
    }
}

fn scanner_junk(name: &str) -> bool {
    super::scanner::is_junk_name(name)
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateDownloadFolder {
    pub key: Option<String>,
    #[serde(default)]
    pub parent: String,
    pub name: String,
}
impl Library {
    pub async fn create_download_directory(
        &self,
        id: &str,
        body: &CreateDownloadFolder,
    ) -> Result<String> {
        folders::validate_folder(&body.parent)?;
        folders::validate_folder(&body.name)?;
        if body.name.is_empty() || body.name.contains('/') {
            return Err(CoreError::invalid("Enter one folder name."));
        }
        let _moves = self.lock_moves().await?;
        let def = defs::load_one(&self.db, id)
            .await?
            .ok_or(CoreError::NotFound)?;
        let groups = self.download_folders(id).await?;
        let key = body
            .key
            .clone()
            .unwrap_or_else(|| format!("custom:{}", uuid::Uuid::now_v7()));
        let target = if let Some(group) = &body.key {
            if !groups.iter().any(|g| g.key == *group) {
                return Err(CoreError::NotFound);
            }
            if base(&self.db, id, group).await?.is_empty() && body.parent.is_empty() {
                return Err(CoreError::conflict(
                    "These files do not share a physical download folder. Organize them together first.",
                ));
            }
            format!(
                "{}{}",
                prefix(&self.db, id, group, &body.parent).await?,
                body.name
            )
        } else {
            if !body.parent.is_empty() {
                return Err(CoreError::invalid(
                    "Choose a download before creating a subfolder.",
                ));
            }
            if groups
                .iter()
                .any(|g| g.name.to_lowercase() == body.name.to_lowercase())
            {
                return Err(CoreError::conflict("Another folder has this name."));
            }
            format!("{}/{}", def.folder(), body.name)
        };
        let root = super::cleanup::Root::open(self.layout.root())?;
        root.mkdir(&target)?;
        if body.key.is_none() {
            let result = sqlx::query(
                "INSERT INTO library_download_folders(library_id,group_key,name) VALUES(?,?,?)",
            )
            .bind(id)
            .bind(&key)
            .bind(&body.name)
            .execute(&self.db)
            .await;
            if let Err(e) = result {
                root.remove_empty_organized_dir(&target)?;
                return Err(e.into());
            }
        }
        root.directory(&target)?.sync_all()?;
        super::flatten_downloads::recovery::sync_parent(&root, &target)?;
        self.events.publish(crate::events::Event::Libraries);
        Ok(key)
    }
}
