//! The stash folder layout and filesystem safety helpers (section 3).

use std::path::{Component, Path, PathBuf};

use crate::models::Category;
use crate::{CoreError, Result};

pub const MOVIES: &str = "Movies";
pub const SHOWS: &str = "Shows";
pub const HOME_VIDEOS: &str = "Home Videos";
pub const PHOTOS: &str = "Photos";
pub const OTHER: &str = "Other";
pub const INBOX: &str = "Inbox";
pub const DOWNLOADS: &str = "Downloads";
pub const IN_PROGRESS: &str = "Downloads/In Progress";
pub const COMPLETED: &str = "Downloads/Completed";
/// Partial browser uploads (`<id>.part`). Kept out of `Downloads/In Progress`, which
/// the torrent engine may write, and private to the web server's user (0700).
pub const UPLOADS_TMP: &str = ".popcornstash/uploads";
/// Where partial uploads lived before; moved to [`UPLOADS_TMP`] on start.
pub const LEGACY_UPLOADS_TMP: &str = "Downloads/In Progress/.uploads";
pub const INTERNAL: &str = ".popcornstash";
pub const TRASH: &str = ".popcornstash/Trash";
pub const MARKER: &str = ".popcornstash/stash.id";

/// Which part of the stash a relative path lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// One of the five library folders.
    Library(Category),
    /// `Inbox/`: sorted automatically.
    Inbox,
    /// `Downloads/In Progress/`: ignored by the indexer.
    InProgress,
    /// `Downloads/Completed/`: sorted automatically.
    Completed,
    /// `.popcornstash/`: never indexed.
    Internal,
    /// Anything else: the owner's own folders. Indexed in place, never moved.
    Custom,
}

impl Zone {
    /// Files here are moved by the sorter without asking.
    pub fn auto_sorts(self) -> bool {
        matches!(self, Zone::Inbox | Zone::Completed)
    }

    /// Files here are indexed into the library.
    pub fn indexed(self) -> bool {
        !matches!(self, Zone::InProgress | Zone::Internal)
    }
}

#[derive(Debug, Clone)]
pub struct StashLayout {
    root: PathBuf,
}

