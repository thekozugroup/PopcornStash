//! Read-only filesystem observations. A library binding can follow a directory
//! only within the same stash, with both directory and indexed file evidence.
use std::collections::HashMap;
use std::fs::{File, Metadata};
use std::os::unix::fs::MetadataExt;

use super::{Library, cleanup::Root, defs, folders, scanner};
use crate::{
    Result,
    events::{Event, LibraryChange},
};

pub(super) fn identity(meta: &Metadata) -> (i64, i64) {
    (meta.dev() as i64, meta.ino() as i64)
}

// Enumerate the descriptor, not a path through a possibly replaced parent.
// Include non-directories: a dangling link or a file at the old name blocks adoption.
fn names(dir: File) -> std::io::Result<Vec<String>> {
    let mut listing = nix::dir::Dir::from_fd(dir.into())?;
    let mut out = Vec::new();
    for entry in listing.iter() {
        let entry = entry?;
        if let Ok(name) = entry.file_name().to_str()
            && name != "."
            && name != ".."
        {
            out.push(name.to_owned());
        }
    }
    Ok(out)
}

pub(super) fn renamed_directory(lib: &Library, parent: &str, old: &str, new: &str) -> Result<bool> {
    if old == new
        || [old, new]
            .iter()
            .any(|n| n.is_empty() || n.contains('/') || folders::validate_folder(n).is_err())
    {
        return Ok(false);
    }
    let root = Root::open(lib.layout.root())?;
    let Ok(dir) = root.directory(parent) else {
        return Ok(false);
    };
    let entries = names(dir)?;
    Ok(!entries.iter().any(|n| n == old)
        && entries.iter().any(|n| n == new)
        && root.directory(&format!("{parent}/{new}")).is_ok())
}

/// A copy found elsewhere cannot claim a private item's identity while its
/// source is offline or an unrelated directory occupies the original mount point.
pub(super) async fn source_available(lib: &Library, id: &str, folder: &str) -> Result<bool> {
    if defs::folder_state(&lib.layout, folder, true).await == crate::models::FolderState::Missing {
        return Ok(false);
    }
    let anchor: Option<(i64, i64)> =
        sqlx::query_as("SELECT dev,ino FROM library_root_identity WHERE library_id=?")
            .bind(id)
            .fetch_optional(&lib.db)
            .await?;
    let Some(expected) = anchor else {
        return Ok(true);
    };
    Ok(std::fs::symlink_metadata(lib.layout.root().join(folder))
        .is_ok_and(|meta| meta.is_dir() && identity(&meta) == expected))
}

/// Called under the move lock, before indexing a snapshot or a batch of notifications.
pub(super) async fn roots(lib: &Library) -> Result<()> {
    let root = Root::open(lib.layout.root())?;
    let parent = File::open(lib.layout.root())?;
    let parent_id = identity(&parent.metadata()?);
    let entries = names(parent)?;
    let libraries = defs::load_all(&lib.db).await?;
    let anchors: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT library_id,dev,ino FROM library_root_identity WHERE parent_dev=? AND parent_ino=?",
    )
    .bind(parent_id.0)
    .bind(parent_id.1)
    .fetch_all(&lib.db)
    .await?;
    let needs_lookup = libraries
        .iter()
        .any(|d| !entries.iter().any(|name| name == d.folder()));
    let mut candidates = HashMap::<_, Vec<&str>>::new();
    for name in entries.iter().filter(|_| needs_lookup) {
        if defs::check_folder_name(name).ok().as_ref() != Some(name) {
            continue;
        }
        if let Ok(dir) = root.directory(name) {
            let id = identity(&dir.metadata()?);
            if id.0 == parent_id.0 && id.1 != 0 {
                candidates.entry(id).or_default().push(name);
            }
        }
    }
    for libdef in &libraries {
        let old = libdef.folder();
        if entries.iter().any(|n| n == old) {
            if !anchors.iter().any(|(key, _, _)| key == libdef.id())
                && let Ok(dir) = root.directory(old)
            {
                let id = identity(&dir.metadata()?);
                if id.0 == parent_id.0 && id.1 != 0 {
                    sqlx::query("INSERT OR IGNORE INTO library_root_identity(library_id,parent_dev,parent_ino,dev,ino) VALUES(?,?,?,?,?)")
                        .bind(libdef.id()).bind(parent_id.0).bind(parent_id.1).bind(id.0).bind(id.1).execute(&lib.db).await?;
                }
            }
            continue;
        }
        let Some((_, dev, ino)) = anchors.iter().find(|(key, _, _)| key == libdef.id()) else {
            continue;
        };
        let id = (*dev, *ino);
        if anchors
            .iter()
            .any(|(key, dev, ino)| key != libdef.id() && (*dev, *ino) == id)
        {
            continue;
        }
        let Some(&[new]) = candidates.get(&id).map(Vec::as_slice) else {
            continue;
        };
        let folded = new.to_lowercase();
        if entries
            .iter()
            .any(|entry| entry != new && entry.to_lowercase() == folded)
            || libraries
                .iter()
                .any(|d| d.id() != libdef.id() && d.folder().to_lowercase() == folded)
        {
            continue;
        }
        rebind(lib, &root, libdef, new, id).await?;
    }
    Ok(())
}

