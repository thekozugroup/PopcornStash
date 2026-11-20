//! Physical membership of a released torrent, across flattening and later review.
use super::download_import::identity;
use super::*;
use crate::internal_api::{ReleaseWitness, TorrentFile, TorrentFiles};
use crate::library::cleanup::Root;
use crate::models::DownloadKind;
use sqlx::Connection;
use std::collections::HashSet;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;

fn changed() -> CoreError {
    CoreError::conflict(
        "The completed torrent no longer matches its selected files. Its files were kept in place.",
    )
}

#[derive(serde::Deserialize)]
struct FrozenManifest {
    #[serde(flatten)]
    selection: TorrentFiles,
    release_witness: Option<ReleaseWitness>,
}

enum Proof {
    LegacySize(u64),
    Observed([u64; 5]),
}

impl Proof {
    fn for_selected(file: &TorrentFile, release: Option<&ReleaseWitness>) -> Result<Self> {
        let Some(release) = release else {
            return Ok(Self::LegacySize(file.size));
        };
        let witness = release.files.get(&file.index).ok_or_else(changed)?;
        if witness.size != file.size {
            return Err(changed());
        }
        // Adoption changes uid and ctime. Publication and flattening preserve
        // the five fields used by the import journal.
        Ok(Self::Observed([
            witness.dev,
            witness.ino,
            witness.size,
            witness.mtime_sec as u64,
            witness.mtime_nsec as u64,
        ]))
    }
}

pub(super) struct Members {
    pub scan: String,
    pub fresh: bool,
    dirs: HashMap<PathBuf, Option<String>>,
    files: HashMap<String, Proof>,
}

fn dir_identity(m: &Metadata) -> String {
    let birth = m
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
    format!(
        "{}:{}:{}",
        m.dev(),
        m.ino(),
        birth.map_or(0, |d| d.as_nanos())
    )
}

fn parents<'a>(file: &'a str, source: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    Path::new(file)
        .ancestors()
        .skip(1)
        .take_while(move |p| p.starts_with(source))
        .map(Path::to_owned)
}

impl Members {
    pub async fn load(
        lib: &Library,
        root: &Root,
        download: &DownloadRow,
        source: &str,
    ) -> Result<Option<Self>> {
        if download.kind != DownloadKind::Torrent {
            return Ok(None);
        }
        let saved: Vec<(String, Option<String>, String)> = sqlx::query_as(
            "SELECT original_path,current_path,identity FROM torrent_import_files WHERE download_id=?"
        ).bind(&download.id).fetch_all(&lib.db).await?;
        let mut members = Self {
            scan: source.into(),
            fresh: saved.is_empty(),
            dirs: HashMap::new(),
            files: HashMap::new(),
        };
        if !members.fresh {
            for (original, current, stamp) in saved {
                if !Path::new(&original).starts_with(source) {
                    return Err(changed());
                }
                if let Some(current) = current {
                    members
                        .files
                        .insert(current, Proof::Observed(serde_json::from_str(&stamp)?));
                }
            }
            let dirs: Vec<(String, String)> =
                sqlx::query_as("SELECT path,identity FROM torrent_import_dirs WHERE download_id=?")
                    .bind(&download.id)
                    .fetch_all(&lib.db)
                    .await?;
            members
                .dirs
                .extend(dirs.into_iter().map(|(p, stamp)| (p.into(), Some(stamp))));
            return Ok(Some(members));
        }

        // The physical payload may be the public root, a generated directory
        // leaf, or the original filename inside a private wrapper.
        let handoff: Option<(String, Option<String>, bool)> = sqlx::query_as(
            "SELECT source,payload_leaf,import_started FROM torrent_handoffs WHERE download_id=? AND destination=? AND published"
        ).bind(&download.id).bind(source).fetch_optional(&lib.db).await?;
        if download.torrent_files_json.is_none() && handoff.is_none() {
            return Ok(None); // Historical import without a frozen manifest or a handoff.
        }
        let mut original = source;
        if let Some((before, leaf, started)) = &handoff {
            if *started {
                return Err(changed());
            }
            original = before;
            if let Some(leaf) = leaf {
                if leaf.contains('/') || normalize_rel(leaf)? != *leaf {
                    return Err(changed());
                }
                members.scan = format!("{source}/{leaf}");
            }
        }
        let meta = root.metadata(&members.scan)?;
        if let Some(json) = &download.torrent_files_json {
            let FrozenManifest {
                selection: manifest,
                release_witness,
            } = serde_json::from_str(json)?;
            if let Some(witness) = &release_witness
                && (meta.dev(), meta.ino()) != (witness.root_dev, witness.root_ino)
            {
                return Err(changed());
            }
            let mut paths = HashSet::new();
            let mut indices = HashSet::new();
            for file in &manifest.files {
                if normalize_rel(&file.path)? != file.path
                    || !paths.insert(&file.path)
                    || !indices.insert(file.index)
                {
                    return Err(changed());
                }
                let path = if meta.is_dir() {
                    format!("{}/{}", members.scan, file.path)
                } else if meta.is_file()
                    && manifest.files.len() == 1
                    && Path::new(original).file_name() == Some(file.path.as_ref())
                {
                    members.scan.clone()
                } else {
                    return Err(changed());
                };
                members
                    .dirs
                    .extend(parents(&path, source).map(|p| (p, None)));
                if file.selected {
                    members
                        .files
                        .insert(path, Proof::for_selected(file, release_witness.as_ref())?);
                }
            }
        } else if meta.is_file() {
            // A legacy single-file handoff proves the one payload object itself.
            members
                .dirs
                .extend(parents(&members.scan, source).map(|p| (p, None)));
            members
                .files
                .insert(members.scan.clone(), Proof::Observed(identity(&meta)));
        }
        if members.files.is_empty() {
            return Err(changed());
        }
        if members.scan != source {
            members.check_dir(source, &root.metadata(source)?)?;
        }
        Ok(Some(members))
    }

