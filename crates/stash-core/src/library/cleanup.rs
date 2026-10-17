//! Owner-initiated file cleanup. No scanning or mutation of unfinished downloads.
//! Paths are relative, traversal and symlinks are refused, and moves are recoverable.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs::{File, Metadata, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use walkdir::WalkDir;

use super::{Library, classify::media_kind, repo, scanner};
use crate::events::Event;
use crate::models::MediaKind;
use crate::paths::StashLayout;
use crate::{CoreError, Result, new_id, now};

const MAX_FILES: usize = 20_000;
pub const MAX_BATCH: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChoice {
    pub path: String,
    pub fingerprint: String,
}

#[derive(Debug, Serialize)]
pub struct CleanupFile {
    #[serde(flatten)]
    pub choice: FileChoice,
    pub kind: &'static str,
    pub size: u64,
    pub review: &'static str,
    pub source_download_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Inventory {
    pub files: Vec<CleanupFile>,
    pub truncated: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct CleanupResult {
    pub removed: Vec<String>,
    pub failed: Vec<CleanupFailure>,
}

#[derive(Debug, Serialize)]
pub struct CleanupFailure {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FileTrash {
    pub id: String,
    pub original_rel_path: String,
    pub trashed_rel_path: String,
    pub is_dir: bool,
    pub size: i64,
    pub file_count: i64,
    pub trashed_at: i64,
    pub trashed_by: Option<String>,
}

fn allowed(rel: &str) -> Result<()> {
    if scanner::normalize_rel(rel)? != rel
        || rel.split('/').any(scanner::is_junk_name)
        || !StashLayout::zone(rel).indexed()
        || matches!(rel, "Inbox" | "Other" | "Downloads/Completed")
    {
        return Err(CoreError::invalid(
            "Choose a file outside unfinished downloads and system folders.",
        ));
    }
    Ok(())
}

pub fn file_kind(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if [
        "srt", "ass", "ssa", "sub", "idx", "vtt", "sup", "smi", "ttml",
    ]
    .contains(&ext.as_str())
    {
        return "subtitle";
    }
    match media_kind(path) {
        MediaKind::Video => "video",
        MediaKind::Photo => "image",
        MediaKind::Audio => "audio",
        MediaKind::Other => "other",
    }
}

fn fingerprint(meta: &Metadata) -> String {
    let mut hash = Sha256::new();
    for n in [
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime() as u64,
        meta.mtime_nsec() as u64,
        meta.ctime() as u64,
        meta.ctime_nsec() as u64,
    ] {
        hash.update(n.to_le_bytes());
    }
    hex::encode(hash.finalize())
}

// Directory descriptors anchor both ends of every rename. Linux also refuses mount
// crossings, including same-device bind mounts. Never resolve a request through a link.
struct Root(File);
impl Root {
    fn open(path: &Path) -> Result<Self> {
        Ok(Self(
            OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
                .open(path)?,
        ))
    }

    fn parent(&self, rel: &str, create: bool) -> Result<(File, CString)> {
        let parts: Vec<_> = rel.split('/').collect();
        if parts.is_empty()
            || parts
                .iter()
                .any(|p| p.is_empty() || *p == "." || *p == "..")
        {
            return Err(CoreError::invalid("That path is not inside the stash."));
        }
        let mut dir = self.0.try_clone()?;
        for part in &parts[..parts.len() - 1] {
            let part = cstr(part)?;
            if create {
                // mkdirat never follows the leaf; open_dir checks existing entries too.
                if let Err(e) =
                    nix::sys::stat::mkdirat(&dir, part.as_c_str(), nix::sys::stat::Mode::S_IRWXU)
                    && e != nix::errno::Errno::EEXIST
                {
                    return Err(std::io::Error::from(e).into());
                }
            }
            dir = open_dir(&dir, &part)?;
        }
        Ok((dir, cstr(parts[parts.len() - 1])?))
    }

    fn metadata(&self, rel: &str) -> Result<Metadata> {
        let (parent, name) = self.parent(rel, false)?;
        let flags = nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_NONBLOCK
            | nix::fcntl::OFlag::O_CLOEXEC;
        let fd = nix::fcntl::openat(
            &parent,
            name.as_c_str(),
            flags,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        let file = File::from(fd);
        Ok(file.metadata()?)
    }

    // SAFETY: CString paths live for the call; both borrowed descriptors are open
    // directories. Kernel no-replace flags make the single rename atomic.
    #[allow(unsafe_code)]
    fn rename(&self, from: &str, to: &str) -> Result<()> {
        let (src, from) = self.parent(from, false)?;
        let (dst, to) = self.parent(to, true)?;
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                src.as_raw_fd(),
                from.as_ptr(),
                dst.as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        let result = unsafe {
            libc::renameatx_np(
                src.as_raw_fd(),
                from.as_ptr(),
                dst.as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        if result < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
}

fn cstr(value: &str) -> Result<CString> {
    CString::new(value).map_err(|_| CoreError::invalid("That path contains an invalid character."))
}

fn open_dir(parent: &File, name: &CString) -> Result<File> {
    use nix::fcntl::OFlag;
    let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    #[cfg(target_os = "linux")]
    let fd = {
        use nix::fcntl::{OpenHow, ResolveFlag, openat2};
        openat2(
            parent,
            name.as_c_str(),
            OpenHow::new().flags(flags).resolve(
                ResolveFlag::RESOLVE_NO_XDEV
                    | ResolveFlag::RESOLVE_NO_SYMLINKS
                    | ResolveFlag::RESOLVE_BENEATH,
            ),
        )
    };
    #[cfg(target_os = "macos")]
    let fd = nix::fcntl::openat(
        parent,
        name.as_c_str(),
        flags,
        nix::sys::stat::Mode::empty(),
    );
    Ok(File::from(fd.map_err(std::io::Error::from)?))
}

impl Library {
    /// All visible regular files, including sidecars the media index intentionally omits.
    /// Owner authorization is enforced by the API. Narrow `folder` after a truncated scan.
    pub async fn cleanup_inventory(&self, folder: Option<&str>) -> Result<Inventory> {
        let _moves = self.lock_moves().await;
        let root_path = self.layout.root().to_path_buf();
        let folder = folder.unwrap_or("").to_string();
        if !folder.is_empty() {
            // The three fixed incoming/other roots are valid filters, never trash targets.
            if !matches!(folder.as_str(), "Inbox" | "Other" | "Downloads/Completed") {
                allowed(&folder)?;
            }
            let root = Root::open(&root_path)?;
            let (parent, name) = root.parent(&folder, false)?;
            open_dir(&parent, &name)?;
        }
        let walk_root = if folder.is_empty() {
            root_path.clone()
        } else {
            root_path.join(&folder)
        };
        let (raw, truncated) = tokio::task::spawn_blocking(move || -> Result<_> {
            let root = Root::open(&root_path)?;
            let mut files = Vec::new();
            let mut truncated = false;
            let walker = WalkDir::new(walk_root)
                .follow_links(false)
                .sort_by_file_name()
                .into_iter()
                .filter_entry(|e| {
                    if e.depth() == 0 {
                        return true;
                    }
                    let Ok(rel) = e.path().strip_prefix(&root_path) else {
                        return false;
                    };
                    let rel = rel.to_string_lossy();
                    !rel.split('/').any(scanner::is_junk_name)
                        && (StashLayout::zone(&rel).indexed() || rel == "Downloads")
                        && !e.file_type().is_symlink()
                });
            for entry in walker {
                let entry = entry.map_err(|e| {
                    CoreError::invalid(format!(
                        "A folder could not be read. Refresh or choose a smaller folder: {e}"
                    ))
                })?;
                let rel = entry
                    .path()
                    .strip_prefix(&root_path)
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                if entry.file_type().is_dir() {
                    if !rel.is_empty() {
                        let (parent, name) = root.parent(&rel, false)?;
                        open_dir(&parent, &name)?;
                    }
                    continue;
                }
                if !entry.file_type().is_file() || allowed(&rel).is_err() {
                    continue;
                }
                if files.len() == MAX_FILES {
                    truncated = true;
                    break;
                }
                let meta = root.metadata(&rel)?;
                files.push((rel, meta));
            }
            Ok((files, truncated))
        })
        .await
        .map_err(|e| CoreError::invalid(e.to_string()))??;
        let indexed: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT rel_path, status, source_download_id FROM items WHERE status != 'trashed'",
        )
        .fetch_all(&self.db)
        .await?;
        let by_path: HashMap<_, _> = indexed.into_iter().map(|(p, s, d)| (p, (s, d))).collect();
        Ok(Inventory {
            files: raw
                .into_iter()
                .map(|(path, meta)| {
                    let record = by_path.get(&path);
                    let review = match record.map(|(s, _)| s.as_str()) {
                        Some("needs_review") => "waiting",
                        Some("ready") if StashLayout::zone(&path).auto_sorts() => "ignored",
                        _ => "none",
                    };
                    CleanupFile {
                        kind: file_kind(Path::new(&path)),
                        size: meta.len(),
                        review,
                        source_download_id: record.and_then(|(_, d)| d.clone()),
                        choice: FileChoice {
                            path,
                            fingerprint: fingerprint(&meta),
                        },
                    }
                })
                .collect(),
            truncated,
        })
    }

    pub async fn cleanup_files(&self, choices: &[FileChoice], user: &str) -> Result<CleanupResult> {
        if choices.is_empty()
            || choices.len() > MAX_BATCH
            || choices
                .iter()
                .map(|f| &f.path)
                .collect::<HashSet<_>>()
                .len()
                != choices.len()
        {
            return Err(CoreError::invalid(
                "Choose between 1 and 500 distinct files.",
            ));
        }
        // Validate every request path before doing any work. No directory selections here.
        for choice in choices {
            allowed(&choice.path)?;
        }
        let _moves = self.lock_moves().await;
        let root = Root::open(self.layout.root())?;
        let mut out = CleanupResult::default();
        for choice in choices {
            let result = async {
                let meta = root.metadata(&choice.path)?;
                if !meta.is_file() || fingerprint(&meta) != choice.fingerprint {
                    return Err(CoreError::conflict(
                        "This file changed. Refresh and select it again.",
                    ));
                }
                let mut tx = crate::db::begin_write(&self.db).await?;
                let staged = stage(
                    self,
                    &mut tx,
                    &choice.path,
                    false,
                    Some(user),
                    Some(&choice.fingerprint),
                )
                .await?;
                if let Err(e) = tx.commit().await {
                    root.rename(&staged.trashed_rel_path, &choice.path)?;
                    return Err(e.into());
                }
                for (id, facts) in staged.facts {
                    self.events.publish(Event::Library {
                        r#type: crate::events::LibraryChange::Removed,
                        item_id: id,
                        before: Some(facts),
                    });
                }
                Ok(())
            }
            .await;
            match result {
                Ok(()) => out.removed.push(choice.path.clone()),
                Err(e) => out.failed.push(CleanupFailure {
                    path: choice.path.clone(),
                    message: e.to_string(),
                }),
            }
        }
        self.events.publish(Event::Inbox {
            count: repo::needs_review_count(&self.db).await?,
        });
        Ok(out)
    }

    pub async fn file_trash(&self) -> Result<Vec<FileTrash>> {
        Ok(
            sqlx::query_as("SELECT * FROM file_trash ORDER BY trashed_at DESC, id DESC")
                .fetch_all(&self.db)
                .await?,
        )
    }

    pub async fn restore_file_trash(&self, id: &str) -> Result<()> {
        let _moves = self.lock_moves().await;
        let mut tx = crate::db::begin_write(&self.db).await?;
        let entry: FileTrash = sqlx::query_as("SELECT * FROM file_trash WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(CoreError::NotFound)?;
        allowed(&entry.original_rel_path)?;
        let root = Root::open(self.layout.root())?;
        let items: Vec<(String, String, String)> = sqlx::query_as("SELECT item_id, original_rel_path, original_status FROM file_trash_items WHERE trash_id = ?").bind(id).fetch_all(&mut *tx).await?;
        root.rename(&entry.trashed_rel_path, &entry.original_rel_path).map_err(|e| CoreError::conflict(format!("Restore could not finish. The original name may be occupied or the folder unavailable. {e}")))?;
        let result = async {
            for (item_id, path, status) in &items {
                sqlx::query(
                    "UPDATE items SET rel_path = ?, status = ?, updated_at = ? WHERE id = ?",
                )
                .bind(path)
                .bind(status)
                .bind(now())
                .bind(item_id)
                .execute(&mut *tx)
                .await?;
            }
            sqlx::query("DELETE FROM file_trash WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Result::Ok(())
        }
        .await;
        if let Err(e) = result {
            root.rename(&entry.original_rel_path, &entry.trashed_rel_path)?;
            return Err(e);
        }
        let _ = tokio::fs::remove_dir(
            self.layout
                .root()
                .join(&entry.trashed_rel_path)
                .parent()
                .unwrap(),
        )
        .await;
        for (id, _, _) in items {
            if let Some(row) = repo::get(&self.db, &id).await? {
                self.events.publish(Event::Library {
                    r#type: crate::events::LibraryChange::Added,
                    item_id: id,
                    before: Some(row.facts()),
                });
            }
        }
        self.events.publish(Event::Inbox {
            count: repo::needs_review_count(&self.db).await?,
        });
        Ok(())
    }

    pub(crate) async fn empty_file_trash(&self, cutoff: Option<i64>) -> Result<u64> {
        let mut deleted = 0;
        for entry in self.file_trash().await? {
            if cutoff.is_some_and(|t| entry.trashed_at > t) {
                continue;
            }
            let _moves = self.lock_moves().await;
            let mut tx = crate::db::begin_write(&self.db).await?;
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM file_trash WHERE id = ?)")
                    .bind(&entry.id)
                    .fetch_one(&mut *tx)
                    .await?;
            if !exists {
                continue;
            }
            let dir = self.layout.trash().join(&entry.id);
            // Reject substituted parents and mount points before recursive removal.
            let root = Root::open(self.layout.root())?;
            let rel = format!(".popcornstash/Trash/{}", entry.id);
            if dir.exists() {
                verify_tree(self.layout.root(), &rel, &root)?;
                tokio::fs::remove_dir_all(&dir).await?;
            }
            let ids: Vec<String> =
                sqlx::query_scalar("SELECT item_id FROM file_trash_items WHERE trash_id = ?")
                    .bind(&entry.id)
                    .fetch_all(&mut *tx)
                    .await?;
            for id in &ids {
                sqlx::query("DELETE FROM items WHERE id = ? AND status = 'trashed'")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query("DELETE FROM file_trash WHERE id = ?")
                .bind(&entry.id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            for id in ids {
                super::organizer::remove_cached_media(self, &id).await;
            }
            deleted += 1;
        }
        Ok(deleted)
    }
}

pub(crate) struct Staged {
    pub trashed_rel_path: String,
    pub facts: Vec<(String, crate::models::ItemFacts)>,
}

fn verify_tree(root_path: &Path, rel: &str, root: &Root) -> Result<(i64, i64)> {
    let (mut bytes, mut count) = (0_i64, 0_i64);
    // No traversal through links. Validate every directory descriptor to reject mounts.
    for e in WalkDir::new(root_path.join(rel)).follow_links(false) {
        let e = e.map_err(|e| CoreError::invalid(e.to_string()))?;
        if e.file_type().is_dir() {
            let rel = e.path().strip_prefix(root_path).unwrap().to_string_lossy();
            let (parent, name) = root.parent(&rel, false)?;
            open_dir(&parent, &name)?;
        } else if e.file_type().is_file() {
            bytes = bytes.saturating_add(
                i64::try_from(
                    e.metadata()
                        .map_err(|e| CoreError::invalid(e.to_string()))?
                        .len(),
                )
                .unwrap_or(i64::MAX),
            );
            count += 1;
        }
    }
    Ok((bytes, count))
}

/// Called under the move lock and a write transaction. On a database failure after
/// rename, restore the filesystem before returning. No companion files are inferred.
pub(crate) async fn stage(
    lib: &Library,
    conn: &mut SqliteConnection,
    rel: &str,
    directory: bool,
    user: Option<&str>,
    expected: Option<&str>,
) -> Result<Staged> {
    allowed(rel)?;
    let root = Root::open(lib.layout.root())?;
    let meta = root.metadata(rel)?;
    if directory != meta.is_dir() || (!directory && !meta.is_file()) {
        return Err(CoreError::invalid(
            "This is not the expected file or folder.",
        ));
    }
    if expected.is_some_and(|value| value != fingerprint(&meta)) {
        return Err(CoreError::conflict(
            "This file changed. Refresh and select it again.",
        ));
    }
    let (size, count) = if directory {
        verify_tree(lib.layout.root(), rel, &root)?
    } else {
        (i64::try_from(meta.len()).unwrap_or(i64::MAX), 1)
    };
    let id = format!("cleanup-{}", new_id());
    let name = Path::new(rel).file_name().unwrap().to_string_lossy();
    let target = format!(".popcornstash/Trash/{id}/{name}");
    let rows: Vec<(String, String, String)> = sqlx::query_as("SELECT id, rel_path, status FROM items WHERE status != 'trashed' AND (rel_path = ? OR (? AND substr(rel_path, 1, length(?)) = ?))")
        .bind(rel).bind(directory).bind(format!("{rel}/")).bind(format!("{rel}/")).fetch_all(&mut *conn).await?;
    let mut facts = Vec::new();
    for (item_id, _, _) in &rows {
        if let Some(item) =
            sqlx::query_as::<_, crate::models::ItemRow>("SELECT * FROM items WHERE id = ?")
                .bind(item_id)
                .fetch_optional(&mut *conn)
                .await?
        {
            facts.push((item_id.clone(), item.facts()));
        }
    }
    root.rename(rel, &target)?;
    // External filesystem edits do not hold our move lock. Check the renamed
    // inode too, so a replacement between stat and rename is preserved.
    let moved = root.metadata(&target);
    if moved.as_ref().ok().is_none_or(|m| {
        m.dev() != meta.dev()
            || m.ino() != meta.ino()
            || m.len() != meta.len()
            || m.mtime() != meta.mtime()
            || m.mtime_nsec() != meta.mtime_nsec()
    }) {
        root.rename(&target, rel)?;
        return Err(CoreError::conflict(
            "This file changed during cleanup. Refresh and select it again.",
        ));
    }
    let result = async {
        sqlx::query("INSERT INTO file_trash VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&id)
            .bind(rel)
            .bind(&target)
            .bind(directory)
            .bind(size)
            .bind(count)
            .bind(now())
            .bind(user)
            .execute(&mut *conn)
            .await?;
        for (item_id, path, status) in &rows {
            sqlx::query("INSERT INTO file_trash_items VALUES (?, ?, ?, ?)")
                .bind(&id)
                .bind(item_id)
                .bind(path)
                .bind(status)
                .execute(&mut *conn)
                .await?;
            let dest = format!("{target}{}", &path[rel.len()..]);
            sqlx::query(
                "UPDATE items SET status = 'trashed', rel_path = ?, updated_at = ? WHERE id = ?",
            )
            .bind(dest)
            .bind(now())
            .bind(item_id)
            .execute(&mut *conn)
            .await?;
        }
        Result::Ok(())
    }
    .await;
    if let Err(e) = result {
        root.rename(&target, rel)?;
        return Err(e);
    }
    Ok(Staged {
        trashed_rel_path: target,
        facts,
    })
}

pub(crate) fn undo_stage(lib: &Library, staged: &Staged, original: &str) -> Result<()> {
    Root::open(lib.layout.root())?.rename(&staged.trashed_rel_path, original)
}