async fn rebind(
    lib: &Library,
    root: &Root,
    def: &defs::LibraryDef,
    new: &str,
    dir_id: (i64, i64),
) -> Result<()> {
    let old = def.folder();
    let old_prefix = format!("{old}/");
    let new_prefix = format!("{new}/");
    let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM items WHERE substr(rel_path,1,length(?1))=?1)
        OR EXISTS(SELECT 1 FROM conversion_jobs WHERE library_id=?2 AND (stage IS NOT NULL OR state IN ('encoding','verifying','publishing')))")
        .bind(&new_prefix).bind(def.id()).fetch_one(&lib.db).await?;
    if blocked {
        return Ok(());
    }
    // At least one file at its original relative location must retain its inode,
    // size and indexed content fingerprint. Copies, symlinks and hard links do
    // not establish identity. A library without a prior witness stays missing.
    #[derive(sqlx::FromRow)]
    struct Witness {
        rel_path: String,
        file_dev: i64,
        file_ino: i64,
        size: i64,
        quick_hash: String,
    }
    let rows: Vec<Witness> = sqlx::query_as(
        "SELECT rel_path,file_dev,file_ino,size,quick_hash FROM items WHERE library_id=?
         AND status IN ('ready','missing') AND file_ino IS NOT NULL AND quick_hash IS NOT NULL ORDER BY id"
    ).bind(def.id()).fetch_all(&lib.db).await?;
    let mut verified = false;
    for Witness {
        rel_path: path,
        file_dev: dev,
        file_ino: ino,
        size,
        quick_hash: hash,
    } in rows
    {
        let Some(tail) = path.strip_prefix(&old_prefix) else {
            continue;
        };
        let Ok(file) = root.file(&format!("{new_prefix}{tail}")) else {
            continue;
        };
        let meta = file.metadata()?;
        if identity(&meta) != (dev, ino) || meta.nlink() != 1 || meta.len() as i64 != size {
            continue;
        }
        if tokio::task::spawn_blocking(move || scanner::hash_file(file))
            .await
            .ok()
            .and_then(|result| result.ok())
            .as_ref()
            == Some(&hash)
        {
            verified = true;
            break;
        }
    }
    if !verified {
        return Ok(());
    }
    let mut tx = crate::db::begin_write(&lib.db).await?;
    // Re-read the exact spelling as well (case-only renames on insensitive shares).
    if names(File::open(lib.layout.root())?)?
        .iter()
        .any(|n| n == old)
        || !root
            .directory(new)
            .and_then(|d| Ok(identity(&d.metadata()?)))
            .is_ok_and(|id| id == dir_id)
    {
        return Ok(());
    }
    sqlx::query("UPDATE libraries SET folder=?,updated_at=? WHERE id=?")
        .bind(new)
        .bind(crate::now())
        .bind(def.id())
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE conversion_jobs SET target_path=?2||substr(target_path,length(?1)+1)
        WHERE state='complete' AND substr(target_path,1,length(?1))=?1 AND item_id IN (SELECT id FROM items WHERE library_id=?3 AND rel_path=conversion_jobs.target_path)")
        .bind(&old_prefix).bind(&new_prefix).bind(def.id()).execute(&mut *tx).await?;
    let changed: Vec<String> = sqlx::query_scalar(
        "UPDATE items SET rel_path=?2||substr(rel_path,length(?1)+1),updated_at=?4
         WHERE library_id=?3 AND substr(rel_path,1,length(?1))=?1 RETURNING id",
    )
    .bind(&old_prefix)
    .bind(&new_prefix)
    .bind(def.id())
    .bind(crate::now())
    .fetch_all(&mut *tx)
    .await?;
    for table in ["trash", "file_trash", "file_trash_items"] {
        sqlx::query(&format!("UPDATE {table} SET original_rel_path=?2||substr(original_rel_path,length(?1)+1) WHERE substr(original_rel_path,1,length(?1))=?1"))
            .bind(&old_prefix).bind(&new_prefix).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    lib.bump_generation();
    for item in changed {
        scanner::publish(lib, LibraryChange::Updated, &item);
    }
    lib.events.publish(Event::Libraries);
    Ok(())
}

