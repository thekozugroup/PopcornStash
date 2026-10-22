//! A flat presentation of imported videos by download. No filesystem mutations.
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{Library, defs, folders};
use crate::events::{Event, LibraryChange};
use crate::{CoreError, Result};

/// Shared by the group list and cursor-paged item query. Only memberships in the
/// item's current library apply; moving a file to another library cannot leak a label.
pub const GROUP_KEY: &str = "COALESCE((SELECT m.group_key FROM library_download_members m WHERE m.item_id=i.id AND m.library_id=i.library_id), CASE WHEN i.source_download_id IS NOT NULL AND i.source_download_id<>'' THEN 'download:'||i.source_download_id WHEN instr(substr(i.rel_path,length(l.folder)+2),'/')>0 THEN 'folder:'||substr(substr(i.rel_path,length(l.folder)+2),1,instr(substr(i.rel_path,length(l.folder)+2),'/')-1) ELSE 'loose' END)";

#[derive(Debug, Serialize, FromRow)]
pub struct DownloadFolder {
    pub key: String,
    pub name: String,
    pub item_count: i64,
    pub size: i64,
    pub added_at: i64,
    pub custom: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameFolder {
    /// Absent creates an empty presentation folder. Present renames a group.
    pub key: Option<String>,
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveToFolder {
    pub key: String,
    pub item_ids: Vec<String>,
}

pub fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > 1600 || key.chars().any(char::is_control) {
        return Err(CoreError::invalid(
            "Choose a download folder from this library.",
        ));
    }
    Ok(())
}

impl Library {
    pub async fn download_folders(&self, id: &str) -> Result<Vec<DownloadFolder>> {
        defs::load_one(&self.db, id)
            .await?
            .ok_or(CoreError::NotFound)?;
        let sql = format!(
            "WITH grouped AS (SELECT {GROUP_KEY} AS group_key, COALESCE(o.name, CASE WHEN instr(substr(i.rel_path,length(l.folder)+2),'/')>0 THEN substr(substr(i.rel_path,length(l.folder)+2),1,instr(substr(i.rel_path,length(l.folder)+2),'/')-1) ELSE 'Loose videos' END) AS source_name, i.size,i.added_at FROM items i JOIN libraries l ON l.id=i.library_id LEFT JOIN download_origins o ON o.id=i.source_download_id WHERE i.library_id=? AND i.status='ready' AND i.kind='video'), stats AS (SELECT group_key,MIN(source_name) AS source_name,COUNT(*) AS item_count,SUM(size) AS size,MAX(added_at) AS added_at FROM grouped GROUP BY group_key), keys AS (SELECT group_key FROM stats UNION SELECT group_key FROM library_download_folders WHERE library_id=?) SELECT k.group_key AS key, COALESCE(f.name,s.source_name,'Download') AS name,COALESCE(s.item_count,0) AS item_count,COALESCE(s.size,0) AS size,COALESCE(s.added_at,0) AS added_at,f.group_key IS NOT NULL AS custom FROM keys k LEFT JOIN stats s ON s.group_key=k.group_key LEFT JOIN library_download_folders f ON f.library_id=? AND f.group_key=k.group_key ORDER BY name COLLATE NOCASE,k.group_key LIMIT 5001"
        );
        let rows = sqlx::query_as::<_, DownloadFolder>(&sql)
            .bind(id)
            .bind(id)
            .bind(id)
            .fetch_all(&self.db)
            .await?;
        if rows.len() > 5000 {
            return Err(CoreError::invalid(
                "This library has more than 5,000 download folders. Use the all-videos view.",
            ));
        }
        Ok(rows)
    }

    pub async fn name_download_folder(&self, id: &str, body: &NameFolder) -> Result<String> {
        folders::validate_folder(&body.name)?;
        if body.name.is_empty() || body.name.contains('/') {
            return Err(CoreError::invalid("Enter one folder name."));
        }
        let _moves = self.lock_moves().await?;
        let groups = self.download_folders(id).await?;
        if body
            .key
            .as_ref()
            .is_some_and(|key| !groups.iter().any(|g| &g.key == key))
        {
            return Err(CoreError::NotFound);
        }
        if groups.iter().any(|g| {
            g.name.to_lowercase() == body.name.to_lowercase() && Some(&g.key) != body.key.as_ref()
        }) {
            return Err(CoreError::conflict("Another folder has this name."));
        }
        let key = body
            .key
            .clone()
            .unwrap_or_else(|| format!("custom:{}", uuid::Uuid::now_v7()));
        sqlx::query("INSERT INTO library_download_folders(library_id,group_key,name) VALUES(?,?,?) ON CONFLICT(library_id,group_key) DO UPDATE SET name=excluded.name")
            .bind(id).bind(&key).bind(&body.name).execute(&self.db).await?;
        self.events.publish(Event::Libraries);
        Ok(key)
    }

    pub async fn move_to_download_folder(&self, id: &str, body: &MoveToFolder) -> Result<()> {
        folders::validate_ids(&body.item_ids)?;
        validate_key(&body.key)?;
        let _moves = self.lock_moves().await?;
        let groups = self.download_folders(id).await?;
        let group = groups
            .iter()
            .find(|g| g.key == body.key)
            .ok_or(CoreError::NotFound)?;
        let mut tx = crate::db::begin_write(&self.db).await?;
        // Freeze a label for a destination even when every original member later moves.
        sqlx::query("INSERT OR IGNORE INTO library_download_folders(library_id,group_key,name) VALUES(?,?,?)").bind(id).bind(&body.key).bind(&group.name).execute(&mut *tx).await?;
        for item in &body.item_ids {
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM items WHERE id=? AND library_id=? AND status='ready' AND kind='video')").bind(item).bind(id).fetch_one(&mut *tx).await?;
            if !valid {
                return Err(CoreError::conflict(
                    "A selected video moved or is unavailable. Refresh and select it again.",
                ));
            }
            sqlx::query("INSERT INTO library_download_members(item_id,library_id,group_key) VALUES(?,?,?) ON CONFLICT(item_id) DO UPDATE SET library_id=excluded.library_id,group_key=excluded.group_key").bind(item).bind(id).bind(&body.key).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        for item in &body.item_ids {
            self.events.publish(Event::Library {
                r#type: LibraryChange::Updated,
                item_id: item.clone(),
                before: None,
            });
        }
        self.events.publish(Event::Libraries);
        Ok(())
    }
}