    pub fn check_dir(&mut self, rel: &str, m: &Metadata) -> Result<()> {
        let stamp = self.dirs.get_mut(Path::new(rel)).ok_or_else(changed)?;
        let observed = dir_identity(m);
        if stamp.as_ref().is_some_and(|expected| *expected != observed) {
            return Err(changed());
        }
        *stamp = Some(observed);
        Ok(())
    }

    pub async fn validate(
        &self,
        lib: &Library,
        root: &Root,
        id: &str,
        found: &[(PathBuf, String, Metadata)],
    ) -> Result<()> {
        if self.files.len() != found.len() {
            return Err(changed());
        }
        let mut seen = HashSet::new();
        for (_, rel, m) in found {
            if !seen.insert(rel) {
                return Err(changed());
            }
            match self.files.get(rel) {
                Some(Proof::LegacySize(bytes)) if *bytes == m.len() => (),
                Some(Proof::Observed(stamp)) if *stamp == identity(m) => (),
                _ => return Err(changed()),
            }
        }
        if self.fresh {
            let mut conn = lib.db.acquire().await?;
            sqlx::query("PRAGMA synchronous=FULL")
                .execute(&mut *conn)
                .await?;
            let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
            for (dir, stamp) in &self.dirs {
                // Deselection can leave some manifest directories empty or absent.
                let Some(stamp) = stamp else {
                    continue;
                };
                let path = dir.to_str().ok_or_else(changed)?;
                let m = root.metadata(path)?;
                if !m.is_dir() || dir_identity(&m) != *stamp {
                    return Err(changed());
                }
                sqlx::query(
                    "INSERT INTO torrent_import_dirs(download_id,path,identity) VALUES(?,?,?)",
                )
                .bind(id)
                .bind(path)
                .bind(stamp)
                .execute(&mut *tx)
                .await?;
            }
            for (_, rel, m) in found {
                sqlx::query("INSERT INTO torrent_import_files(download_id,original_path,current_path,identity) VALUES(?1,?2,?2,?3)")
                    .bind(id).bind(rel).bind(serde_json::to_string(&identity(m))?).execute(&mut *tx).await?;
            }
            tx.commit().await?;
        }
        Ok(())
    }
}

pub(super) async fn tracked(lib: &Library, id: &str) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM torrent_import_files WHERE download_id=?)")
            .bind(id)
            .fetch_one(&lib.db)
            .await?,
    )
}

pub(super) async fn check_member(lib: &Library, id: &str, rel: &str, m: &Metadata) -> Result<()> {
    let stamp: Option<String> = sqlx::query_scalar(
        "SELECT identity FROM torrent_import_files WHERE download_id=? AND current_path=?",
    )
    .bind(id)
    .bind(rel)
    .fetch_optional(&lib.db)
    .await?;
    if !m.is_file() || m.nlink() != 1 || stamp != Some(serde_json::to_string(&identity(m))?) {
        return Err(changed());
    }
    Ok(())
}

pub(super) async fn check_sort_file(
    lib: &Library,
    download: &DownloadRow,
    rel: &str,
) -> Result<()> {
    if download.kind == DownloadKind::Torrent && tracked(lib, &download.id).await? {
        let root = Root::open(lib.layout.root())?;
        check_member(lib, &download.id, rel, &root.metadata(rel)?).await?;
    }
    Ok(())
}

/// Reclaim only observed manifest directories. A new empty subdirectory is also
/// someone else's entry and must survive cleanup after a reviewed item moves.
pub(super) async fn cleanup_dirs(lib: &Library, source: &str, id: &str) -> Result<bool> {
    if !tracked(lib, id).await? {
        return Ok(false);
    }
    let dirs: Vec<(String,String)> = sqlx::query_as(
        "SELECT path,identity FROM torrent_import_dirs WHERE download_id=? ORDER BY length(path) DESC"
    ).bind(id).fetch_all(&lib.db).await?;
    let root = Root::open(lib.layout.root())?;
    for (dir, stamp) in dirs {
        if !Path::new(&dir).starts_with(source) {
            return Err(changed());
        }
        match root.metadata(&dir) {
            Ok(m) if m.is_dir() && dir_identity(&m) == stamp => {
                match root.remove_empty_organized_dir(&dir) {
                    Ok(()) => (),
                    Err(CoreError::Io(e))
                        if matches!(e.raw_os_error(), Some(libc::ENOTEMPTY | libc::EEXIST)) =>
                    {
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
            Ok(_) => return Err(changed()),
            Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
        sqlx::query("DELETE FROM torrent_import_dirs WHERE download_id=? AND path=?")
            .bind(id)
            .bind(dir)
            .execute(&lib.db)
            .await?;
    }
    Ok(true)
}
