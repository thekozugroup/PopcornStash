//! A flat presentation of imported videos by download. No filesystem mutations.
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{Library, defs, folders};
use crate::events::{Event, LibraryChange};
use crate::{CoreError, Result};

/// Shared by the group list and cursor-paged item query. Only memberships in the
/// item's current library apply; moving a file to another library cannot leak a label.
pub const GROUP_KEY: &str = "COALESCE((SELECT m.group_key FROM library_download_members m WHERE m.item_id=i.id AND m.library_id=i.library_id), CASE WHEN i.source_download_id IS NOT NULL AND i.source_download_id<>'' THEN 'download:'||i.source_download_id WHEN instr(substr(i.rel_path,length(l.folder)+2),'/')>0 THEN 'folder:'||substr(substr(i.rel_path,length(l.folder)+2),1,instr(substr(i.rel_path,length(l.folder)+2),'/')-1) ELSE 'loose' END)";

/// A physical name is meaningful only when the entire group occupies one unshared
/// directory. Include missing members in the decision so a partial scan cannot
/// temporarily turn a scattered collection into a physical one.
fn group_directories() -> String {
    format!("WITH members AS (
        SELECT i.id, i.size, i.added_at, i.status, i.kind, {GROUP_KEY} AS group_key,
               o.name AS origin, substr(i.rel_path,length(l.folder)+2) AS path
        FROM items i JOIN libraries l ON l.id=i.library_id
        LEFT JOIN download_origins o ON o.id=i.source_download_id WHERE i.library_id=?1
    ), paths AS (
        SELECT *, CASE WHEN instr(path,'/')>0 THEN substr(path,1,instr(path,'/')-1) ELSE '' END AS root
        FROM members
    ), exclusive_roots AS (
        SELECT root, min(group_key) AS group_key FROM paths WHERE root<>''
        GROUP BY root HAVING min(group_key)=max(group_key)
    ), directories AS (
        SELECT p.group_key, min(p.root) AS root, min(p.id) AS item_id
        FROM paths p LEFT JOIN exclusive_roots e ON e.root=p.root AND e.group_key=p.group_key
        GROUP BY p.group_key HAVING count(e.root)=count(*) AND min(p.root)=max(p.root)
          AND min(p.status='ready')=1
    )")
}

pub(super) async fn physical_group(lib: &Library, id: &str, root: &str) -> Result<Option<String>> {
    let sql = format!(
        "{} SELECT DISTINCT p.group_key,d.root IS NOT NULL
        FROM paths p LEFT JOIN directories d ON d.group_key=p.group_key AND d.root=p.root
        WHERE p.root=?2 LIMIT 2",
        group_directories()
    );
    let matches: Vec<(String, bool)> = sqlx::query_as(&sql)
        .bind(id)
        .bind(root)
        .fetch_all(&lib.db)
        .await?;
    match matches.as_slice() {
        [] | [(_, false)] => Ok(None),
        [(key, true)] => Ok(Some(key.clone())),
        _ => Err(CoreError::conflict(
            "Several collections share this directory. Choose an unambiguous destination.",
        )),
    }
}

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

/// Repair an import's persisted destination as well as its shelf title. A legacy
/// label alone is not evidence of an on-disk name. Accept a witnessed rename
/// or the importer's original (possibly collision-numbered) source name.
pub(super) async fn reconcile_directories(lib: &Library, library: &defs::LibraryDef) -> Result<()> {
    let sql = format!(
        "{}
        SELECT f.group_key,f.name,d.root,d.item_id,f.follows_directory,o.name AS origin
        FROM library_download_folders f JOIN directories d USING(group_key)
        LEFT JOIN download_origins o ON substr(f.group_key,1,9)='download:' AND o.id=substr(f.group_key,10)
        WHERE f.library_id=?1 AND f.name<>d.root AND f.follows_directory IS NOT 0
        AND NOT EXISTS (SELECT 1 FROM library_download_folders other WHERE other.library_id=?1
            AND other.group_key<>f.group_key AND other.name=d.root COLLATE NOCASE)",
        group_directories()
    );
    #[derive(sqlx::FromRow)]
    struct Candidate {
        group_key: String,
        name: String,
        root: String,
        item_id: String,
        follows_directory: Option<bool>,
        origin: Option<String>,
    }
    let candidates: Vec<Candidate> = sqlx::query_as(&sql)
        .bind(library.id())
        .fetch_all(&lib.db)
        .await?;
    for c in candidates {
        if c.follows_directory != Some(true)
            && !c.origin.as_deref().is_some_and(|origin| {
                c.name == origin
                    || c.name
                        .strip_prefix(origin)
                        .and_then(|s| s.strip_prefix(" ("))
                        .and_then(|s| s.strip_suffix(')'))
                        .and_then(|s| s.parse::<u32>().ok())
                        .is_some_and(|n| n >= 2)
            })
        {
            continue;
        }
        if !super::observed::renamed_directory(lib, library.folder(), &c.name, &c.root)? {
            continue;
        }
        sqlx::query("UPDATE library_download_folders SET name=?,follows_directory=1 WHERE library_id=? AND group_key=? AND name=?")
            .bind(c.root).bind(library.id()).bind(c.group_key).bind(c.name).execute(&lib.db).await?;
        super::scanner::publish(lib, LibraryChange::Updated, &c.item_id);
        lib.events.publish(Event::Libraries);
    }
    Ok(())
}

impl Library {
    pub async fn download_folders(&self, id: &str) -> Result<Vec<DownloadFolder>> {
        defs::load_one(&self.db, id)
            .await?
            .ok_or(CoreError::NotFound)?;
        let sql = format!(
            "{}, stats AS (
                SELECT group_key, min(coalesce(origin,nullif(root,''),'Loose videos')) AS fallback,
                       count(*) AS item_count, sum(size) AS size, max(added_at) AS added_at
                FROM paths WHERE status='ready' AND kind='video' GROUP BY group_key
            ), keys AS (
                SELECT group_key FROM stats UNION SELECT group_key FROM library_download_folders WHERE library_id=?1
            )
            SELECT k.group_key AS key, coalesce(f.name,d.root,s.fallback,'Download') AS name,
                   coalesce(s.item_count,0) AS item_count, coalesce(s.size,0) AS size,
                   coalesce(s.added_at,0) AS added_at, f.group_key IS NOT NULL AS custom
            FROM keys k LEFT JOIN stats s USING(group_key) LEFT JOIN directories d USING(group_key)
            LEFT JOIN library_download_folders f ON f.library_id=?1 AND f.group_key=k.group_key
            ORDER BY name COLLATE NOCASE,k.group_key LIMIT 5001", group_directories()
        );
        let rows = sqlx::query_as::<_, DownloadFolder>(&sql)
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
        sqlx::query("INSERT INTO library_download_folders(library_id,group_key,name,follows_directory) VALUES(?,?,?,0) ON CONFLICT(library_id,group_key) DO UPDATE SET name=excluded.name,follows_directory=0")
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
        sqlx::query("INSERT OR IGNORE INTO library_download_folders(library_id,group_key,name,follows_directory) VALUES(?,?,?,0)").bind(id).bind(&body.key).bind(&group.name).execute(&mut *tx).await?;
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