impl StashLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Create every fixed folder and the marker file. Idempotent.
    pub async fn ensure(&self) -> Result<()> {
        for dir in [
            MOVIES,
            SHOWS,
            HOME_VIDEOS,
            PHOTOS,
            OTHER,
            INBOX,
            IN_PROGRESS,
            COMPLETED,
            TRASH,
        ] {
            tokio::fs::create_dir_all(self.root.join(dir)).await?;
        }
        self.ensure_uploads_dir().await?;
        let marker = self.root.join(MARKER);
        if tokio::fs::metadata(&marker).await.is_err() {
            tokio::fs::write(&marker, crate::new_id()).await?;
        }
        Ok(())
    }

    pub fn category_dir(&self, category: Category) -> PathBuf {
        self.root.join(category_folder(category))
    }
    pub fn inbox(&self) -> PathBuf {
        self.root.join(INBOX)
    }
    pub fn in_progress(&self) -> PathBuf {
        self.root.join(IN_PROGRESS)
    }
    pub fn completed(&self) -> PathBuf {
        self.root.join(COMPLETED)
    }
    pub fn uploads_tmp(&self) -> PathBuf {
        self.root.join(UPLOADS_TMP)
    }

    /// Create the partial-uploads folder if needed and keep it private (0700): uploads
    /// in progress are family photos and videos nobody else on the system should read.
    pub async fn ensure_uploads_dir(&self) -> Result<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        let dir = self.uploads_tmp();
        tokio::fs::create_dir_all(&dir).await?;
        let meta = tokio::fs::symlink_metadata(&dir).await?;
        if !meta.is_dir() {
            return Err(CoreError::invalid(
                "The partial uploads folder is not a plain folder.",
            ));
        }
        if meta.permissions().mode() & 0o777 != 0o700 {
            tokio::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).await?;
        }
        Ok(dir)
    }
    pub fn trash(&self) -> PathBuf {
        self.root.join(TRASH)
    }

    /// Absolute path for a stored relative path, rejecting anything that escapes the root.
    pub fn abs(&self, rel: impl AsRef<Path>) -> Result<PathBuf> {
        safe_join(&self.root, rel.as_ref())
    }

    /// The absolute path for `rel` without looking at the filesystem (no `..`, no
    /// absolute paths). Only for checking whether something is there; never open or
    /// write through it.
    pub fn lexical(&self, rel: impl AsRef<Path>) -> Result<PathBuf> {
        lexical_join(&self.root, rel.as_ref())
    }

    /// True only when an existing link provably leads `rel` out of the stash. A path
    /// that cannot be resolved (unreadable, a link loop) does not count.
    pub fn leads_outside(&self, rel: impl AsRef<Path>) -> bool {
        let Ok(out) = lexical_join(&self.root, rel.as_ref()) else {
            return false;
        };
        let mut existing = out.as_path();
        while !existing.exists() {
            match existing.parent() {
                Some(parent) => existing = parent,
                None => return false,
            }
        }
        match (self.root.canonicalize(), existing.canonicalize()) {
            (Ok(root), Ok(real)) => !real.starts_with(root),
            _ => false,
        }
    }

    /// Relative path (with `/` separators) for an absolute path under the root.
    pub fn rel(&self, abs: &Path) -> Option<String> {
        let rel = abs.strip_prefix(&self.root).ok()?;
        let parts: Vec<String> = rel
            .components()
            .map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Option<_>>()?;
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("/"))
        }
    }

    /// Classify a relative path into its zone.
    pub fn zone(rel: &str) -> Zone {
        let rel = rel.trim_start_matches('/');
        let starts = |prefix: &str| rel == prefix || rel.starts_with(&format!("{prefix}/"));
        if starts(INTERNAL) {
            Zone::Internal
        } else if starts(IN_PROGRESS) {
            Zone::InProgress
        } else if starts(COMPLETED) {
            Zone::Completed
        } else if starts(DOWNLOADS) {
            // Loose files directly in Downloads/ are treated like completed ones.
            Zone::Completed
        } else if starts(INBOX) {
            Zone::Inbox
        } else if starts(MOVIES) {
            Zone::Library(Category::Movie)
        } else if starts(SHOWS) {
            Zone::Library(Category::Episode)
        } else if starts(HOME_VIDEOS) {
            Zone::Library(Category::HomeVideo)
        } else if starts(PHOTOS) {
            Zone::Library(Category::Photo)
        } else if starts(OTHER) {
            Zone::Library(Category::Other)
        } else {
            Zone::Custom
        }
    }
}

pub fn category_folder(category: Category) -> &'static str {
    match category {
        Category::Movie => MOVIES,
        Category::Episode => SHOWS,
        Category::HomeVideo => HOME_VIDEOS,
        Category::Photo => PHOTOS,
        Category::Other => OTHER,
    }
}

/// Join `rel` onto `root`, refusing absolute paths, `..`, and symlinks that leave the root.
///
/// Fails closed: the deepest part of the path that exists (a link counts, even one
/// whose target is missing) must resolve to somewhere inside the root, and a path
/// whose real location cannot be resolved is refused.
pub fn safe_join(root: &Path, rel: &Path) -> Result<PathBuf> {
    let escape = || CoreError::PathEscape(rel.to_path_buf());
    let out = lexical_join(root, rel)?;
    let real_root = root.canonicalize().map_err(|_| escape())?;
    let mut existing = out.as_path();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            // Missing, or below something that is not a folder: look at the parent.
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(libc::ENOTDIR) =>
            {
                existing = existing.parent().ok_or_else(escape)?;
            }
            Err(_) => return Err(escape()),
        }
    }
    let real = existing.canonicalize().map_err(|_| escape())?;
    if !real.starts_with(&real_root) {
        return Err(escape());
    }
    Ok(out)
}

