//! Owner-initiated file cleanup. No scanning or mutation of unfinished downloads.
//! Paths are relative, traversal and symlinks are refused, and moves are recoverable.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs::{File, Metadata, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
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

pub(super) fn fingerprint(meta: &Metadata) -> String {
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
pub(super) struct Root(File);
impl Root {
    pub(super) fn open(path: &Path) -> Result<Self> {
        Ok(Self(
            OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
                .open(path)?,
        ))
    }

    pub(super) fn directory(&self, rel: &str) -> Result<File> {
        let mut dir = self.0.try_clone()?;
        for part in rel.split('/') {
            if part.is_empty() || part == "." || part == ".." {
                return Err(CoreError::invalid("Choose a folder inside the library."));
            }
            dir = open_dir(&dir, &cstr(part)?)?;
        }
        Ok(dir)
    }

    pub(super) fn mkdir(&self, rel: &str) -> Result<()> {
        let (parent, name) = self.parent(rel, false)?;
        nix::sys::stat::mkdirat(
            &parent,
            name.as_c_str(),
            nix::sys::stat::Mode::from_bits_truncate(0o755),
        )
        .map_err(std::io::Error::from)?;
        Ok(())
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

    pub(super) fn metadata(&self, rel: &str) -> Result<Metadata> {
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

    pub(super) fn file(&self, rel: &str) -> Result<File> {
        let (parent, name) = self.parent(rel, false)?;
        let fd = nix::fcntl::openat(
            &parent,
            name.as_c_str(),
            nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_NONBLOCK
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        let file = File::from(fd);
        if !file.metadata()?.is_file() {
            return Err(CoreError::invalid("Choose a regular video file."));
        }
        Ok(file)
    }

    /// Only worker-owned private leaves may be unlinked by conversion cleanup.
    pub(super) fn remove_conversion_leaf(&self, rel: &str) -> Result<()> {
        if !rel.starts_with(".popcornstash/Trash/conversion-") || !private_trash_leaf(rel) {
            return Err(CoreError::invalid("Invalid conversion staging path."));
        }
        let (parent, name) = self.parent(rel, false)?;
        validate_private(&parent)?;
        match nix::unistd::unlinkat(
            &parent,
            name.as_c_str(),
            nix::unistd::UnlinkatFlags::NoRemoveDir,
        ) {
            Ok(()) | Err(nix::errno::Errno::ENOENT) => Ok(()),
            Err(e) => Err(std::io::Error::from(e).into()),
        }
    }

    pub(super) fn remove_conversion_dir(&self, rel: &str) -> Result<()> {
        if !rel.starts_with(".popcornstash/Trash/conversion-") || rel.split('/').count() != 3 {
            return Err(CoreError::invalid("Invalid conversion staging folder."));
        }
        validate_private(&self.directory(rel)?)?;
        let (parent, name) = self.parent(rel, false)?;
        nix::unistd::unlinkat(
            &parent,
            name.as_c_str(),
            nix::unistd::UnlinkatFlags::RemoveDir,
        )
        .map_err(std::io::Error::from)?;
        parent.sync_all()?;
        Ok(())
    }

    /// Persist the destination's directory entry before any source file moves.
    pub(super) fn prepare_flatten_destination(&self, rel: &str) -> Result<()> {
        let (parent, name) = self.parent(rel, false)?;
        if let Err(e) =
            nix::sys::stat::mkdirat(&parent, name.as_c_str(), nix::sys::stat::Mode::S_IRWXU)
            && e != nix::errno::Errno::EEXIST
        {
            return Err(std::io::Error::from(e).into());
        }
        open_dir(&parent, &name)?.sync_all()?;
        parent.sync_all()?;
        Ok(())
    }

    /// Durable journal in a newly allocated private flatten directory.
    pub(super) fn write_flatten_journal(&self, stage: &str, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        if !stage.starts_with(".popcornstash/Trash/flatten-") || stage.split('/').count() != 3 {
            return Err(CoreError::invalid("Invalid flatten journal directory."));
        }
        let dir = self.directory(stage)?;
        validate_private(&dir)?;
        let fd = nix::fcntl::openat(
            &dir,
            "preparing.json",
            nix::fcntl::OFlag::O_WRONLY
                | nix::fcntl::OFlag::O_CREAT
                | nix::fcntl::OFlag::O_EXCL
                | nix::fcntl::OFlag::O_NOFOLLOW,
            nix::sys::stat::Mode::from_bits_truncate(0o600),
        )
        .map_err(std::io::Error::from)?;
        let mut file = File::from(fd);
        file.write_all(bytes)?;
        file.sync_all()?;
        // Exclusive link publication works on NAS filesystems without renameat2.
        nix::unistd::linkat(
            &dir,
            "preparing.json",
            &dir,
            "journal.json",
            nix::unistd::LinkatFlags::empty(),
        )
        .map_err(std::io::Error::from)?;
        dir.sync_all()?;
        self.remove_flatten_leaf(&format!("{stage}/preparing.json"))?;
        self.directory(".popcornstash/Trash")?.sync_all()?;
        Ok(())
    }

    pub(super) fn remove_flatten_leaf(&self, rel: &str) -> Result<()> {
        if !rel.starts_with(".popcornstash/Trash/flatten-") || !private_trash_leaf(rel) {
            return Err(CoreError::invalid("Invalid flatten journal path."));
        }
        let (parent, name) = self.parent(rel, false)?;
        validate_private(&parent)?;
        match nix::unistd::unlinkat(
            &parent,
            name.as_c_str(),
            nix::unistd::UnlinkatFlags::NoRemoveDir,
        ) {
            Ok(()) | Err(nix::errno::Errno::ENOENT) => {}
            Err(e) => return Err(std::io::Error::from(e).into()),
        }
        parent.sync_all()?;
        Ok(())
    }

    pub(super) fn remove_flatten_dir(&self, rel: &str) -> Result<()> {
        if !rel.starts_with(".popcornstash/Trash/flatten-") || rel.split('/').count() != 3 {
            return Err(CoreError::invalid("Invalid flatten journal directory."));
        }
        validate_private(&self.directory(rel)?)?;
        let (parent, name) = self.parent(rel, false)?;
        nix::unistd::unlinkat(
            &parent,
            name.as_c_str(),
            nix::unistd::UnlinkatFlags::RemoveDir,
        )
        .map_err(std::io::Error::from)?;
        parent.sync_all()?;
        Ok(())
    }

    /// Only removes an already verified empty directory; never recursive.
    pub(super) fn remove_empty_organized_dir(&self, rel: &str) -> Result<()> {
        let (parent, name) = self.parent(rel, false)?;
        nix::unistd::unlinkat(
            &parent,
            name.as_c_str(),
            nix::unistd::UnlinkatFlags::RemoveDir,
        )
        .map_err(std::io::Error::from)?;
        parent.sync_all()?;
        Ok(())
    }

    /// Public moves always require an atomic, exclusive native rename.
    pub(super) fn rename(&self, from: &str, to: &str) -> Result<()> {
        let (src, from) = self.parent(from, false)?;
        let (dst, to) = self.parent(to, true)?;
        rename_exclusive(&src, &from, &dst, &to)?;
        Ok(())
    }

    /// An exclusive, unpredictable final Trash entry, never an intermediate stage.
    pub(super) fn new_trash_dir(&self, prefix: &str) -> Result<String> {
        let rel = format!(
            ".popcornstash/Trash/{prefix}-{:032x}",
            rand::random::<u128>()
        );
        let (parent, name) = self.parent(&rel, true)?;
        nix::sys::stat::mkdirat(&parent, name.as_c_str(), nix::sys::stat::Mode::S_IRWXU)
            .map_err(std::io::Error::from)?;
        let created = open_dir(&parent, &name)?;
        // A setgid Trash parent gives new children 2700 even with mkdir(0700).
        // Normalize only the directory we just created exclusively.
        created.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        validate_private(&created)?;
        Ok(rel)
    }

    /// Only a verified private Trash endpoint permits a filesystem fallback.
    pub(super) fn rename_trash(&self, from: &str, to: &str) -> Result<()> {
        self.rename_trash_with(from, to, rename_exclusive)
    }

    fn rename_trash_with(
        &self,
        from: &str,
        to: &str,
        native: impl FnOnce(&File, &CString, &File, &CString) -> std::io::Result<()>,
    ) -> Result<()> {
        let into = private_trash_leaf(to);
        if !into && !private_trash_leaf(from) {
            return Err(CoreError::invalid("A move needs a private Trash entry."));
        }
        let (src, from) = self.parent(from, false)?;
        let (dst, to) = self.parent(to, !into)?;
        validate_private(if into { &dst } else { &src })?;
        match native(&src, &from, &dst, &to) {
            Ok(()) => return Ok(()),
            Err(e)
                if matches!(
                    e.raw_os_error(),
                    Some(libc::EINVAL | libc::EOPNOTSUPP | libc::ENOSYS)
                ) => {}
            Err(e) => return Err(e.into()),
        }
        use nix::fcntl::AtFlags;
        use nix::sys::stat::{SFlag, fstatat};
        let source = fstatat(&src, from.as_c_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
            .map_err(std::io::Error::from)?;
        let kind = SFlag::from_bits_truncate(source.st_mode);
        if kind != SFlag::S_IFREG && (into || kind != SFlag::S_IFLNK) {
            return Err(CoreError::conflict(
                "This filesystem cannot safely move this folder or symbolic link. The item was kept.",
            ));
        }
        if into {
            rename_into_private(&src, &from, &dst, &to, || {})?;
        } else {
            // linkat is exclusive, including dangling symlinks, and never follows
            // the source leaf. Only the protected source is ever unlinked.
            nix::unistd::linkat(&src, from.as_c_str(), &dst, to.as_c_str(), AtFlags::empty())
                .map_err(std::io::Error::from)?;
            let linked = fstatat(&dst, to.as_c_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(std::io::Error::from)?;
            let kept = fstatat(&src, from.as_c_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(std::io::Error::from)?;
            if source.st_dev != linked.st_dev
                || source.st_ino != linked.st_ino
                || source.st_dev != kept.st_dev
                || source.st_ino != kept.st_ino
            {
                return Err(CoreError::conflict(
                    "The file changed during restore. The Trash source was kept.",
                ));
            }
            nix::unistd::unlinkat(
                &src,
                from.as_c_str(),
                nix::unistd::UnlinkatFlags::NoRemoveDir,
            )
            .map_err(|e| CoreError::conflict(format!(
                "Restore linked the destination but could not remove the protected Trash source. Both copies were kept: {e}"
            )))?;
        }
        Ok(())
    }
}

fn rename_into_private(
    src: &File,
    from: &CString,
    dst: &File,
    to: &CString,
    before_claim: impl FnOnce(),
) -> Result<()> {
    validate_private(dst)?;
    before_claim();
    // Claim a regular leaf exclusively. A late source-directory substitution
    // cannot replace it; no public source is unlinked on failure.
    let placeholder = nix::fcntl::openat(
        dst,
        to.as_c_str(),
        nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_EXCL
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .map_err(std::io::Error::from)?;
    let result = nix::fcntl::renameat(src, from.as_c_str(), dst, to.as_c_str());
    if let Err(e) = result {
        // Only our protected placeholder is removed, never a public path.
        nix::unistd::unlinkat(dst, to.as_c_str(), nix::unistd::UnlinkatFlags::NoRemoveDir)
            .map_err(std::io::Error::from)?;
        return Err(std::io::Error::from(e).into());
    }
    drop(placeholder);
    Ok(())
}

fn private_trash_leaf(rel: &str) -> bool {
    rel.strip_prefix(".popcornstash/Trash/")
        .is_some_and(|rest| rest.split('/').count() == 2)
}

fn validate_private(dir: &File) -> Result<()> {
    let meta = dir.metadata()?;
    if meta.uid() != nix::unistd::geteuid().as_raw() || meta.mode() & 0o7777 != 0o700 {
        return Err(CoreError::conflict(
            "The Trash entry must be owned by the server with folder permissions 0700.",
        ));
    }
    Ok(())
}

// SAFETY: live CString paths and directory descriptors; the kernel enforces exclusion.
#[allow(unsafe_code)]
fn rename_exclusive(src: &File, from: &CString, dst: &File, to: &CString) -> std::io::Result<()> {
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
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn rename_cleanup(root: &Root, from: &str, to: &str, directory: bool) -> Result<()> {
    if directory {
        root.rename(from, to).map_err(|e| match e {
            CoreError::Io(ref io) if matches!(io.raw_os_error(), Some(libc::EINVAL | libc::EOPNOTSUPP | libc::ENOSYS)) =>
                CoreError::conflict("This filesystem cannot safely move folders to or from Trash. The folder was kept."),
            other => other,
        })
    } else {
        root.rename_trash(from, to)
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
        let _moves = self.lock_moves().await?;
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
        let _moves = self.lock_moves().await?;
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
                    root.rename_trash(&staged.trashed_rel_path, &choice.path)?;
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
        let _moves = self.lock_moves().await?;
        let mut tx = crate::db::begin_write(&self.db).await?;
        let entry: FileTrash = sqlx::query_as("SELECT * FROM file_trash WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(CoreError::NotFound)?;
        allowed(&entry.original_rel_path)?;
        let root = Root::open(self.layout.root())?;
        let items: Vec<(String, String, String)> = sqlx::query_as("SELECT item_id, original_rel_path, original_status FROM file_trash_items WHERE trash_id = ?").bind(id).fetch_all(&mut *tx).await?;
        rename_cleanup(&root, &entry.trashed_rel_path, &entry.original_rel_path, entry.is_dir).map_err(|e| CoreError::conflict(format!("Restore could not finish. The original name may be occupied or the folder unavailable. {e}")))?;
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
            rename_cleanup(
                &root,
                &entry.original_rel_path,
                &entry.trashed_rel_path,
                entry.is_dir,
            )?;
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
            let _moves = self.lock_moves().await?;
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
    let dir = if directory {
        format!(".popcornstash/Trash/cleanup-{}", new_id())
    } else {
        root.new_trash_dir("cleanup")?
    };
    let id = dir.rsplit('/').next().unwrap().to_string();
    let name = Path::new(rel).file_name().unwrap().to_string_lossy();
    let target = format!("{dir}/{name}");
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
    rename_cleanup(&root, rel, &target, directory)?;
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
        rename_cleanup(&root, &target, rel, directory)?;
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
        rename_cleanup(&root, &target, rel, directory)?;
        return Err(e);
    }
    Ok(Staged {
        trashed_rel_path: target,
        facts,
    })
}

pub(crate) fn undo_stage(lib: &Library, staged: &Staged, original: &str) -> Result<()> {
    let root = Root::open(lib.layout.root())?;
    rename_cleanup(
        &root,
        &staged.trashed_rel_path,
        original,
        root.metadata(&staged.trashed_rel_path)?.is_dir(),
    )
}

#[cfg(test)]
mod trash_move_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn fixture() -> (tempfile::TempDir, Root, String) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Inbox")).unwrap();
        let root = Root::open(dir.path()).unwrap();
        let private = root.new_trash_dir("test").unwrap();
        (dir, root, format!("{private}/payload"))
    }

    fn unsupported(_: &File, _: &CString, _: &File, _: &CString) -> std::io::Result<()> {
        Err(std::io::Error::from_raw_os_error(libc::EINVAL))
    }

    #[test]
    fn fallback_roundtrip_and_database_rollback_directions() {
        let (dir, root, trash) = fixture();
        std::fs::write(dir.path().join("Inbox/file"), b"original").unwrap();
        let before = root.metadata("Inbox/file").unwrap();
        for errno in [libc::EINVAL, libc::EOPNOTSUPP, libc::ENOSYS] {
            root.rename_trash_with("Inbox/file", &trash, |_, _, _, _| {
                Err(std::io::Error::from_raw_os_error(errno))
            })
            .unwrap();
            assert!(!dir.path().join("Inbox/file").exists());
            root.rename_trash_with(&trash, "Inbox/file", unsupported)
                .unwrap();
            let after = root.metadata("Inbox/file").unwrap();
            assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
            assert_eq!(
                std::fs::read(dir.path().join("Inbox/file")).unwrap(),
                b"original"
            );
        }
        assert!(
            root.rename_trash_with("Inbox/file", "Inbox/other", unsupported)
                .is_err()
        );
        assert!(
            root.rename_trash_with("Inbox/file", &trash, |_, _, _, _| {
                Err(std::io::Error::from_raw_os_error(libc::EACCES))
            })
            .is_err()
        );
        assert!(dir.path().join("Inbox/file").exists());
    }

    #[test]
    fn fallback_collisions_include_dangling_links_in_both_directions() {
        let (dir, root, trash) = fixture();
        for dangling in [false, true] {
            std::fs::write(dir.path().join("Inbox/file"), b"public").unwrap();
            if dangling {
                symlink("absent", dir.path().join(&trash)).unwrap();
            } else {
                std::fs::write(dir.path().join(&trash), b"private").unwrap();
            }
            assert!(
                root.rename_trash_with("Inbox/file", &trash, unsupported)
                    .is_err()
            );
            assert_eq!(
                std::fs::read(dir.path().join("Inbox/file")).unwrap(),
                b"public"
            );
            if dangling {
                assert_eq!(
                    std::fs::read_link(dir.path().join(&trash)).unwrap(),
                    Path::new("absent")
                );
            } else {
                assert_eq!(std::fs::read(dir.path().join(&trash)).unwrap(), b"private");
            }
            std::fs::remove_file(dir.path().join(&trash)).unwrap();
            std::fs::write(dir.path().join(&trash), b"private").unwrap();
            if dangling {
                std::fs::remove_file(dir.path().join("Inbox/file")).unwrap();
                symlink("absent", dir.path().join("Inbox/file")).unwrap();
            }
            assert!(
                root.rename_trash_with(&trash, "Inbox/file", unsupported)
                    .is_err()
            );
            assert_eq!(std::fs::read(dir.path().join(&trash)).unwrap(), b"private");
            if dangling {
                assert_eq!(
                    std::fs::read_link(dir.path().join("Inbox/file")).unwrap(),
                    Path::new("absent")
                );
            } else {
                assert_eq!(
                    std::fs::read(dir.path().join("Inbox/file")).unwrap(),
                    b"public"
                );
            }
            std::fs::remove_file(dir.path().join("Inbox/file")).unwrap();
            std::fs::remove_file(dir.path().join(&trash)).unwrap();
        }
    }

    #[test]
    fn fallback_refuses_substituted_symlinks_and_unprotected_directories() {
        let (dir, root, trash) = fixture();
        std::fs::write(dir.path().join("Inbox/file"), b"original").unwrap();
        std::fs::write(dir.path().join("Inbox/keep"), b"keep").unwrap();
        assert!(
            root.rename_trash_with("Inbox/file", &trash, |_, _, _, _| {
                std::fs::remove_file(dir.path().join("Inbox/file")).unwrap();
                symlink("keep", dir.path().join("Inbox/file")).unwrap();
                unsupported(
                    &root.0,
                    &cstr("unused").unwrap(),
                    &root.0,
                    &cstr("unused").unwrap(),
                )
            })
            .is_err()
        );
        assert_eq!(
            std::fs::read(dir.path().join("Inbox/keep")).unwrap(),
            b"keep"
        );
        assert!(dir.path().join("Inbox/file").is_symlink());
        symlink(dir.path().join("Inbox/keep"), dir.path().join(&trash)).unwrap();
        root.rename_trash_with(&trash, "Inbox/restored", unsupported)
            .unwrap();
        assert!(dir.path().join("Inbox/restored").is_symlink());
        assert_eq!(
            std::fs::read(dir.path().join("Inbox/keep")).unwrap(),
            b"keep"
        );
        std::fs::write(dir.path().join(&trash), b"private").unwrap();
        let parent = dir.path().join(&trash).parent().unwrap().to_path_buf();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            root.rename_trash_with(&trash, "Inbox/restored", unsupported)
                .is_err()
        );
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        // A substituted parent is refused even when its target has the right mode.
        std::fs::rename(&parent, parent.with_extension("saved")).unwrap();
        symlink(parent.with_extension("saved"), &parent).unwrap();
        assert!(
            root.rename_trash_with(&trash, "Inbox/restored", unsupported)
                .is_err()
        );
    }

    #[test]
    fn fallback_rejects_folders_and_native_moves_remain_exclusive() {
        let (dir, root, trash) = fixture();
        std::fs::create_dir(dir.path().join("Inbox/folder")).unwrap();
        let err = root
            .rename_trash_with("Inbox/folder", &trash, unsupported)
            .unwrap_err();
        assert!(err.to_string().contains("cannot safely move"));
        std::fs::write(dir.path().join("Inbox/file"), b"source").unwrap();
        symlink("absent", dir.path().join("Inbox/occupied")).unwrap();
        assert!(root.rename("Inbox/file", "Inbox/occupied").is_err());
        assert_eq!(
            std::fs::read(dir.path().join("Inbox/file")).unwrap(),
            b"source"
        );
        assert!(dir.path().join("Inbox/occupied").is_symlink());
    }
    #[test]
    fn claimed_leaf_blocks_directory_swap_and_captured_symlink_rolls_back() {
        let (dir, root, trash) = fixture();
        std::fs::write(dir.path().join("Inbox/file"), b"original").unwrap();
        let (src, from) = root.parent("Inbox/file", false).unwrap();
        let (dst, to) = root.parent(&trash, false).unwrap();
        assert!(
            rename_into_private(&src, &from, &dst, &to, || {
                std::fs::rename(
                    dir.path().join("Inbox/file"),
                    dir.path().join("Inbox/saved"),
                )
                .unwrap();
                std::fs::create_dir(dir.path().join("Inbox/file")).unwrap();
            })
            .is_err()
        );
        assert!(dir.path().join("Inbox/file").is_dir());
        assert!(!dir.path().join(&trash).exists());
        std::fs::remove_dir(dir.path().join("Inbox/file")).unwrap();
        std::fs::write(dir.path().join("Inbox/file"), b"replacement").unwrap();
        rename_into_private(&src, &from, &dst, &to, || {
            std::fs::remove_file(dir.path().join("Inbox/file")).unwrap();
            symlink("saved", dir.path().join("Inbox/file")).unwrap();
        })
        .unwrap();
        assert!(root.metadata(&trash).is_err());
        root.rename_trash_with(&trash, "Inbox/file", unsupported)
            .unwrap();
        assert_eq!(
            std::fs::read_link(dir.path().join("Inbox/file")).unwrap(),
            Path::new("saved")
        );
        assert_eq!(
            std::fs::read(dir.path().join("Inbox/saved")).unwrap(),
            b"original"
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn new_trash_dir_clears_inherited_setgid_but_existing_entries_stay_strict() {
        let (dir, root, _) = fixture();
        let trash = dir.path().join(".popcornstash/Trash");
        std::fs::set_permissions(&trash, std::fs::Permissions::from_mode(0o2775)).unwrap();
        let inherited = trash.join("inherited");
        std::fs::create_dir(&inherited).unwrap();
        assert_ne!(std::fs::metadata(&inherited).unwrap().mode() & 0o2000, 0);
        let entry = root.new_trash_dir("inbox").unwrap();
        let meta = root.directory(&entry).unwrap().metadata().unwrap();
        assert_eq!(meta.mode() & 0o7777, 0o700);
        assert_eq!(meta.uid(), nix::unistd::geteuid().as_raw());
        std::fs::set_permissions(
            dir.path().join(&entry),
            std::fs::Permissions::from_mode(0o2700),
        )
        .unwrap();
        std::fs::write(dir.path().join("Inbox/file"), b"keep").unwrap();
        assert!(
            root.rename_trash_with("Inbox/file", &format!("{entry}/payload"), unsupported)
                .is_err()
        );
        assert_eq!(
            root.directory(&entry).unwrap().metadata().unwrap().mode() & 0o7777,
            0o2700
        );
        assert_eq!(
            std::fs::read(dir.path().join("Inbox/file")).unwrap(),
            b"keep"
        );
    }
}
