//! Adopt socket (section 5, step 9; section 3, Ownership).
//!
//! When a torrent finishes, the web server (PUID) asks the supervisor to give
//! `Downloads/In Progress/<name>` to PUID:PGID so library files never stay
//! owned by the engine user. The walk is hostile-input safe: every step is
//! `openat` relative to a directory fd with `O_NOFOLLOW`, ownership changes
//! go through an `O_PATH` fd of the exact inode that was checked, it never
//! leaves the filesystem of `In Progress`, and only entries owned by the
//! engine user are changed (a hard link to a root-owned file stays root's).

use std::ffi::{CStr, CString};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use nix::dir::Dir;
use nix::errno::Errno;
use nix::fcntl::{AtFlags, OFlag, open, openat};
use nix::sys::stat::{FileStat, Mode, SFlag, fstat};
use nix::unistd::{Gid, Uid, fchownat};
use stash_core::internal_api::{SupervisorRequest, SupervisorResponse};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Stop after this many entries (a torrent with more files is not plausible).
pub const MAX_ENTRIES: u64 = 1_000_000;
/// Deepest directory nesting we descend into.
pub const MAX_DEPTH: usize = 256;
const MAX_REQUEST: usize = 4096;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct AdoptConfig {
    /// `STASH_ROOT`. The walk starts at `<root>/Downloads/In Progress`.
    pub stash_root: PathBuf,
    pub puid: u32,
    pub pgid: u32,
    pub engine_uid: u32,
    pub max_entries: u64,
}

