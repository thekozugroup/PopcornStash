//! Publish one adopted torrent root with a durable intent before the exclusive rename.
use sqlx::Connection;
use std::fs::{File, Metadata};
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use super::Library;
use super::cleanup::Root;
use super::flatten_downloads::recovery::sync_parent;
use super::scanner::normalize_rel;
use crate::paths::{COMPLETED, IN_PROGRESS, sanitize_filename};
use crate::{CoreError, Result, now};

fn changed() -> CoreError {
    CoreError::conflict("The finished torrent root changed. Its files were kept in place.")
}

// shfs supplies dev, inode and nanosecond mtime but no birth time. Use the
// rename-stable stamp through publication and first import. The private
// wrapper also has a random persistent witness independent of inode reuse.
fn identity(meta: &Metadata) -> Result<String> {
    if !(meta.is_dir() || meta.is_file() && meta.nlink() == 1) {
        return Err(changed());
    }
    let birth = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Ok(format!(
        "{}:{}:{}:{birth}",
        meta.dev(),
        meta.ino(),
        meta.is_dir()
    ))
}

fn stamp(meta: &Metadata) -> String {
    format!(
        "{}:{}:{}",
        if meta.is_file() { meta.len() } else { 0 },
        meta.mtime(),
        meta.mtime_nsec()
    )
}

