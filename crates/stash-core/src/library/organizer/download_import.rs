//! Completed downloads keep their names and one physical folder per destination.
use super::*;
use crate::library::cleanup::{Root, fingerprint};
use crate::library::flatten_downloads::{FlattenFile, recovery};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;

#[derive(Serialize, Deserialize)]
pub(in crate::library) struct Journal {
    pub import_version: u8,
    id: String,
    download_id: String,
    source: String,
    files: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
struct Entry {
    file: FlattenFile,
    identity: [u64; 5],
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
fn check(root: &Root, path: &str, expected: [u64; 5]) -> Result<()> {
    if identity(&root.metadata(path)?) != expected {
        return Err(CoreError::conflict(
            "A download file changed. Every copy was kept.",
        ));
    }
    Ok(())
}
fn exists(root: &Root, path: &str) -> Result<bool> {
    match root.metadata(path) {
        Ok(_) => Ok(true),
        Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

pub(super) async fn released(lib: &Library, id: &str, source: &str) -> Result<()> {
    if StashLayout::zone(source) != Zone::Completed {
        return Err(CoreError::conflict(
            "Unfinished downloads cannot be imported.",
        ));
    }
    let Some(row) = download_row(lib, id).await? else {
        let known: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM download_origins WHERE id=?)")
                .bind(id)
                .fetch_one(&lib.db)
                .await?;
        return if known {
            Ok(())
        } else {
            Err(CoreError::NotFound)
        };
    };
    if !matches!(
        row.state,
        crate::models::DownloadState::Completed
            | crate::models::DownloadState::Sorting
            | crate::models::DownloadState::Sorted
    ) || row.rel_path.as_deref() != Some(source)
        || StashLayout::zone(source) != Zone::Completed
    {
        return Err(CoreError::conflict(
            "This download has not been released for import. Its files were kept in place.",
        ));
    }
    Ok(())
}

pub(super) struct Plan {
    pub files: Vec<(PathBuf, Role)>,
    pub companions: HashMap<String, Vec<String>>,
}

fn numbered(name: &str, n: usize) -> String {
    if n == 1 {
        return name.into();
    }
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| format!(".{s}"))
        .unwrap_or_default();
    format!("{stem} ({n}){ext}")
}
fn allocate(name: &str, reserved: &HashSet<String>, used: &mut HashSet<String>) -> Result<String> {
    for n in 1..10_000 {
        let candidate = numbered(name, n);
        let key = candidate.to_lowercase();
        if !used.contains(&key) && (n == 1 || !reserved.contains(&key)) {
            used.insert(key);
            return Ok(candidate);
        }
    }
    Err(CoreError::conflict(
        "Could not allocate a collision-safe download filename.",
    ))
}
fn claim(file: &str, media: &str) -> Option<usize> {
    sidecar_claim(&file.to_ascii_lowercase(), &media.to_ascii_lowercase())
}

/// The completion path has already released/adopted the torrent and discarded deselections.
pub(super) async fn prepare(lib: &Library, download: &DownloadRow) -> Result<Plan> {
    let source = download.rel_path.as_deref().ok_or(CoreError::NotFound)?;
    let _moves = lib.lock_moves().await?;
    released(lib, &download.id, source).await?;
    let root = Root::open(lib.layout.root())?;
    let abs = lib.layout.abs(source)?;
    let original = plan_sort(&lib.layout, &abs, true, Taken::default());
    let roles: HashMap<_, _> = original.files.into_iter().collect();
    let mut found = Vec::new();
    for entry in WalkDir::new(&abs).follow_links(false) {
        let entry = entry.map_err(|e| CoreError::Other(e.into()))?;
        if entry.depth() > 32 || found.len() >= 2000 {
            return Err(CoreError::invalid(
                "Import a smaller download (up to 2,000 files and 32 levels).",
            ));
        }
        let rel = lib.layout.rel(entry.path()).ok_or(CoreError::NotFound)?;
        let meta = root.metadata(&rel)?;
        if meta.is_dir() {
            continue;
        }
        if !meta.is_file() || meta.nlink() != 1 || rel.split('/').any(is_junk_name) {
            return Err(CoreError::conflict(
                "This download contains a link, special or partial file. Its files were kept in place.",
            ));
        }
        found.push((entry.into_path(), rel, meta));
    }
    found.sort_by(|a, b| {
        a.0.components()
            .count()
            .cmp(&b.0.components().count())
            .then(a.1.cmp(&b.1))
    });
    let directory = root.metadata(source)?.is_dir();
    let reserved: HashSet<String> = found
        .iter()
        .map(|(p, _, _)| file_name(p).to_lowercase())
        .collect();
    let mut names = HashSet::new();
    let mut allocated = HashMap::new();
    for (n, (path, _, _)) in found
        .iter()
        .enumerate()
        .filter(|(_, (p, _, _))| media_kind(p) != MediaKind::Other)
    {
        allocated.insert(n, allocate(&file_name(path), &reserved, &mut names)?);
    }
    for (n, (path, _, _)) in found.iter().enumerate() {
        if allocated.contains_key(&n) {
            continue;
        }
        let original_name = file_name(path);
        let owner = found
            .iter()
            .enumerate()
            .filter(|(_, (p, _, _))| media_kind(p) != MediaKind::Other)
            .filter_map(|(v, (video, _, _))| {
                claim(&original_name, &file_name(video)).map(|claim| {
                    (
                        v,
                        claim,
                        path.parent()
                            .unwrap_or(path)
                            .components()
                            .zip(video.parent().unwrap_or(video).components())
                            .take_while(|(a, b)| a == b)
                            .count(),
                    )
                })
            })
            .max_by_key(|(_, claim, depth)| (*claim, *depth));
        let desired = if super::super::scanner::is_sidecar(path) {
            if let Some((v, claim, _)) = owner {
                let old_media = file_name(&found[v].0);
                let new_media = &allocated[&v];
                let base = if claim == old_media.len() + 1 {
                    new_media.clone()
                } else {
                    Path::new(new_media)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or(CoreError::NotFound)?
                        .into()
                };
                format!(
                    "{base}{}",
                    original_name.get(claim - 1..).ok_or(CoreError::NotFound)?
                )
            } else {
                original_name
            }
        } else {
            original_name
        };
        allocated.insert(n, allocate(&desired, &reserved, &mut names)?);
    }
    let mut files = Vec::new();
    for (n, (_, from, meta)) in found.iter().enumerate() {
        let name = &allocated[&n];
        let to = if directory {
            format!("{source}/{name}")
        } else {
            from.clone()
        };
        let item = repo::get_by_path(&lib.db, from).await?;
        files.push(FlattenFile {
            item_id: item.map_or_else(String::new, |i| i.id),
            from: from.clone(),
            to,
            kind: "item".into(),
            fingerprint: fingerprint(meta),
        });
    }
    relocate(lib, &download.id, source, &files, false, None).await?;
    let mut videos: Vec<usize> = found
        .iter()
        .enumerate()
        .filter(|(_, (p, _, _))| {
            media_kind(p) == MediaKind::Video && roles.get(p) == Some(&Role::Media)
        })
        .map(|(n, _)| n)
        .collect();
    if videos.is_empty() {
        videos = found
            .iter()
            .enumerate()
            .filter(|(_, (p, _, _))| media_kind(p) == MediaKind::Video)
            .map(|(n, _)| n)
            .collect();
    }
    if !videos.is_empty() {
        let mut companions: HashMap<String, Vec<String>> = HashMap::new();
        for (n, (path, _, _)) in found.iter().enumerate() {
            if videos.contains(&n) {
                continue;
            }
            let owner = videos
                .iter()
                .max_by_key(|&&v| {
                    let video = &found[v].0;
                    (
                        claim(&file_name(path), &file_name(video)).unwrap_or(0),
                        path.parent()
                            .unwrap_or(path)
                            .components()
                            .zip(video.parent().unwrap_or(video).components())
                            .take_while(|(a, b)| a == b)
                            .count(),
                    )
                })
                .unwrap();
            companions
                .entry(files[*owner].to.clone())
                .or_default()
                .push(files[n].to.clone());
        }
        remove_empty_dirs(&lib.layout, &abs).await;
        return Ok(Plan {
            files: videos
                .iter()
                .map(|&n| Ok((lib.layout.abs(&files[n].to)?, Role::Media)))
                .collect::<Result<_>>()?,
            companions,
        });
    }
    let has_av = found
        .iter()
        .any(|(p, _, _)| matches!(media_kind(p), MediaKind::Video | MediaKind::Audio));
    let mut plan = Vec::new();
    let mut companions: HashMap<String, Vec<String>> = HashMap::new();
    for ((path, _, _), file) in found.iter().zip(&files) {
        // Sidecars now share their media's directory, including originally nested subtitles.
        if super::super::scanner::is_sidecar(path)
            && found.iter().any(|(p, _, _)| {
                media_kind(p) != MediaKind::Other
                    && claim(&file_name(path), &file_name(p)).is_some()
            })
        {
            if let Some((owner, _)) = found
                .iter()
                .enumerate()
                .filter_map(|(n, (media, _, _))| {
                    claim(&file_name(path), &file_name(media))
                        .filter(|_| media_kind(media) != MediaKind::Other)
                        .map(|strength| (n, strength))
                })
                .max_by_key(|(_, strength)| *strength)
            {
                companions
                    .entry(files[owner].to.clone())
                    .or_default()
                    .push(file.to.clone());
            }
            continue;
        }
        let role = roles
            .get(path)
            .copied()
            .unwrap_or_else(|| match media_kind(path) {
                MediaKind::Other => Role::Standalone,
                MediaKind::Photo if has_av => Role::MaybeArtwork,
                _ => Role::Media,
            });
        plan.push((lib.layout.abs(&file.to)?, role));
    }
    remove_empty_dirs(&lib.layout, &abs).await;
    Ok(Plan {
        files: plan,
        companions,
    })
}

/// Preserve the completion folder's actual name, including a collision-safe suffix.
fn folder(source: &str) -> String {
    let path = Path::new(source);
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Download");
    component(name)
}

pub(super) async fn destination(
    lib: &Library,
    download_id: &str,
    source: &str,
    parent: &str,
    name: &str,
) -> Result<String> {
    released(lib, download_id, source).await?;
    let key = format!("download:{download_id}");
    let def = defs::load_all(&lib.db)
        .await?
        .into_iter()
        .find(|d| d.folder() == parent);
    let saved: Option<String> = if let Some(def) = &def {
        sqlx::query_scalar(
            "SELECT name FROM library_download_folders WHERE library_id=? AND group_key=?",
        )
        .bind(def.id())
        .bind(&key)
        .fetch_optional(&lib.db)
        .await?
    } else {
        None
    };
    let own: Option<String> = sqlx::query_scalar("SELECT rel_path FROM items WHERE source_download_id=? AND substr(rel_path,1,?)=? AND status='ready' ORDER BY id LIMIT 1")
        .bind(download_id).bind((parent.chars().count()+1) as i64).bind(format!("{parent}/")).fetch_optional(&lib.db).await?;
    let own = own
        .as_deref()
        .and_then(|p| p.strip_prefix(&format!("{parent}/")))
        .and_then(|p| p.split_once('/'))
        .map(|(folder, _)| folder.to_string());
    if let (Some(saved), Some(own)) = (&saved, &own)
        && saved != own
    {
        return Err(CoreError::conflict(
            "This download folder was edited. Review its organization before importing more files.",
        ));
    }
    let root = Root::open(lib.layout.root())?;
    if let Some(folder) = saved.or(own) {
        super::super::folders::validate_folder(&folder)?;
        let target = format!("{parent}/{folder}");
        let foreign: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM items WHERE substr(rel_path,1,?)=? AND (source_download_id IS NULL OR source_download_id<>?))")
            .bind((target.chars().count()+1) as i64).bind(format!("{target}/")).bind(download_id).fetch_one(&lib.db).await?;
        if foreign {
            return Err(CoreError::conflict(
                "Another source uses the saved download folder. Its files were kept in place.",
            ));
        }
        if exists(&root, &target)? {
            root.directory(&target)?;
        }
        return Ok(format!("{target}/{name}"));
    }
    let fd = root.directory(parent)?;
    let mut entries = nix::dir::Dir::from_fd(fd.into()).map_err(std::io::Error::from)?;
    let mut reserved = HashSet::new();
    for entry in entries.iter() {
        let entry = entry.map_err(std::io::Error::from)?;
        reserved.insert(entry.file_name().to_string_lossy().to_lowercase());
    }
    if let Some(def) = &def {
        let names: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM library_download_folders WHERE library_id=? AND group_key<>?",
        )
        .bind(def.id())
        .bind(&key)
        .fetch_all(&lib.db)
        .await?;
        reserved.extend(names.into_iter().map(|n| n.to_lowercase()));
    }
    let base = folder(source);
    for n in 1..10_000 {
        let folder = if n == 1 {
            base.clone()
        } else {
            format!("{base} ({n})")
        };
        super::super::folders::validate_folder(&folder)?;
        if reserved.contains(&folder.to_lowercase()) {
            continue;
        }
        let target = format!("{parent}/{folder}");
        match root.mkdir(&target) {
            Ok(()) => {
                recovery::sync_parent(&root, &target)?;
                return Ok(format!("{target}/{name}"));
            }
            Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(CoreError::conflict(
        "Could not allocate a collision-safe download folder.",
    ))
}

/// Caller holds the move lock. The marker and indexed paths commit together.
pub(super) async fn relocate(
    lib: &Library,
    download_id: &str,
    source: &str,
    files: &[FlattenFile],
    ready: bool,
    review: Option<ReviewMetadata<'_>>,
) -> Result<()> {
    released(lib, download_id, source).await?;
    if let Some(review) = &review
        && (!ready || !files.iter().any(|file| file.item_id == review.item_id))
    {
        return Err(CoreError::invalid(
            "Review metadata needs its published download item.",
        ));
    }
    let root = Root::open(lib.layout.root())?;
    let mut destinations = HashSet::new();
    for f in files {
        if !destinations.insert(f.to.to_lowercase()) {
            return Err(CoreError::conflict(
                "Duplicate destination names. Nothing was moved.",
            ));
        }
        if f.from != f.to && exists(&root, &f.to)? {
            return Err(CoreError::conflict(
                "A destination file already exists. Nothing was overwritten.",
            ));
        }
        if f.from != f.to {
            let parent = Path::new(&f.to)
                .parent()
                .and_then(Path::to_str)
                .ok_or(CoreError::NotFound)?;
            if exists(&root, parent)? {
                let fd = root.directory(parent)?;
                let mut entries =
                    nix::dir::Dir::from_fd(fd.into()).map_err(std::io::Error::from)?;
                let name = Path::new(&f.to)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or(CoreError::NotFound)?;
                for entry in entries.iter() {
                    let entry = entry.map_err(std::io::Error::from)?;
                    if entry.file_name().to_string_lossy().to_lowercase() == name.to_lowercase() {
                        return Err(CoreError::conflict(
                            "A destination name already exists, including its letter case. Nothing was overwritten.",
                        ));
                    }
                }
            }
        }
        if fingerprint(&root.metadata(&f.from)?) != f.fingerprint {
            return Err(CoreError::conflict(
                "A download file changed. Nothing was moved.",
            ));
        }
        if !f.item_id.is_empty() {
            let busy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM conversion_jobs WHERE item_id=? AND (stage IS NOT NULL OR state IN ('encoding','verifying','publishing')))")
                .bind(&f.item_id).fetch_one(&lib.db).await?;
            if busy {
                return Err(CoreError::conflict(
                    "A file is converting. Import will wait.",
                ));
            }
        }
    }
    if files.iter().all(|f| f.from == f.to) {
        return Ok(());
    }
    for f in files {
        let parent = Path::new(&f.to)
            .parent()
            .and_then(Path::to_str)
            .ok_or(CoreError::NotFound)?;
        root.prepare_flatten_destination(parent)?;
    }
    let journal = Journal {
        import_version: 1,
        id: crate::new_id(),
        download_id: download_id.into(),
        source: source.into(),
        files: files
            .iter()
            .map(|file| {
                Ok(Entry {
                    file: file.clone(),
                    identity: identity(&root.metadata(&file.from)?),
                })
            })
            .collect::<Result<_>>()?,
    };
    let stage = root.new_trash_dir("flatten-import")?;
    lib.flatten_pending
        .store(true, std::sync::atomic::Ordering::SeqCst);
    root.write_flatten_journal(&stage, &serde_json::to_vec(&journal)?)?;
    use sqlx::Connection;
    let mut conn = lib.db.acquire().await?;
    sqlx::query("PRAGMA synchronous=FULL")
        .execute(&mut *conn)
        .await?;
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    let result: Result<()> = async {
        for (n, entry) in journal.files.iter().enumerate() {
            let f = &entry.file;
            if f.from == f.to { continue; }
            check(&root, &f.from, entry.identity)?;
            let private = format!("{stage}/file-{n}");
            root.rename_trash(&f.from, &private)?;
            recovery::sync_parent(&root, &f.from)?;
            recovery::sync_parent(&root, &private)?;
            check(&root, &private, entry.identity)?;
        }
        for (n, entry) in journal.files.iter().enumerate() {
            let f = &entry.file;
            if f.from == f.to { continue; }
            let private = format!("{stage}/file-{n}");
            root.rename_trash(&private, &f.to)?;
            recovery::sync_parent(&root, &private)?;
            recovery::sync_parent(&root, &f.to)?;
            check(&root, &f.to, entry.identity)?;
            if f.item_id.is_empty() { continue; }
            let changed = sqlx::query("UPDATE items SET rel_path=?,updated_at=?,status=CASE WHEN ? THEN 'ready' ELSE status END,suggestion_json=CASE WHEN ? THEN NULL ELSE suggestion_json END WHERE id=? AND rel_path=?")
                .bind(&f.to).bind(now()).bind(ready).bind(ready).bind(&f.item_id).bind(&f.from).execute(&mut *tx).await?.rows_affected();
            if changed != 1 { return Err(CoreError::conflict("A download item changed. Import was not committed.")); }
            if let Some(review) = &review && review.item_id == f.item_id {
                review.write(&mut tx, &f.to).await?;
            }
            sqlx::query("UPDATE conversion_jobs SET target_path=? WHERE item_id=? AND target_path=? AND state='complete'")
                .bind(&f.to).bind(&f.item_id).bind(&f.from).execute(&mut *tx).await?;
            sqlx::query("INSERT OR IGNORE INTO download_items(download_id,item_id) SELECT id,? FROM downloads WHERE id=?").bind(&f.item_id).bind(download_id).execute(&mut *tx).await?;
            if ready {
                let library: Option<String> = sqlx::query_scalar("SELECT library_id FROM items WHERE id=?").bind(&f.item_id).fetch_one(&mut *tx).await?;
                if let Some(library) = library {
                    let name = Path::new(&f.to).parent().and_then(Path::file_name).and_then(|s| s.to_str()).ok_or(CoreError::NotFound)?;
                    let key = format!("download:{download_id}");
                    sqlx::query("INSERT OR IGNORE INTO library_download_folders(library_id,group_key,name) VALUES(?,?,?)").bind(&library).bind(&key).bind(name).execute(&mut *tx).await?;
                    sqlx::query("INSERT INTO library_download_members(item_id,library_id,group_key) VALUES(?,?,?) ON CONFLICT(item_id) DO UPDATE SET library_id=excluded.library_id,group_key=excluded.group_key").bind(&f.item_id).bind(&library).bind(&key).execute(&mut *tx).await?;
                }
            }
        }
        sqlx::query("INSERT INTO download_import_edits(id,download_id) VALUES(?,?)").bind(&journal.id).bind(download_id).execute(&mut *tx).await?;
        Ok(())
    }.await;
    let result = match result {
        Ok(()) => tx.commit().await.map_err(CoreError::from),
        Err(e) => {
            tx.rollback().await?;
            Err(e)
        }
    };
    drop(conn);
    recover(lib, &root, &stage, &journal).await?;
    lib.recover_flattened_files_locked().await?;
    result
}

pub(in crate::library) async fn recover(
    lib: &Library,
    root: &Root,
    stage: &str,
    journal: &Journal,
) -> Result<()> {
    if journal.import_version != 1
        || journal.files.len() > 2000
        || StashLayout::zone(&journal.source) != Zone::Completed
    {
        return Err(CoreError::invalid(
            "Invalid download import journal. Every copy was kept.",
        ));
    }
    released(lib, &journal.download_id, &journal.source).await?;
    let committed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM download_import_edits WHERE id=? AND download_id=?)",
    )
    .bind(&journal.id)
    .bind(&journal.download_id)
    .fetch_one(&lib.db)
    .await?;
    let defs = defs::load_all(&lib.db).await?;
    for (n, entry) in journal.files.iter().enumerate() {
        let f = &entry.file;
        let inside =
            |p: &str| p == journal.source || p.starts_with(&format!("{}/", journal.source));
        let library = defs::library_for_path(&defs, &f.to).is_some()
            || StashLayout::zone(&f.to) == Zone::Other;
        if normalize_rel(&f.from)? != f.from
            || normalize_rel(&f.to)? != f.to
            || !inside(&f.from)
            || !(inside(&f.to) || (library && f.to.split('/').count() == 3))
        {
            return Err(CoreError::invalid(
                "Download recovery stays inside its completion folder and destination. Every copy was kept.",
            ));
        }
        let private = format!("{stage}/file-{n}");
        if committed || f.from == f.to {
            check(root, &f.to, entry.identity)?;
        } else {
            if !exists(root, &f.from)? {
                if !exists(root, &private)? {
                    check(root, &f.to, entry.identity)?;
                    root.rename_trash(&f.to, &private)?;
                    recovery::sync_parent(root, &f.to)?;
                    recovery::sync_parent(root, &private)?;
                }
                check(root, &private, entry.identity)?;
                root.rename_trash(&private, &f.from)?;
                recovery::sync_parent(root, &private)?;
                recovery::sync_parent(root, &f.from)?;
            }
            check(root, &f.from, entry.identity)?;
            if exists(root, &f.to)? {
                check(root, &f.to, entry.identity)?;
                // Claim any duplicate reference privately before deleting it.
                if exists(root, &private)? {
                    check(root, &private, entry.identity)?;
                    root.remove_flatten_leaf(&private)?;
                }
                root.rename_trash(&f.to, &private)?;
                recovery::sync_parent(root, &f.to)?;
            }
        }
        if exists(root, &private)? {
            check(root, &private, entry.identity)?;
            root.remove_flatten_leaf(&private)?;
        }
        recovery::sync_parent(root, &private)?;
    }
    root.remove_flatten_leaf(&format!("{stage}/preparing.json"))?;
    root.remove_flatten_leaf(&format!("{stage}/journal.json"))?;
    root.remove_flatten_dir(stage)?;
    sqlx::query("DELETE FROM download_import_edits WHERE id=?")
        .bind(&journal.id)
        .execute(&lib.db)
        .await?;
    Ok(())
}
