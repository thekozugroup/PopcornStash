//! Previewed folder renames using exclusive file moves, including on shfs.
//! Originals, unindexed companions and empty subdirectories are all retained.
use super::{
    Library,
    cleanup::{Root, fingerprint},
    defs,
    download_folders::GROUP_KEY,
    download_tree, folders, scanner,
};
use crate::{CoreError, Result, events::Event};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameRequest {
    pub key: String,
    #[serde(default)]
    pub folder: String,
    pub name: String,
    pub token: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct RenamePreview {
    pub from: String,
    pub to: String,
    pub file_count: usize,
    pub folder_count: usize,
    pub blocked: Vec<String>,
    pub token: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    from: String,
    to: String,
    identity: [u64; 5],
    fingerprint: String,
    mode: u32,
}
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct TreeJournal {
    pub tree_version: u8,
    id: String,
    library: String,
    key: String,
    name: Option<String>,
    files: Vec<Entry>,
    dirs: Vec<Entry>,
    targets: Vec<[u64; 2]>,
    items: Vec<(String, String, String)>,
}
fn identity(m: &std::fs::Metadata) -> [u64; 5] {
    [
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime() as u64,
        m.mtime_nsec() as u64,
    ]
}
fn exists(root: &Root, p: &str) -> Result<bool> {
    match root.metadata(p) {
        Ok(_) => Ok(true),
        Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
fn check(root: &Root, p: &str, id: [u64; 5]) -> Result<()> {
    if identity(&root.metadata(p)?) != id {
        return Err(CoreError::conflict(
            "A file changed during the rename. Recovery kept every copy.",
        ));
    }
    Ok(())
}
fn sync(root: &Root, p: &str) -> Result<()> {
    super::flatten_downloads::recovery::sync_parent(root, p)
}
fn collect(
    root: &Root,
    from: &str,
    to: &str,
    files: &mut Vec<Entry>,
    dirs: &mut Vec<Entry>,
) -> Result<()> {
    if files.len() + dirs.len() > 5000 || from.split('/').count() > 32 {
        return Err(CoreError::invalid(
            "Rename a smaller folder (up to 5,000 entries and 32 levels).",
        ));
    }
    let m = root.directory(from)?.metadata()?;
    dirs.push(Entry {
        from: from.into(),
        to: to.into(),
        identity: identity(&m),
        fingerprint: fingerprint(&m),
        mode: m.mode() & 0o7777,
    });
    let fd = root.directory(from)?;
    let mut entries = nix::dir::Dir::from_fd(fd.into()).map_err(std::io::Error::from)?;
    let mut names = Vec::new();
    for entry in entries.iter() {
        let entry = entry.map_err(std::io::Error::from)?;
        let n = entry.file_name().to_str().map_err(|_| {
            CoreError::invalid("This folder contains a name that cannot be organized safely.")
        })?;
        if n == "." || n == ".." {
            continue;
        }
        names.push(n.to_string());
        if names.len() + files.len() + dirs.len() > 5000 {
            return Err(CoreError::invalid(
                "Rename a smaller folder (up to 5,000 entries).",
            ));
        }
    }
    names.sort();
    for n in names {
        let src = format!("{from}/{n}");
        let dst = format!("{to}/{n}");
        let meta = root.metadata(&src)?;
        if meta.is_dir() {
            collect(root, &src, &dst, files, dirs)?;
        } else if meta.is_file() && meta.nlink() == 1 {
            files.push(Entry {
                from: src,
                to: dst,
                identity: identity(&meta),
                fingerprint: fingerprint(&meta),
                mode: meta.mode() & 0o7777,
            });
        } else {
            return Err(CoreError::conflict(
                "This folder contains a link or special file. It was kept in place.",
            ));
        }
    }
    if files.len() + dirs.len() > 5000 {
        return Err(CoreError::invalid(
            "Rename a smaller folder (up to 5,000 entries).",
        ));
    }
    Ok(())
}
impl Library {
    async fn rename_plan(
        &self,
        id: &str,
        body: &RenameRequest,
    ) -> Result<(RenamePreview, TreeJournal)> {
        folders::validate_folder(&body.folder)?;
        folders::validate_folder(&body.name)?;
        if body.name.is_empty() || body.name.contains('/') {
            return Err(CoreError::invalid("Enter one folder name."));
        }
        let groups = self.download_folders(id).await?;
        let group = groups
            .iter()
            .find(|g| g.key == body.key)
            .ok_or(CoreError::NotFound)?;
        let base = download_tree::base(&self.db, id, &body.key).await?;
        if base.is_empty() && body.folder.is_empty() {
            return Err(CoreError::conflict(
                "These videos do not share one physical download folder. Preview Flatten files to bring them together first.",
            ));
        }
        let def = defs::load_one(&self.db, id)
            .await?
            .ok_or(CoreError::NotFound)?;
        let from = download_tree::prefix(&self.db, id, &body.key, &body.folder)
            .await?
            .trim_end_matches('/')
            .to_string();
        let parent = Path::new(&from)
            .parent()
            .and_then(Path::to_str)
            .ok_or(CoreError::NotFound)?;
        let to = format!("{parent}/{}", body.name);
        let root = Root::open(self.layout.root())?;
        let mut plan = RenamePreview {
            from: from.clone(),
            to: to.clone(),
            file_count: 0,
            folder_count: 0,
            blocked: Vec::new(),
            token: String::new(),
        };
        if from == to {
            plan.blocked.push("Choose a different folder name.".into());
        }
        if exists(&root, &to)? {
            plan.blocked
                .push("A file or folder already uses this name. Choose another name.".into());
        }
        if body.folder.is_empty()
            && groups
                .iter()
                .any(|g| g.key != group.key && g.name.to_lowercase() == body.name.to_lowercase())
        {
            plan.blocked
                .push("Another download folder has this name.".into());
        }
        let mut journal = TreeJournal {
            tree_version: 1,
            id: String::new(),
            library: id.into(),
            key: body.key.clone(),
            name: if body.folder.is_empty() {
                Some(body.name.clone())
            } else {
                None
            },
            files: Vec::new(),
            dirs: Vec::new(),
            targets: Vec::new(),
            items: Vec::new(),
        };
        collect(&root, &from, &to, &mut journal.files, &mut journal.dirs)?;
        let prefix = format!("{from}/");
        let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT id,rel_path,library_id FROM items WHERE substr(rel_path,1,?)=? ORDER BY id",
        )
        .bind(prefix.chars().count() as i64)
        .bind(&prefix)
        .fetch_all(&self.db)
        .await?;
        for (item, path, library) in rows {
            if library.as_deref() != Some(id) {
                plan.blocked
                    .push("This folder contains an item outside this library.".into());
            }
            journal.items.push((
                item,
                path.clone(),
                format!("{to}/{}", &path[prefix.len()..]),
            ));
        }
        // A different presentation group may share the same physical directory.
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM items i JOIN libraries l ON l.id=i.library_id WHERE i.library_id=? AND i.status='ready' AND i.kind='video' AND substr(i.rel_path,1,?)=? AND {GROUP_KEY}<>?)"
        );
        if sqlx::query_scalar::<_, bool>(&sql)
            .bind(id)
            .bind(prefix.chars().count() as i64)
            .bind(&prefix)
            .bind(&body.key)
            .fetch_one(&self.db)
            .await?
        {
            plan.blocked.push("Another download shares this physical folder. Organize its files separately first.".into());
        }
        let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM conversion_jobs j JOIN items i ON i.id=j.item_id WHERE substr(i.rel_path,1,?)=? AND (j.stage IS NOT NULL OR j.state IN ('encoding','verifying','publishing'))) OR EXISTS(SELECT 1 FROM downloads d WHERE d.state IN ('queued','downloading','paused','seeding','sorting') AND (d.id=? OR EXISTS(SELECT 1 FROM items i WHERE substr(i.rel_path,1,?)=? AND (i.source_download_id=d.id OR EXISTS(SELECT 1 FROM download_items di WHERE di.item_id=i.id AND di.download_id=d.id)))))")
            .bind(prefix.chars().count() as i64).bind(&prefix).bind(body.key.strip_prefix("download:").unwrap_or("")).bind(prefix.chars().count() as i64).bind(&prefix).fetch_one(&self.db).await?;
        if busy {
            plan.blocked.push("Files here are downloading, seeding or converting. Rename after that work finishes.".into());
        }
        plan.file_count = journal.files.len();
        plan.folder_count = journal.dirs.len();
        plan.blocked.sort();
        plan.blocked.dedup();
        plan.token = hex::encode(Sha256::digest(
            serde_json::to_vec(&(&plan, &journal))
                .map_err(|e| CoreError::invalid(e.to_string()))?,
        ));
        // The root must stay inside the selected library, never rename the library itself.
        if !from.starts_with(&format!("{}/", def.folder())) {
            return Err(CoreError::NotFound);
        }
        Ok((plan, journal))
    }
    pub async fn preview_folder_rename(
        &self,
        id: &str,
        body: &RenameRequest,
    ) -> Result<RenamePreview> {
        let _moves = self.lock_moves().await?;
        Ok(self.rename_plan(id, body).await?.0)
    }
    pub async fn rename_download_directory(&self, id: &str, body: &RenameRequest) -> Result<()> {
        let _moves = self.lock_moves().await?;
        let (plan, mut journal) = self.rename_plan(id, body).await?;
        if body.token.as_deref() != Some(&plan.token) || !plan.blocked.is_empty() {
            return Err(CoreError::conflict(
                "The preview changed or has blocked files. Review it again before renaming.",
            ));
        }
        let group_name = self
            .download_folders(id)
            .await?
            .into_iter()
            .find(|g| g.key == body.key)
            .ok_or(CoreError::NotFound)?
            .name;
        let root = Root::open(self.layout.root())?;
        // Reserve the destination exclusively. Empty directories are prepared before
        // moving any file, matching the existing flatten publication contract.
        for d in &journal.dirs {
            if let Err(e) = root.mkdir(&d.to) {
                for (created, identity) in journal.dirs.iter().zip(&journal.targets).rev() {
                    remove_dir(&root, &created.to, *identity)?;
                }
                return Err(e);
            }
            let meta = root.directory(&d.to)?.metadata()?;
            journal.targets.push([meta.dev(), meta.ino()]);
            sync(&root, &d.to)?;
        }
        journal.id = uuid::Uuid::now_v7().to_string();
        let stage = root.new_trash_dir("flatten-tree")?;
        self.flatten_pending
            .store(true, std::sync::atomic::Ordering::SeqCst);
        root.write_flatten_journal(
            &stage,
            &serde_json::to_vec(&journal).map_err(|e| CoreError::invalid(e.to_string()))?,
        )?;
        use sqlx::Connection;
        let mut conn = self.db.acquire().await?;
        sqlx::query("PRAGMA synchronous=FULL")
            .execute(&mut *conn)
            .await?;
        let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
        let result:Result<()>=async {
            for (n,f) in journal.files.iter().enumerate(){
                if fingerprint(&root.metadata(&f.from)?)!=f.fingerprint{return Err(CoreError::conflict("A file changed. Refresh the preview."));}
                let private=format!("{stage}/file-{n}");root.rename_trash(&f.from,&private)?;sync(&root,&f.from)?;sync(&root,&private)?;check(&root,&private,f.identity)?;
            }
            let mut remaining=Vec::new();let mut dirs=Vec::new();collect(&root,&plan.from,&plan.to,&mut remaining,&mut dirs)?;
            if !remaining.is_empty()||dirs.iter().map(|d|(&d.from,d.identity[0],d.identity[1])).collect::<Vec<_>>()!=journal.dirs.iter().map(|d|(&d.from,d.identity[0],d.identity[1])).collect::<Vec<_>>(){return Err(CoreError::conflict("The source folder changed during the rename. Files were kept."));}
            for (n,f) in journal.files.iter().enumerate(){let private=format!("{stage}/file-{n}");root.rename_trash(&private,&f.to)?;sync(&root,&f.to)?;sync(&root,&private)?;check(&root,&f.to,f.identity)?;}
            // Memberships reference the saved group, including previously derived groups.
            sqlx::query("INSERT INTO library_download_folders(library_id,group_key,name) VALUES(?,?,?) ON CONFLICT(library_id,group_key) DO UPDATE SET name=excluded.name")
                .bind(id).bind(&body.key).bind(journal.name.as_deref().unwrap_or(&group_name)).execute(&mut *tx).await?;
            for (item,from,to) in &journal.items {
                let changed=sqlx::query("UPDATE items SET rel_path=? WHERE id=? AND library_id=? AND rel_path=?").bind(to).bind(item).bind(id).bind(from).execute(&mut *tx).await?.rows_affected();
                if changed!=1{return Err(CoreError::conflict("An item changed. The rename was not committed."));}
                sqlx::query("UPDATE conversion_jobs SET target_path=? WHERE item_id=? AND target_path=? AND state='complete'").bind(to).bind(item).bind(from).execute(&mut *tx).await?;
                sqlx::query("INSERT INTO library_download_members(item_id,library_id,group_key) VALUES(?,?,?) ON CONFLICT(item_id) DO UPDATE SET library_id=excluded.library_id,group_key=excluded.group_key").bind(item).bind(id).bind(&body.key).execute(&mut *tx).await?;
            }
            sqlx::query("INSERT INTO library_folder_edits(id,library_id) VALUES(?,?)").bind(&journal.id).bind(id).execute(&mut *tx).await?;
            tx.commit().await?;Ok(())
        }.await;
        drop(conn);
        recover(self, &root, &stage, &journal).await?;
        self.recover_flattened_files_locked().await?;
        result?;
        for (item, _, _) in &journal.items {
            self.events.publish(Event::Library {
                r#type: crate::events::LibraryChange::Updated,
                item_id: item.clone(),
                before: None,
            });
        }
        self.events.publish(Event::Libraries);
        Ok(())
    }
}
fn remove_dir(root: &Root, path: &str, id: [u64; 2]) -> Result<()> {
    if !exists(root, path)? {
        return Ok(());
    }
    let m = root.directory(path)?.metadata()?;
    if [m.dev(), m.ino()] != id {
        return Err(CoreError::conflict(
            "A folder changed. Recovery kept it in place.",
        ));
    }
    root.remove_empty_organized_dir(path)
}
pub(super) async fn recover(
    lib: &Library,
    root: &Root,
    stage: &str,
    j: &TreeJournal,
) -> Result<()> {
    let def = defs::load_one(&lib.db, &j.library)
        .await?
        .ok_or(CoreError::NotFound)?;
    if j.tree_version != 1
        || j.files.len() + j.dirs.len() > 5000
        || j.dirs.is_empty()
        || j.dirs.len() != j.targets.len()
    {
        return Err(CoreError::invalid(
            "Invalid folder rename journal. Files were kept.",
        ));
    }
    for e in j.files.iter().chain(&j.dirs) {
        for p in [&e.from, &e.to] {
            if scanner::normalize_rel(p)? != *p || !p.starts_with(&format!("{}/", def.folder())) {
                return Err(CoreError::invalid(
                    "Folder rename recovery stays inside its library.",
                ));
            }
        }
    }
    let committed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM library_folder_edits WHERE id=? AND library_id=?)",
    )
    .bind(&j.id)
    .bind(&j.library)
    .fetch_one(&lib.db)
    .await?;
    // Check every reference before removing a duplicate or moving any file back.
    for (n, f) in j.files.iter().enumerate() {
        let private = format!("{stage}/file-{n}");
        let mut found = false;
        for p in [&f.from, &f.to, &private] {
            if exists(root, p)? {
                check(root, p, f.identity)?;
                found = true;
            }
        }
        if !found || (committed && !exists(root, &f.to)?) {
            return Err(CoreError::conflict(
                "Recovery could not verify a renamed file. Every copy was kept.",
            ));
        }
    }
    for (item, from, to) in &j.items {
        let path: Option<String> =
            sqlx::query_scalar("SELECT rel_path FROM items WHERE id=? AND library_id=?")
                .bind(item)
                .bind(&j.library)
                .fetch_optional(&lib.db)
                .await?;
        if path.as_deref() != Some(if committed { to } else { from }) {
            return Err(CoreError::conflict(
                "An item changed during folder recovery. Every copy was kept.",
            ));
        }
    }
    for (n, f) in j.files.iter().enumerate() {
        let private = format!("{stage}/file-{n}");
        if !committed {
            if !exists(root, &f.from)? {
                if !exists(root, &private)? {
                    root.rename_trash(&f.to, &private)?;
                    sync(root, &f.to)?;
                    sync(root, &private)?;
                }
                root.rename_trash(&private, &f.from)?;
                sync(root, &f.from)?;
                sync(root, &private)?;
            }
            if exists(root, &f.to)? {
                if exists(root, &private)? {
                    root.remove_flatten_leaf(&private)?;
                }
                root.rename_trash(&f.to, &private)?;
                sync(root, &f.to)?;
            }
            check(root, &f.from, f.identity)?;
        }
        if exists(root, &private)? {
            check(root, &private, f.identity)?;
            root.remove_flatten_leaf(&private)?;
        }
    }
    for (d, target) in j.dirs.iter().zip(&j.targets).rev() {
        if committed {
            let fd = root.directory(&d.to)?;
            let m = fd.metadata()?;
            if [m.dev(), m.ino()] != *target {
                return Err(CoreError::conflict(
                    "A destination folder changed. It was kept.",
                ));
            }
            fd.set_permissions(std::fs::Permissions::from_mode(d.mode))?;
            fd.sync_all()?;
            remove_dir(root, &d.from, [d.identity[0], d.identity[1]])?;
        } else {
            remove_dir(root, &d.to, *target)?;
        }
    }
    root.remove_flatten_leaf(&format!("{stage}/preparing.json"))?;
    root.remove_flatten_leaf(&format!("{stage}/journal.json"))?;
    root.remove_flatten_dir(stage)?;
    sqlx::query("DELETE FROM library_folder_edits WHERE id=?")
        .bind(&j.id)
        .execute(&lib.db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::{
        config::{Config, HwAccel},
        events::EventBus,
        jobs::JobQueue,
        media::MediaService,
        paths::StashLayout,
        settings::SettingsStore,
    };
    use std::sync::Arc;
    #[tokio::test]
    async fn tree_rename_recovers_every_interruption_and_keeps_unexpected_files() {
        for cut in 0..5 {
            let temp = tempfile::tempdir().unwrap();
            let root_path = temp.path().join("stash");
            std::fs::create_dir(&root_path).unwrap();
            let config = Arc::new(Config::for_tests(&root_path, &temp.path().join("config")));
            let db = crate::db::open_memory().await.unwrap();
            let settings = SettingsStore::load(db.clone(), HwAccel::None)
                .await
                .unwrap();
            let layout = Arc::new(StashLayout::new(&root_path));
            layout.ensure().await.unwrap();
            defs::ensure_folders(&db, &layout).await.unwrap();
            let events = EventBus::new();
            let media = Arc::new(MediaService::new(
                config,
                db.clone(),
                settings.clone(),
                layout.clone(),
                events.clone(),
            ));
            let jobs = JobQueue::new(db.clone(), events.clone());
            let lib = Library::new(db, layout, events, jobs, settings, media);
            std::fs::create_dir_all(root_path.join("Movies/Collection/deep")).unwrap();
            std::fs::write(root_path.join("Movies/Collection/deep/clip.mkv"), b"video").unwrap();
            std::fs::write(root_path.join("Movies/Collection/note.txt"), b"note").unwrap();
            let item = lib
                .index_path("Movies/Collection/deep/clip.mkv")
                .await
                .unwrap()
                .unwrap();
            let body = RenameRequest {
                key: "folder:Collection".into(),
                folder: String::new(),
                name: "Renamed".into(),
                token: None,
            };
            let (_, mut journal) = lib.rename_plan(defs::STARTING_MOVIES, &body).await.unwrap();
            let root = Root::open(&root_path).unwrap();
            for d in &journal.dirs {
                root.mkdir(&d.to).unwrap();
                let m = root.directory(&d.to).unwrap().metadata().unwrap();
                journal.targets.push([m.dev(), m.ino()]);
            }
            journal.id = uuid::Uuid::now_v7().to_string();
            let stage = root.new_trash_dir("flatten-tree").unwrap();
            root.write_flatten_journal(&stage, &serde_json::to_vec(&journal).unwrap())
                .unwrap();
            if cut >= 1 {
                for (n, f) in journal.files.iter().enumerate() {
                    root.rename_trash(&f.from, &format!("{stage}/file-{n}"))
                        .unwrap();
                }
            }
            if cut >= 2 {
                let f = &journal.files[0];
                root.rename_trash(&format!("{stage}/file-0"), &f.to)
                    .unwrap();
            }
            if cut == 3 {
                for (n, f) in journal.files.iter().enumerate().skip(1) {
                    root.rename_trash(&format!("{stage}/file-{n}"), &f.to)
                        .unwrap();
                }
                for (id, _, to) in &journal.items {
                    sqlx::query("UPDATE items SET rel_path=? WHERE id=?")
                        .bind(to)
                        .bind(id)
                        .execute(&lib.db)
                        .await
                        .unwrap();
                }
                sqlx::query("INSERT INTO library_folder_edits VALUES(?,?)")
                    .bind(&journal.id)
                    .bind(&journal.library)
                    .execute(&lib.db)
                    .await
                    .unwrap();
            }
            if cut == 4 {
                std::fs::write(root_path.join(&journal.files[0].to), b"unexpected").unwrap();
                assert!(lib.recover_flattened_files().await.is_err());
                assert_eq!(
                    std::fs::read(root_path.join(&journal.files[0].to)).unwrap(),
                    b"unexpected"
                );
                assert!(
                    lib.create_library_folder(
                        defs::STARTING_MOVIES,
                        &folders::CreateFolder {
                            parent: String::new(),
                            name: "Blocked".into()
                        }
                    )
                    .await
                    .is_err()
                );
                continue;
            }
            lib.recover_flattened_files().await.unwrap();
            lib.recover_flattened_files().await.unwrap();
            let current = super::super::repo::get(&lib.db, &item.id)
                .await
                .unwrap()
                .unwrap();
            let target = if cut == 3 {
                "Movies/Renamed/deep/clip.mkv"
            } else {
                "Movies/Collection/deep/clip.mkv"
            };
            assert_eq!(current.rel_path, target);
            assert_eq!(std::fs::read(root_path.join(target)).unwrap(), b"video");
            assert_eq!(
                std::fs::read_dir(root_path.join(".popcornstash/Trash"))
                    .unwrap()
                    .count(),
                0
            );
        }
    }
}