impl AdoptConfig {
    pub fn new(stash_root: PathBuf, puid: u32, pgid: u32, engine_uid: u32) -> Self {
        Self {
            stash_root,
            puid,
            pgid,
            engine_uid,
            max_entries: MAX_ENTRIES,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdoptReport {
    pub visited: u64,
    pub changed: u64,
    /// Entries on another filesystem (not descended into, not changed).
    pub skipped_other_fs: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AdoptError {
    #[error("The name must be a single folder or file name.")]
    InvalidName,
    #[error("Nothing named {0:?} is in Downloads/In Progress.")]
    NotFound(String),
    #[error("Downloads/In Progress is missing or is not a plain folder.")]
    BadRoot,
    #[error("The folder has more than {0} entries; it was not adopted completely.")]
    TooManyEntries(u64),
    #[error("The folder is nested too deeply.")]
    TooDeep,
    #[error("A file changed while it was being adopted.")]
    Changed,
    #[error("Could not change ownership: {0}")]
    Io(String),
}

/// `name` must be one normal path component.
pub fn validate_name(name: &str) -> Result<(), AdoptError> {
    let ok = !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\0');
    if ok {
        Ok(())
    } else {
        Err(AdoptError::InvalidName)
    }
}

fn io(e: Errno) -> AdoptError {
    AdoptError::Io(e.desc().to_string())
}

fn is_dir(st: &FileStat) -> bool {
    SFlag::from_bits_truncate(st.st_mode) & SFlag::S_IFMT == SFlag::S_IFDIR
}

const DIR_FLAGS: OFlag = OFlag::O_RDONLY
    .union(OFlag::O_DIRECTORY)
    .union(OFlag::O_NOFOLLOW)
    .union(OFlag::O_CLOEXEC);

/// Open `<root>/Downloads/In Progress` without following symlinks below the root.
fn open_in_progress(stash_root: &Path) -> Result<OwnedFd, AdoptError> {
    let root = open(
        stash_root,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| AdoptError::BadRoot)?;
    let downloads =
        openat(&root, "Downloads", DIR_FLAGS, Mode::empty()).map_err(|_| AdoptError::BadRoot)?;
    openat(&downloads, "In Progress", DIR_FLAGS, Mode::empty()).map_err(|_| AdoptError::BadRoot)
}

/// Which entries a walk re-owns.
#[derive(Debug, Clone, Copy)]
enum Policy {
    /// Only entries owned by this uid (adopt: files the engine created).
    FromUid(u32),
    /// Every entry not already owned by the target, except non-directories
    /// with more than one hard link (a planted link to a system file).
    Normalize,
}

struct Walker {
    uid: u32,
    gid: u32,
    policy: Policy,
    dev: u64,
    /// (dev, ino) of a directory left alone (the engine state inside /config).
    skip: Option<(u64, u64)>,
    max_entries: u64,
    report: AdoptReport,
}

impl Walker {
    fn should_chown(&self, st: &FileStat) -> bool {
        match self.policy {
            Policy::FromUid(uid) => st.st_uid == uid,
            Policy::Normalize => {
                (st.st_uid != self.uid || st.st_gid != self.gid) && (is_dir(st) || st.st_nlink == 1)
            }
        }
    }

    fn visit(
        &mut self,
        parent: BorrowedFd<'_>,
        name: &CStr,
        depth: usize,
    ) -> Result<(), AdoptError> {
        self.report.visited += 1;
        if self.report.visited > self.max_entries {
            return Err(AdoptError::TooManyEntries(self.max_entries));
        }
        // An O_PATH handle on the entry itself (a symlink stays a symlink).
        let handle = openat(
            parent,
            name,
            OFlag::O_PATH | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(io)?;
        let st = fstat(&handle).map_err(io)?;
        if st.st_dev != self.dev {
            self.report.skipped_other_fs += 1;
            return Ok(());
        }
        if self.skip == Some((st.st_dev, st.st_ino)) {
            return Ok(());
        }
        if self.should_chown(&st) {
            fchownat(
                &handle,
                "",
                Some(Uid::from_raw(self.uid)),
                Some(Gid::from_raw(self.gid)),
                AtFlags::AT_EMPTY_PATH,
            )
            .map_err(io)?;
            self.report.changed += 1;
        }
        if !is_dir(&st) {
            return Ok(());
        }
        if depth >= MAX_DEPTH {
            return Err(AdoptError::TooDeep);
        }
        let dir_fd = openat(parent, name, DIR_FLAGS, Mode::empty()).map_err(io)?;
        let check = fstat(&dir_fd).map_err(io)?;
        if check.st_ino != st.st_ino || check.st_dev != st.st_dev {
            return Err(AdoptError::Changed);
        }
        let mut dir = Dir::from_fd(dir_fd).map_err(io)?;
        let mut names: Vec<CString> = Vec::new();
        for entry in dir.iter() {
            let entry = entry.map_err(io)?;
            let entry_name = entry.file_name();
            if entry_name == c"." || entry_name == c".." {
                continue;
            }
            names.push(entry_name.to_owned());
            if self.report.visited + names.len() as u64 > self.max_entries {
                return Err(AdoptError::TooManyEntries(self.max_entries));
            }
        }
        for child in names {
            self.visit(dir.as_fd(), &child, depth + 1)?;
        }
        Ok(())
    }
}

/// `(device, inode)` of an open file, to name a directory the walk skips.
pub fn file_id(fd: &impl AsFd) -> std::io::Result<(u64, u64)> {
    fstat(fd)
        .map(|st| (st.st_dev, st.st_ino))
        .map_err(|e| std::io::Error::from_raw_os_error(e as i32))
}

/// Give the tree at `path` to `uid:gid` without following symlinks or leaving
/// its filesystem, skipping the directory whose `(device, inode)` is `skip`.
/// Used at start-up for `/config` (PUID). `path` itself is trusted (an
/// environment setting); everything below it is opened without following
/// symlinks.
pub fn own_tree(
    path: &Path,
    uid: u32,
    gid: u32,
    skip: Option<(u64, u64)>,
    max_entries: u64,
) -> Result<AdoptReport, AdoptError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("/"));
    let name = path.file_name().ok_or(AdoptError::InvalidName)?;
    let c_name = CString::new(name.as_encoded_bytes()).map_err(|_| AdoptError::InvalidName)?;
    let parent_fd = open(
        parent,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(io)?;
    own_tree_at(parent_fd.as_fd(), &c_name, uid, gid, skip, max_entries)
}

/// Like [`own_tree`] for the entry `name` inside the directory `parent`
/// (opened by the caller without following symlinks). `skip` is the
/// `(dev, inode)` of a directory to leave alone.
pub fn own_tree_at(
    parent: BorrowedFd<'_>,
    name: &CStr,
    uid: u32,
    gid: u32,
    skip: Option<(u64, u64)>,
    max_entries: u64,
) -> Result<AdoptReport, AdoptError> {
    let st = nix::sys::stat::fstatat(parent, name, AtFlags::AT_SYMLINK_NOFOLLOW).map_err(io)?;
    let mut walker = Walker {
        uid,
        gid,
        policy: Policy::Normalize,
        dev: st.st_dev,
        skip,
        max_entries,
        report: AdoptReport::default(),
    };
    walker.visit(parent, name, 0)?;
    Ok(walker.report)
}

/// Give `Downloads/In Progress/<name>` (file or tree) to PUID:PGID. Blocking.
pub fn adopt(config: &AdoptConfig, name: &str) -> Result<AdoptReport, AdoptError> {
    validate_name(name)?;
    let in_progress = open_in_progress(&config.stash_root)?;
    let root_stat = fstat(&in_progress).map_err(io)?;
    let c_name = CString::new(name).map_err(|_| AdoptError::InvalidName)?;
    match nix::sys::stat::fstatat(
        &in_progress,
        c_name.as_c_str(),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(_) => {}
        Err(Errno::ENOENT) => return Err(AdoptError::NotFound(name.to_string())),
        Err(e) => return Err(io(e)),
    }
    let mut walker = Walker {
        uid: config.puid,
        gid: config.pgid,
        policy: Policy::FromUid(config.engine_uid),
        dev: root_stat.st_dev,
        skip: None,
        max_entries: config.max_entries,
        report: AdoptReport::default(),
    };
    walker.visit(in_progress.as_fd(), &c_name, 0)?;
    Ok(walker.report)
}

/// Bind the adopt socket: owner PUID, mode 0600.
pub fn bind(path: &Path, puid: u32, pgid: u32) -> std::io::Result<UnixListener> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    std::os::unix::fs::chown(path, Some(puid), Some(pgid))?;
    Ok(listener)
}

/// Serve adopt requests until `cancel`. Only peers with uid PUID are answered.
pub async fn serve(listener: UnixListener, config: Arc<AdoptConfig>, cancel: CancellationToken) {
    let limit = Arc::new(Semaphore::new(4));
    loop {
        let accepted = tokio::select! {
            _ = cancel.cancelled() => return,
            r = listener.accept() => r,
        };
        let Ok((stream, _)) = accepted else { continue };
        let config = config.clone();
        let limit = limit.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, config, limit).await {
                tracing::debug!(error = %e, "adopt connection ended with an error");
            }
        });
    }
}