/// `root` joined with the normal components of `rel`; absolute paths and `..` refused.
fn lexical_join(root: &Path, rel: &Path) -> Result<PathBuf> {
    let mut out = root.to_path_buf();
    for component in rel.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return Err(CoreError::PathEscape(rel.to_path_buf())),
        }
    }
    Ok(out)
}

/// Make a user-supplied file name safe: no separators, control characters,
/// leading dots or reserved names; at most 200 bytes; never empty.
pub fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut cleaned: String = base
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    cleaned = cleaned.trim().trim_start_matches('.').trim().to_string();
    if cleaned.len() > 200 {
        // Keep the extension when truncating.
        let (stem, ext) = match cleaned.rfind('.') {
            Some(i) if cleaned.len() - i <= 12 => {
                (cleaned[..i].to_string(), cleaned[i..].to_string())
            }
            _ => (cleaned.clone(), String::new()),
        };
        let mut end = 200usize.saturating_sub(ext.len());
        while !stem.is_char_boundary(end.min(stem.len())) {
            end -= 1;
        }
        cleaned = format!("{}{}", &stem[..end.min(stem.len())], ext);
    }
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "untitled".to_string()
    } else {
        cleaned
    }
}

/// Names the indexer and sorter never touch: hidden files, partial downloads, OS clutter.
pub fn is_ignored_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    name.starts_with('.')
        || name.starts_with("~$")
        || lower.ends_with(".part")
        || lower.ends_with(".crdownload")
        || lower.ends_with(".!qb")
        || lower.ends_with(".tmp")
        || lower.ends_with(".partial")
        || lower == "thumbs.db"
        || lower == "desktop.ini"
        || lower == "@eadir"
}

/// `dest`, or `dest (2).ext`, `dest (3).ext`… whichever does not exist yet.
pub fn unique_path(dest: &Path) -> PathBuf {
    if !dest.exists() {
        return dest.to_path_buf();
    }
    let parent = dest.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = dest
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| parent.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("unbounded search finds a free name")
}

/// Move a file or directory. Uses rename; falls back to copy + verify + remove across filesystems.
/// Never overwrites: fails if `dest` exists.
pub async fn move_path(src: &Path, dest: &Path) -> Result<()> {
    if tokio::fs::symlink_metadata(dest).await.is_ok() {
        return Err(CoreError::conflict(format!(
            "{} already exists",
            dest.display()
        )));
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    match tokio::fs::rename(src, dest).await {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            let (src, dest) = (src.to_path_buf(), dest.to_path_buf());
            tokio::task::spawn_blocking(move || copy_then_remove(&src, &dest))
                .await
                .map_err(|e| CoreError::Other(e.into()))?
        }
        Err(e) => Err(e.into()),
    }
}