fn metadata(root: &Root, rel: &str) -> Result<Option<Metadata>> {
    match root.metadata(rel) {
        Ok(m) => Ok(Some(m)),
        Err(CoreError::Io(e)) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[derive(sqlx::FromRow)]
struct Intent {
    source: String,
    destination: String,
    root_identity: String,
    root_stamp: String,
    private_key: Option<String>,
    wrapper_identity: Option<String>,
    payload_leaf: Option<String>,
    import_started: bool,
    retirement: String,
}

impl Intent {
    fn check(&self, meta: &Metadata) -> Result<()> {
        if identity(meta)? != self.root_identity || stamp(meta) != self.root_stamp {
            return Err(changed());
        }
        Ok(())
    }

    fn marker(&self) -> Result<String> {
        self.private_key
            .as_ref()
            .map(|k| format!(".popcornstash-handoff-{k}"))
            .ok_or_else(changed)
    }

    fn child(&self) -> Result<&str> {
        self.payload_leaf.as_deref().ok_or_else(changed)
    }

    fn private_dir(&self, root: &Root) -> Result<File> {
        self.witnessed_dir(root, &self.destination)
    }

    fn witnessed_dir(&self, root: &Root, rel: &str) -> Result<File> {
        let dir = root.private_handoff_directory(rel)?;
        if let Some(expected) = &self.wrapper_identity
            && identity(&dir.metadata()?)? != *expected
        {
            return Err(changed());
        }
        let marker = self.marker()?;
        let witness = nix::sys::stat::fstatat(
            &dir,
            marker.as_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(std::io::Error::from)?;
        if witness.st_mode & libc::S_IFMT != libc::S_IFREG
            || witness.st_size != 0
            || witness.st_nlink != 1
        {
            return Err(changed());
        }
        Ok(dir)
    }

    // Before import starts there must be exactly one payload, plus our marker.
    fn only_witness(&self, dir: &File) -> Result<bool> {
        let marker = self.marker()?;
        let mut entries =
            nix::dir::Dir::from_fd(dir.try_clone()?.into()).map_err(std::io::Error::from)?;
        for entry in entries.iter() {
            let entry = entry.map_err(std::io::Error::from)?;
            let name = entry.file_name().to_bytes();
            if !matches!(name, b"." | b"..") && name != marker.as_bytes() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn check_entries(&self, dir: &File, allow_payload: bool) -> Result<()> {
        let marker = self.marker()?;
        let child = self.child()?;
        let mut entries =
            nix::dir::Dir::from_fd(dir.try_clone()?.into()).map_err(std::io::Error::from)?;
        for e in entries.iter() {
            let e = e.map_err(std::io::Error::from)?;
            let name = e.file_name().to_bytes();
            if !matches!(name, b"." | b"..")
                && name != marker.as_bytes()
                && !(allow_payload && name == child.as_bytes())
            {
                return Err(changed());
            }
        }
        Ok(())
    }

    // With an existing intent, only allocate the wrapper after proving the
    // original is still at source. A crash after mkdir and before COMMIT leaves
    // an unclaimed directory; retry skips it without adopting or deleting it.
    async fn reserve(
        lib: &Library,
        id: &str,
        source: &str,
        meta: &Metadata,
        wrapping_existing: bool,
    ) -> Result<Self> {
        let name = sanitize_filename(source.rsplit('/').next().ok_or_else(changed)?);
        let path = Path::new(&name);
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(&name);
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map_or(String::new(), |s| format!(".{s}"));
        let root_identity = identity(meta)?;
        let root_stamp = stamp(meta);
        let private_key = (wrapping_existing || meta.created().is_err()).then(crate::new_id);
        let payload_leaf = private_key.as_ref().map(|key| {
            if meta.is_dir() {
                format!("payload-{key}")
            } else {
                source.rsplit('/').next().unwrap_or(&name).to_owned()
            }
        });
        let mut conn = lib.db.acquire().await?;
        sqlx::query("PRAGMA synchronous=FULL")
            .execute(&mut *conn)
            .await?;
        let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
        for n in 1..=10_000 {
            let leaf = if n == 1 {
                name.clone()
            } else {
                format!("{stem} ({n}){ext}")
            };
            let destination = format!("{COMPLETED}/{leaf}");
            match std::fs::symlink_metadata(lib.layout.completed().join(&leaf)) {
                Ok(_) => continue,
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            let busy: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM torrent_handoffs WHERE destination=?1 AND download_id<>?2 AND retirement IN ('active','retiring')
                    UNION ALL SELECT 1 FROM downloads d WHERE rel_path=?1 COLLATE NOCASE
                    AND NOT EXISTS(SELECT 1 FROM torrent_handoffs h WHERE h.download_id=d.id AND h.retirement IN ('staged','retired'))
                    UNION ALL SELECT 1 FROM jobs j WHERE kind='sort' AND state IN ('queued','running')
                    AND json_extract(payload,'$.rel_path')=?1 COLLATE NOCASE
                    AND NOT EXISTS(SELECT 1 FROM torrent_handoffs h WHERE h.download_id=json_extract(j.payload,'$.download_id')
                    AND h.retirement IN ('staged','retired')))",
            )
            .bind(&destination).bind(id)
            .fetch_one(&mut *tx)
            .await?;
            if busy {
                continue;
            }
            let inserted = sqlx::query("INSERT INTO torrent_handoffs(download_id,source,destination,root_identity,root_stamp,private_key,payload_leaf)
                SELECT id,rel_path,?1,?2,?3,?4,?5 FROM downloads WHERE id=?6 AND kind='torrent'
                AND state='completed' AND engine_id IS NULL AND rel_path=?7
                ON CONFLICT(download_id) DO UPDATE SET destination=excluded.destination,private_key=excluded.private_key,
                payload_leaf=excluded.payload_leaf WHERE ?8 AND torrent_handoffs.wrapper_identity IS NULL
                AND torrent_handoffs.root_identity=excluded.root_identity AND torrent_handoffs.root_stamp=excluded.root_stamp")
                .bind(&destination).bind(&root_identity).bind(&root_stamp).bind(&private_key).bind(&payload_leaf)
                .bind(id).bind(source).bind(wrapping_existing).execute(&mut *tx).await?;
            if inserted.rows_affected() != 1 {
                return Err(changed());
            }
            tx.commit().await?;
            return Ok(Self {
                source: source.into(),
                destination,
                root_identity,
                root_stamp,
                private_key,
                wrapper_identity: None,
                payload_leaf,
                import_started: false,
                retirement: "active".into(),
            });
        }
        Err(CoreError::conflict(
            "No free destination for the finished torrent.",
        ))
    }

    /// The secret marker was committed before mkdir. A crash after writing it
    /// can finish recording the directory's identity; a path lacking the
    /// witness is never adopted, even if its permissions and name look right.
    async fn claim(&mut self, lib: &Library, root: &Root, id: &str) -> Result<()> {
        if self.wrapper_identity.is_some() {
            return Ok(());
        }
        let original = root.metadata(&self.source)?;
        self.check(&original)?;
        let dir = match root.claim_handoff_directory(&self.destination, &self.marker()?) {
            Ok(dir) => dir,
            Err(CoreError::Io(e)) if e.kind() == ErrorKind::AlreadyExists => {
                if let Ok(dir) = self.private_dir(root) {
                    dir
                } else {
                    *self = Self::reserve(lib, id, &self.source, &original, true).await?;
                    root.claim_handoff_directory(&self.destination, &self.marker()?)?
                }
            }
            Err(e) => return Err(e),
        };
        self.check_entries(&dir, false)?;
        let proof = identity(&dir.metadata()?)?;
        dir.sync_all()?;
        sync_parent(root, &self.destination)?;
        let mut conn = lib.db.acquire().await?;
        sqlx::query("PRAGMA synchronous=FULL")
            .execute(&mut *conn)
            .await?;
        let stored = sqlx::query(
            "UPDATE torrent_handoffs SET wrapper_identity=?
            WHERE download_id=? AND destination=? AND private_key=? AND wrapper_identity IS NULL",
        )
        .bind(&proof)
        .bind(id)
        .bind(&self.destination)
        .bind(&self.private_key)
        .execute(&mut *conn)
        .await?;
        if stored.rows_affected() != 1 {
            return Err(changed());
        }
        self.wrapper_identity = Some(proof);
        Ok(())
    }

    fn publish_private(&self, root: &Root) -> Result<()> {
        let dir = self.private_dir(root)?;
        self.check_entries(&dir, true)?;
        let child = self.child()?;
        let payload = format!("{}/{child}", self.destination);
        if let Some(meta) = metadata(root, &payload)? {
            self.check(&meta)?;
        } else {
            self.check(&root.metadata(&self.source)?)?;
            root.rename_into_handoff(&self.source, &dir, child)?;
        }
        dir.sync_all()?;
        self.private_dir(root)?;
        self.check(&root.metadata(&payload)?)
    }
}

impl Library {
    /// Without a prepared root, replay an existing intent only. This must precede
    /// even looking at the old source: a later transfer may already live there.
    pub async fn handoff_torrent(
        &self,
        id: &str,
        source: &str,
        prepared: Option<&Metadata>,
    ) -> Result<bool> {
        self.handoff_torrent_with(id, source, prepared, Root::rename)
            .await
    }

    pub(super) async fn handoff_torrent_with(
        &self,
        id: &str,
        source: &str,
        prepared: Option<&Metadata>,
        rename: fn(&Root, &str, &str) -> Result<()>,
    ) -> Result<bool> {
        if normalize_rel(source)? != source
            || Path::new(source).parent() != Some(Path::new(IN_PROGRESS))
        {
            return Err(changed());
        }
        let _moves = self.lock_moves().await?;
        let intent: Option<Intent> =
            sqlx::query_as("SELECT * FROM torrent_handoffs WHERE download_id=?")
                .bind(id)
                .fetch_optional(&self.db)
                .await?;
        let root = Root::open(self.layout.root())?;
        let mut intent = match (intent, prepared) {
            (Some(intent), _) => intent,
            (None, Some(prepared)) => {
                let meta = root.metadata(source)?;
                if identity(prepared)? != identity(&meta)? {
                    return Err(changed());
                }
                Intent::reserve(self, id, source, &meta, false).await?
            }
            (None, None) => return Ok(false),
        };
        if intent.source != source
            || normalize_rel(&intent.destination)? != intent.destination
            || Path::new(&intent.destination).parent() != Some(Path::new(COMPLETED))
        {
            return Err(changed());
        }
        // Recheck eligibility before any filesystem mutation, including on a stale retry.
        let eligible: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM downloads WHERE id=? AND kind='torrent'
            AND state='completed' AND engine_id IS NULL AND rel_path=?)",
        )
        .bind(id)
        .bind(source)
        .fetch_one(&self.db)
        .await?;
        if !eligible {
            return Ok(true);
        }
        if intent.private_key.is_none() {
            if let Some(meta) = metadata(&root, &intent.destination)? {
                intent.check(&meta)?;
            } else {
                intent.check(&root.metadata(source)?)?;
                match rename(&root, source, &intent.destination) {
                    Ok(()) => {}
                    Err(CoreError::Io(e))
                        if matches!(
                            e.raw_os_error(),
                            Some(libc::EINVAL | libc::EOPNOTSUPP | libc::ENOSYS)
                        ) =>
                    {
                        let meta = root.metadata(source)?;
                        intent.check(&meta)?;
                        intent = Intent::reserve(self, id, source, &meta, true).await?;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        if intent.private_key.is_some() {
            intent.claim(self, &root, id).await?;
            intent.publish_private(&root)?;
        } else {
            intent.check(&root.metadata(&intent.destination)?)?;
        }
        sync_parent(&root, &intent.destination)?;
        sync_parent(&root, source)?;
        // Bind provenance and make the owned sort runnable in a single commit.
        let t = now();
        let mut conn = self.db.acquire().await?;
        sqlx::query("PRAGMA synchronous=FULL")
            .execute(&mut *conn)
            .await?;
        let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
        let updated = sqlx::query("UPDATE downloads SET rel_path=?,state='sorting',error=NULL,updated_at=?
            WHERE id=? AND kind='torrent' AND state='completed' AND engine_id IS NULL AND rel_path=?")
            .bind(&intent.destination).bind(t).bind(id).bind(source).execute(&mut *tx).await?;
        if updated.rows_affected() != 1 {
            return Err(changed());
        }
        sqlx::query("UPDATE torrent_handoffs SET published=1 WHERE download_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO jobs(kind,payload,state,run_after,created_at,updated_at)
            VALUES ('sort',json_object('rel_path',?,'download_id',?),'queued',?,?,?)",
        )
        .bind(&intent.destination)
        .bind(id)
        .bind(t)
        .bind(t)
        .bind(t)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.jobs.wake_workers();
        Ok(true)
    }
}

/// Check a Completed path at consumption time, including watcher jobs that do
/// not carry an id. An absent, already published root may be reconciled by the
/// existing import journal (e.g. the sorter already relocated a single file).
pub(super) async fn owner(
    lib: &Library,
    rel: &str,
    requested: Option<&str>,
) -> Result<Option<String>> {
    if !rel.starts_with("Downloads/Completed/") {
        return Ok(None);
    }
    let top = super::watcher::top_level_entry(rel).ok_or_else(changed)?;
    #[derive(sqlx::FromRow)]
    struct Claimed {
        download_id: String,
        published: bool,
        #[sqlx(flatten)]
        intent: Intent,
    }
    let claim: Option<Claimed> =
        sqlx::query_as("SELECT * FROM torrent_handoffs WHERE download_id=?2 OR (destination=?1 AND retirement IN ('active','retiring'))
         ORDER BY (download_id=?2) DESC LIMIT 1")
            .bind(&top)
            .bind(requested)
            .fetch_optional(&lib.db)
            .await?;
    let Some(Claimed {
        download_id: id,
        published,
        intent,
    }) = claim
    else {
        return Ok(None);
    };
    if !published
        || intent.retirement != "active"
        || top != intent.destination
        || requested.is_some_and(|r| r != id)
    {
        return Err(changed());
    }
    let root = Root::open(lib.layout.root())?;
    if let Some(meta) = metadata(&root, &top)? {
        if intent.private_key.is_some() {
            let dir = intent.private_dir(&root)?;
            if intent.import_started {
                // A directory can be emptied by the flatten journal. While
                // the original root still exists it must keep its identity.
                if let Some(m) = metadata(&root, &format!("{top}/{}", intent.child()?))?
                    && (identity(&m)? != intent.root_identity
                        || m.is_file() && stamp(&m) != intent.root_stamp)
                {
                    return Err(changed());
                }
            } else {
                intent.check_entries(&dir, true)?;
                intent.check(&root.metadata(&format!("{top}/{}", intent.child()?))?)?;
            }
        } else if intent.import_started {
            if identity(&meta)? != intent.root_identity {
                return Err(changed());
            }
        } else {
            intent.check(&meta)?;
        }
    }
    Ok(Some(id))
}

/// Called from the import planner under the move lock. Only this exact private
/// witness is excluded from the media tree. Recovery of subsequent moves uses
/// the importer's separate per-file journal.
pub(super) async fn import_marker(lib: &Library, id: &str, source: &str) -> Result<Option<String>> {
    let key: Option<String> = sqlx::query_scalar(
        "UPDATE torrent_handoffs SET import_started=1
        WHERE download_id=? AND destination=? AND published AND retirement='active' RETURNING private_key",
    )
    .bind(id)
    .bind(source)
    .fetch_optional(&lib.db)
    .await?
    .flatten();
    Ok(key.map(|k| format!("{source}/.popcornstash-handoff-{k}")))
}

// Every ordinary rename during retirement targets an absent, unpredictable
// entry in this server-owned 0700 directory. No public rmdir by a stale name.
const RETIREMENT_DIR: &str = ".popcornstash/TorrentHandoffs";

async fn retirement(lib: &Library, id: &str, from: &str, to: &str) -> Result<()> {
    let mut conn = lib.db.acquire().await?;
    sqlx::query("PRAGMA synchronous=FULL")
        .execute(&mut *conn)
        .await?;
    let changed = sqlx::query(
        "UPDATE torrent_handoffs SET retirement=? WHERE download_id=? AND retirement=?",
    )
    .bind(to)
    .bind(id)
    .bind(from)
    .execute(&mut *conn)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(CoreError::conflict("The torrent handoff changed."));
    }
    Ok(())
}

/// The sort and the item links are committed before claiming an empty root.
/// Recovery never relies on a public name after the witness has been removed.
pub(super) async fn remove_empty_wrapper(lib: &Library, rel: &str) -> Result<()> {
    let Some(top) = super::watcher::top_level_entry(rel) else {
        return Ok(());
    };
    let row: Option<String> = sqlx::query_scalar(
        "SELECT h.download_id FROM torrent_handoffs h
        LEFT JOIN downloads d ON d.id=h.download_id WHERE h.destination=? AND h.published
        AND h.retirement IN ('active','retiring') AND (d.id IS NULL OR d.state='sorted')",
    )
    .bind(&top)
    .fetch_optional(&lib.db)
    .await?;
    if let Some(id) = row
        && let Err(error) = retire(lib, &id).await
    {
        lib.retirement_retry_at
            .store(now() + 30, std::sync::atomic::Ordering::Relaxed);
        return Err(error);
    }
    Ok(())
}

async fn retire(lib: &Library, id: &str) -> Result<()> {
    let intent: Intent = sqlx::query_as("SELECT * FROM torrent_handoffs WHERE download_id=?")
        .bind(id)
        .fetch_one(&lib.db)
        .await?;
    let root = Root::open(lib.layout.root())?;
    if intent.retirement == "active" {
        let Some(public) = metadata(&root, &intent.destination)? else {
            return retirement(lib, id, "active", "retired").await;
        };
        if intent.private_key.is_none() {
            if identity(&public)? != intent.root_identity {
                return retirement(lib, id, "active", "retired").await;
            }
            if public.is_dir() {
                match root.remove_empty_organized_dir(&intent.destination) {
                    Ok(()) => return retirement(lib, id, "active", "retired").await,
                    Err(CoreError::Io(e))
                        if matches!(e.raw_os_error(), Some(libc::ENOTEMPTY | libc::EEXIST)) => {}
                    Err(e) => return Err(e),
                }
            }
            return Ok(());
        }
        let dir = intent.private_dir(&root)?;
        if !intent.only_witness(&dir)? {
            return Ok(());
        }
        retirement(lib, id, "active", "retiring").await?;
    }
    let key = intent.private_key.as_deref().ok_or_else(changed)?;
    let stage_dir = root.handoff_retirement_directory(RETIREMENT_DIR)?;
    let stage = format!("{RETIREMENT_DIR}/{key}");
    if intent.retirement != "staged" {
        if metadata(&root, &stage)?.is_none() {
            let dir = intent.private_dir(&root)?;
            if !intent.only_witness(&dir)? {
                return Err(changed());
            }
            root.rename_into_handoff(&intent.destination, &stage_dir, key)?;
        }
        let staged = intent.witnessed_dir(&root, &stage)?;
        if !intent.only_witness(&staged)? {
            return Err(changed());
        }
        stage_dir.sync_all()?;
        sync_parent(&root, &intent.destination)?;
        retirement(lib, id, "retiring", "staged").await?;
    }
    if let Some(meta) = metadata(&root, &stage)? {
        if Some(identity(&meta)?) != intent.wrapper_identity {
            return Err(changed());
        }
        let dir = root.private_handoff_directory(&stage)?;
        let marker = intent.marker()?;
        // Once staged, a crash between unlink and rmdir is unambiguous: the
        // public location cannot address this identity-bound private leaf.
        if !intent.only_witness(&dir)? {
            return Err(changed());
        }
        match nix::unistd::unlinkat(
            &dir,
            marker.as_str(),
            nix::unistd::UnlinkatFlags::NoRemoveDir,
        ) {
            Ok(()) | Err(nix::errno::Errno::ENOENT) => {}
            Err(e) => return Err(std::io::Error::from(e).into()),
        }
        dir.sync_all()?;
        root.remove_empty_organized_dir(&stage)?;
    }
    stage_dir.sync_all()?;
    retirement(lib, id, "staged", "retired").await
}

/// Called with the same move lock as the sorter, once on startup. A single
/// damaged root must not prevent independent downloads from recovering.
pub(super) async fn recover_retirements(lib: &Library) -> Result<bool> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT h.download_id FROM torrent_handoffs h
        LEFT JOIN downloads d ON d.id=h.download_id WHERE h.published AND h.retirement<>'retired'
        AND (d.id IS NULL OR d.state='sorted')",
    )
    .fetch_all(&lib.db)
    .await?;
    let mut recovered = true;
    for id in ids {
        if let Err(error) = retire(lib, &id).await {
            recovered = false;
            tracing::warn!(%error, download=%id, "could not retire a torrent handoff");
        }
    }
    Ok(recovered)
}