/// On case-insensitive filesystems a lookup by the old spelling can still
/// resolve. Only an absent exact directory entry proves that spelling vanished.
pub(super) fn changed_spelling(lib: &Library, old: &str, new: &str) -> bool {
    let Ok(root) = Root::open(lib.layout.root()) else {
        return false;
    };
    let mut parent = String::new();
    for (a, b) in old.split('/').zip(new.split('/')) {
        if a != b {
            let dir = if parent.is_empty() {
                File::open(lib.layout.root()).ok()
            } else {
                root.directory(&parent).ok()
            };
            return dir.and_then(|dir| names(dir).ok()).is_some_and(|entries| {
                entries.iter().any(|n| n == b) && !entries.iter().any(|n| n == a)
            });
        }
        if !parent.is_empty() {
            parent.push('/');
        }
        parent.push_str(a);
    }
    false
}

pub(super) async fn remember_collection_move(
    conn: &mut sqlx::SqliteConnection,
    lib: &Library,
    item: &crate::models::ItemRow,
    new: &str,
) -> Result<()> {
    fn parts(p: &str) -> Option<(&str, &str, &str)> {
        let (library, rest) = p.split_once('/')?;
        let (folder, tail) = rest.split_once('/')?;
        Some((library, folder, tail))
    }
    let (Some((library, old, tail)), Some((new_library, new_folder, new_tail))) =
        (parts(&item.rel_path), parts(new))
    else {
        return Ok(());
    };
    if library != new_library || tail != new_tail || old == new_folder || item.library_id.is_none()
    {
        return Ok(());
    }
    let sql = format!(
        "SELECT f.group_key FROM items i JOIN libraries l ON l.id=i.library_id
        JOIN library_download_folders f ON f.library_id=i.library_id AND f.group_key={}
        WHERE i.id=? AND f.follows_directory IS NULL AND f.name=?",
        super::download_folders::GROUP_KEY
    );
    let key: Option<String> = sqlx::query_scalar(&sql)
        .bind(&item.id)
        .bind(old)
        .fetch_optional(&mut *conn)
        .await?;
    if let Some(key) = key
        && renamed_directory(lib, library, old, new_folder)?
    {
        sqlx::query("UPDATE library_download_folders SET follows_directory=1 WHERE library_id=? AND group_key=?")
            .bind(&item.library_id).bind(key).execute(conn).await?;
    }
    Ok(())
}