async fn handle(
    stream: UnixStream,
    config: Arc<AdoptConfig>,
    limit: Arc<Semaphore>,
) -> std::io::Result<()> {
    let peer_uid = stream.peer_cred()?.uid();
    let (read, mut write) = stream.into_split();
    let response = if peer_uid != config.puid {
        tracing::warn!(
            peer_uid,
            "refused an adopt request from a user other than PUID"
        );
        SupervisorResponse {
            ok: false,
            error: Some("Only the web server may use this socket.".into()),
        }
    } else {
        let mut line = Vec::new();
        let mut reader = BufReader::new(read.take(MAX_REQUEST as u64));
        let read_result =
            tokio::time::timeout(REQUEST_TIMEOUT, reader.read_until(b'\n', &mut line)).await;
        match read_result {
            Ok(Ok(_)) => respond(&line, config, limit).await,
            _ => SupervisorResponse {
                ok: false,
                error: Some("The request was not received in time.".into()),
            },
        }
    };
    let mut out = serde_json::to_vec(&response).map_err(std::io::Error::other)?;
    out.push(b'\n');
    write.write_all(&out).await?;
    write.shutdown().await
}

async fn respond(
    line: &[u8],
    config: Arc<AdoptConfig>,
    limit: Arc<Semaphore>,
) -> SupervisorResponse {
    let request = match serde_json::from_slice::<SupervisorRequest>(line) {
        Ok(request) => request,
        Err(_) => {
            return SupervisorResponse {
                ok: false,
                error: Some("The request is not valid.".into()),
            };
        }
    };
    match request {
        SupervisorRequest::Adopt { name } => {
            let Ok(_permit) = limit.acquire_owned().await else {
                return SupervisorResponse {
                    ok: false,
                    error: Some("The supervisor is shutting down.".into()),
                };
            };
            let name_for_log = name.clone();
            let result = tokio::task::spawn_blocking(move || adopt(&config, &name)).await;
            match result {
                Ok(Ok(report)) => {
                    tracing::info!(name = %name_for_log, visited = report.visited, changed = report.changed, "adopted a finished download");
                    SupervisorResponse {
                        ok: true,
                        error: None,
                    }
                }
                Ok(Err(e)) => {
                    tracing::warn!(name = %name_for_log, error = %e, "adopt failed");
                    SupervisorResponse {
                        ok: false,
                        error: Some(e.to_string()),
                    }
                }
                Err(_) => SupervisorResponse {
                    ok: false,
                    error: Some("The adopt task failed.".into()),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    const ENGINE: u32 = 6881;
    const PUID: u32 = 4242;
    const PGID: u32 = 4343;

    fn is_root() -> bool {
        nix::unistd::geteuid().is_root()
    }

    fn lchown(path: &Path, uid: u32, gid: u32) {
        std::os::unix::fs::lchown(path, Some(uid), Some(gid)).unwrap();
    }

    fn owner(path: &Path) -> (u32, u32) {
        let m = std::fs::symlink_metadata(path).unwrap();
        (m.uid(), m.gid())
    }

    fn setup() -> (tempfile::TempDir, PathBuf, AdoptConfig) {
        let dir = tempfile::tempdir().unwrap();
        let in_progress = dir.path().join("stash/Downloads/In Progress");
        std::fs::create_dir_all(&in_progress).unwrap();
        let config = AdoptConfig::new(dir.path().join("stash"), PUID, PGID, ENGINE);
        (dir, in_progress, config)
    }

    #[test]
    fn names_must_be_single_components() {
        for bad in ["", ".", "..", "a/b", "../etc", "/etc", "a\0b"] {
            assert_eq!(validate_name(bad), Err(AdoptError::InvalidName), "{bad:?}");
        }
        assert!(validate_name("Movie (2019) [1080p]").is_ok());
        assert!(validate_name(".hidden").is_ok());
        assert!(validate_name(&"x".repeat(256)).is_err());
    }

    #[test]
    fn adopts_tree_without_following_symlinks() {
        if !is_root() {
            eprintln!("skipped: needs root");
            return;
        }
        let (dir, in_progress, config) = setup();
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, b"secret").unwrap();
        lchown(&outside, ENGINE, PGID);
        let outside_dir = dir.path().join("outside-dir");
        std::fs::create_dir(&outside_dir).unwrap();
        std::fs::write(outside_dir.join("f"), b"x").unwrap();
        lchown(&outside_dir, ENGINE, PGID);
        lchown(&outside_dir.join("f"), ENGINE, PGID);
        let root_owned = dir.path().join("root-owned");
        std::fs::write(&root_owned, b"root").unwrap();

        let torrent = in_progress.join("Show S01");
        std::fs::create_dir_all(torrent.join("Season 1/Extras")).unwrap();
        std::fs::write(torrent.join("Season 1/e01.mkv"), b"video").unwrap();
        std::fs::write(torrent.join("Season 1/Extras/x.mkv"), b"video").unwrap();
        std::os::unix::fs::symlink(&outside, torrent.join("link-to-file")).unwrap();
        std::os::unix::fs::symlink(&outside_dir, torrent.join("link-to-dir")).unwrap();
        // A hard link to a file the engine does not own must stay untouched.
        std::fs::hard_link(&root_owned, torrent.join("hardlink")).unwrap();
        for path in [
            torrent.clone(),
            torrent.join("Season 1"),
            torrent.join("Season 1/Extras"),
            torrent.join("Season 1/e01.mkv"),
            torrent.join("Season 1/Extras/x.mkv"),
            torrent.join("link-to-file"),
            torrent.join("link-to-dir"),
        ] {
            lchown(&path, ENGINE, PGID);
        }

        let report = adopt(&config, "Show S01").unwrap();
        assert_eq!(report.visited, 8);
        assert_eq!(report.changed, 7);
        for path in [
            torrent.clone(),
            torrent.join("Season 1/e01.mkv"),
            torrent.join("Season 1/Extras/x.mkv"),
            torrent.join("link-to-file"),
        ] {
            assert_eq!(owner(&path), (PUID, PGID), "{}", path.display());
        }
        // Symlink targets outside the tree keep their owner.
        assert_eq!(owner(&outside), (ENGINE, PGID));
        assert_eq!(owner(&outside_dir), (ENGINE, PGID));
        assert_eq!(owner(&outside_dir.join("f")), (ENGINE, PGID));
        assert_eq!(owner(&root_owned), (0, 0));

        // Idempotent.
        let again = adopt(&config, "Show S01").unwrap();
        assert_eq!(again.changed, 0);
    }

    #[test]
    fn adopts_single_file_and_refuses_symlinked_roots() {
        if !is_root() {
            eprintln!("skipped: needs root");
            return;
        }
        let (dir, in_progress, config) = setup();
        let file = in_progress.join("movie.mkv");
        std::fs::write(&file, b"m").unwrap();
        lchown(&file, ENGINE, PGID);
        assert_eq!(adopt(&config, "movie.mkv").unwrap().changed, 1);
        assert_eq!(owner(&file), (PUID, PGID));
        assert_eq!(
            adopt(&config, "missing"),
            Err(AdoptError::NotFound("missing".into()))
        );

        // A top-level symlink is adopted as a link; its target is not touched.
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        lchown(&target, ENGINE, PGID);
        std::os::unix::fs::symlink(&target, in_progress.join("evil")).unwrap();
        lchown(&in_progress.join("evil"), ENGINE, PGID);
        adopt(&config, "evil").unwrap();
        assert_eq!(owner(&target), (ENGINE, PGID));

        // `Downloads` replaced by a symlink: refused.
        let elsewhere = dir.path().join("elsewhere/In Progress");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("x"), b"x").unwrap();
        lchown(&elsewhere.join("x"), ENGINE, PGID);
        let downloads = dir.path().join("stash/Downloads");
        std::fs::rename(&downloads, dir.path().join("old-downloads")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("elsewhere"), &downloads).unwrap();
        assert_eq!(adopt(&config, "x"), Err(AdoptError::BadRoot));
        assert_eq!(owner(&elsewhere.join("x")), (ENGINE, PGID));
    }

    #[test]
    fn entry_limit_is_enforced() {
        if !is_root() {
            eprintln!("skipped: needs root");
            return;
        }
        let (_dir, in_progress, mut config) = setup();
        let torrent = in_progress.join("many");
        std::fs::create_dir(&torrent).unwrap();
        for i in 0..20 {
            std::fs::write(torrent.join(format!("f{i}")), b"").unwrap();
        }
        config.max_entries = 10;
        assert_eq!(adopt(&config, "many"), Err(AdoptError::TooManyEntries(10)));
    }

    #[test]
    fn own_tree_normalizes_but_skips_links_and_engine_dir() {
        if !is_root() {
            eprintln!("skipped: needs root");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("cache/thumbs")).unwrap();
        std::fs::create_dir_all(config.join("torrent/engine")).unwrap();
        std::fs::write(config.join("popcornstash.db"), b"db").unwrap();
        std::fs::write(config.join("cache/thumbs/a.webp"), b"img").unwrap();
        std::fs::write(config.join("torrent/engine/session.json"), b"{}").unwrap();
        lchown(&config.join("torrent/engine"), ENGINE, PGID);
        lchown(&config.join("torrent/engine/session.json"), ENGINE, PGID);
        let system = dir.path().join("shadow");
        std::fs::write(&system, b"root:x").unwrap();
        std::fs::hard_link(&system, config.join("planted")).unwrap();
        std::os::unix::fs::symlink(&system, config.join("symlink")).unwrap();

        let engine = nix::sys::stat::lstat(&config.join("torrent/engine")).unwrap();
        let report = own_tree(
            &config,
            PUID,
            PGID,
            Some((engine.st_dev, engine.st_ino)),
            1000,
        )
        .unwrap();
        assert!(report.changed >= 6, "{report:?}");
        for path in [
            "",
            "popcornstash.db",
            "cache",
            "cache/thumbs/a.webp",
            "torrent",
            "symlink",
        ] {
            assert_eq!(owner(&config.join(path)), (PUID, PGID), "{path}");
        }
        assert_eq!(owner(&system), (0, 0), "hard-linked system file untouched");
        assert_eq!(owner(&config.join("torrent/engine")), (ENGINE, PGID));
        assert_eq!(
            owner(&config.join("torrent/engine/session.json")),
            (ENGINE, PGID)
        );
        assert_eq!(
            own_tree(&config, PUID, PGID, None, 3),
            Err(AdoptError::TooManyEntries(3))
        );
    }

    #[tokio::test]
    async fn socket_checks_peer_and_answers() {
        if !is_root() {
            eprintln!("skipped: needs root");
            return;
        }
        let (dir, in_progress, _) = setup();
        let file = in_progress.join("a.mkv");
        std::fs::write(&file, b"m").unwrap();
        lchown(&file, ENGINE, PGID);
        let socket = dir.path().join("supervisor.sock");

        // The test process is root, so a config whose PUID is 0 lets us connect.
        let config = Arc::new(AdoptConfig::new(dir.path().join("stash"), 0, 0, ENGINE));
        let listener = bind(&socket, 0, 0).unwrap();
        assert_eq!(
            std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let cancel = CancellationToken::new();
        tokio::spawn(serve(listener, config, cancel.clone()));

        let ask = |line: &'static str| {
            let socket = socket.clone();
            async move {
                let mut stream = UnixStream::connect(&socket).await.unwrap();
                stream.write_all(line.as_bytes()).await.unwrap();
                let mut answer = String::new();
                BufReader::new(stream).read_line(&mut answer).await.unwrap();
                serde_json::from_str::<SupervisorResponse>(answer.trim()).unwrap()
            }
        };
        let ok = ask("{\"op\":\"adopt\",\"name\":\"a.mkv\"}\n").await;
        assert!(ok.ok, "{ok:?}");
        assert_eq!(owner(&file), (0, 0));
        let bad = ask("{\"op\":\"adopt\",\"name\":\"../x\"}\n").await;
        assert!(!bad.ok);
        let junk = ask("hello\n").await;
        assert!(!junk.ok);

        // A different PUID: the root peer is refused.
        let socket2 = dir.path().join("other.sock");
        let listener = bind(&socket2, 0, 0).unwrap();
        let config = Arc::new(AdoptConfig::new(
            dir.path().join("stash"),
            PUID,
            PGID,
            ENGINE,
        ));
        tokio::spawn(serve(listener, config, cancel.clone()));
        let mut stream = UnixStream::connect(&socket2).await.unwrap();
        stream
            .write_all(b"{\"op\":\"adopt\",\"name\":\"a.mkv\"}\n")
            .await
            .unwrap();
        let mut answer = String::new();
        BufReader::new(stream).read_line(&mut answer).await.unwrap();
        let refused: SupervisorResponse = serde_json::from_str(answer.trim()).unwrap();
        assert!(!refused.ok);
        assert!(refused.error.unwrap().contains("Only the web server"));
        cancel.cancel();
    }
}