fn copy_then_remove(src: &Path, dest: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_then_remove(&entry.path(), &dest.join(entry.file_name()))?;
        }
        std::fs::remove_dir(src)?;
    } else {
        let tmp = dest.with_extension(format!(
            "{}popcornstash-tmp",
            dest.extension()
                .map(|e| format!("{}.", e.to_string_lossy()))
                .unwrap_or_default()
        ));
        let copied = std::fs::copy(src, &tmp)?;
        let file = std::fs::File::open(&tmp)?;
        file.sync_all()?;
        if copied != meta.len() {
            let _ = std::fs::remove_file(&tmp);
            return Err(CoreError::external(format!(
                "copy of {} was incomplete",
                src.display()
            )));
        }
        std::fs::rename(&tmp, dest)?;
        if let Ok(modified) = meta.modified() {
            let _ = std::fs::File::options()
                .write(true)
                .open(dest)
                .and_then(|f| f.set_modified(modified));
        }
        std::fs::remove_file(src)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones() {
        assert_eq!(
            StashLayout::zone("Movies/Heat (1995)/Heat (1995).mkv"),
            Zone::Library(Category::Movie)
        );
        assert_eq!(
            StashLayout::zone("Home Videos/2024/06/IMG_1.MOV"),
            Zone::Library(Category::HomeVideo)
        );
        assert_eq!(StashLayout::zone("Inbox/a.mp4"), Zone::Inbox);
        assert_eq!(
            StashLayout::zone("Downloads/In Progress/x/a.mkv"),
            Zone::InProgress
        );
        assert_eq!(
            StashLayout::zone("Downloads/Completed/x/a.mkv"),
            Zone::Completed
        );
        assert_eq!(
            StashLayout::zone(".popcornstash/Trash/a.mkv"),
            Zone::Internal
        );
        assert_eq!(StashLayout::zone("Family 2019/beach.mp4"), Zone::Custom);
        assert_eq!(StashLayout::zone("Moviesque/a.mp4"), Zone::Custom);
    }

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename(".hidden"), "hidden");
        assert_eq!(sanitize_filename("a\u{0}b:c.mkv"), "a_b_c.mkv");
        assert_eq!(sanitize_filename(""), "untitled");
        assert_eq!(sanitize_filename(".."), "untitled");
        let long = format!("{}.mkv", "x".repeat(300));
        let s = sanitize_filename(&long);
        assert!(s.len() <= 200 && s.ends_with(".mkv"));
    }

    #[test]
    fn safe_join_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        assert!(safe_join(dir.path(), Path::new("../x")).is_err());
        assert!(safe_join(dir.path(), Path::new("/etc/passwd")).is_err());
        assert!(safe_join(dir.path(), Path::new("Movies/a.mkv")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn safe_join_rejects_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        assert!(safe_join(dir.path(), Path::new("link/file")).is_err());
    }

    /// A link whose target does not exist (yet) can still be written through, so it
    /// is refused like any other link that cannot be shown to stay inside. So is a
    /// path whose real location cannot be worked out at all.
    #[cfg(unix)]
    #[test]
    fn safe_join_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Inbox")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("not-yet"),
            dir.path().join("Inbox/dangling.jpg"),
        )
        .unwrap();
        assert!(safe_join(dir.path(), Path::new("Inbox/dangling.jpg")).is_err());
        assert!(safe_join(dir.path(), Path::new("Inbox/dangling.jpg/x")).is_err());
        // Missing names under real folders are fine (files about to be created).
        assert!(safe_join(dir.path(), Path::new("Inbox/new/deeper/file.jpg")).is_ok());
        // Below a file: nothing can be there, but the path stays inside.
        std::fs::write(dir.path().join("Inbox/file.jpg"), b"x").unwrap();
        assert!(safe_join(dir.path(), Path::new("Inbox/file.jpg/x")).is_ok());
        // A stash root that does not exist proves nothing.
        assert!(safe_join(&dir.path().join("gone"), Path::new("Movies/a.mkv")).is_err());
    }

    #[test]
    fn unique_names() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.mkv");
        assert_eq!(unique_path(&p), p);
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(unique_path(&p), dir.path().join("a (2).mkv"));
    }

    #[tokio::test]
    async fn move_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("sub/b");
        std::fs::write(&a, b"1").unwrap();
        move_path(&a, &b).await.unwrap();
        assert!(b.exists() && !a.exists());
        std::fs::write(&a, b"2").unwrap();
        assert!(move_path(&a, &b).await.is_err());
        assert_eq!(std::fs::read(&b).unwrap(), b"1");
    }

    #[test]
    fn ignored() {
        assert!(is_ignored_name(".DS_Store"));
        assert!(is_ignored_name("movie.mkv.part"));
        assert!(is_ignored_name("x.crdownload"));
        assert!(!is_ignored_name("movie.mkv"));
    }
}
