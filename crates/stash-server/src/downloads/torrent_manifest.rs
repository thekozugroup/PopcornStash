//! Validate the adopted payload before pruning or publishing. Sizes check for
//! changes since verification; the engine's piece map supplies integrity.
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use nix::dir::Dir;
use nix::fcntl::{AtFlags, OFlag, openat};
use nix::sys::stat::{Mode, SFlag, fstat, fstatat};
use stash_core::internal_api::{ReleaseWitness, TorrentFile, TorrentFiles};
use std::fs::{File, Metadata};
use std::os::unix::fs::MetadataExt;

/// Flattening preserves old readers of torrent_files_json, including the public file list.
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct Frozen {
    #[serde(flatten)]
    pub files: TorrentFiles,
    #[serde(default)]
    pub release_witness: Option<ReleaseWitness>,
}

fn check_root(meta: &Metadata, witness: &ReleaseWitness) -> io::Result<()> {
    if meta.dev() != witness.root_dev || meta.ino() != witness.root_ino {
        return Err(io::Error::other("Torrent root changed"));
    }
    Ok(())
}

fn check_file(meta: &Metadata, file: &TorrentFile, witness: &ReleaseWitness) -> io::Result<()> {
    let original = witness
        .files
        .get(&file.index)
        .ok_or_else(|| io::Error::other("Missing file witness"))?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.dev() != original.dev
        || meta.ino() != original.ino
        || file.selected
            && (meta.len() != original.size
                || meta.mtime() != original.mtime_sec || meta.mtime_nsec() != original.mtime_nsec
                // Chown changes ctime. Before adoption, ctime also detects
                // same-inode rewrites whose original mtime was restored.
                || (meta.uid() == original.uid && (meta.ctime() != original.ctime_sec || meta.ctime_nsec() != original.ctime_nsec))
                || (meta.uid() != original.uid && meta.uid() != nix::unistd::geteuid().as_raw())
                || meta.len() != file.size
                || !file.complete
                || file.verified_bytes != Some(file.size))
    {
        return Err(io::Error::other("Torrent file changed since verification"));
    }
    Ok(())
}

fn check_tree(root: &OwnedFd, files: &TorrentFiles, witness: &ReleaseWitness) -> io::Result<()> {
    let mut expected = HashMap::new();
    let mut dirs = HashSet::new();
    let mut indices = HashSet::new();
    for file in &files.files {
        if !indices.insert(file.index) || !witness.files.contains_key(&file.index) {
            return Err(io::Error::other("Ambiguous torrent file identity"));
        }
        if file.path.contains('\\') || file.path.split('/').any(|p| matches!(p, "" | "." | "..")) {
            return Err(io::Error::other("Invalid torrent path"));
        }
        let path = Path::new(&file.path);
        if expected.insert(path, file).is_some() {
            return Err(io::Error::other("Duplicate torrent path"));
        }
        dirs.extend(path.ancestors().skip(1));
    }
    let required = files.files.iter().filter(|f| f.selected).count();
    if required == 0 {
        return Err(io::Error::other("No selected torrent files"));
    }
    let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    let dir = openat(root, ".", flags, Mode::empty())?;
    let found = inspect(dir, Path::new(""), &expected, &dirs, flags, witness)?;
    if found != required {
        return Err(io::Error::other("A selected torrent file is missing"));
    }
    Ok(())
}

fn inspect(
    fd: OwnedFd,
    parent: &Path,
    files: &HashMap<&Path, &TorrentFile>,
    dirs: &HashSet<&Path>,
    flags: OFlag,
    witness: &ReleaseWitness,
) -> io::Result<usize> {
    let dev = fstat(&fd)?.st_dev;
    let mut dir = Dir::from_fd(fd)?;
    let mut found = 0;
    // Dir::iter borrows the directory mutably, so use a separate descriptor for openat.
    let anchor = openat(dir.as_fd(), ".", flags, Mode::empty())?;
    for entry in dir.iter() {
        let entry = entry?;
        let name = entry.file_name();
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        let path = parent.join(OsStr::from_bytes(name.to_bytes()));
        let stat = fstatat(&anchor, name, AtFlags::AT_SYMLINK_NOFOLLOW)?;
        if stat.st_dev != dev {
            return Err(io::Error::other("Unexpected filesystem in torrent folder"));
        }
        match SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT {
            SFlag::S_IFDIR if dirs.contains(path.as_path()) => {
                let child = openat(&anchor, name, flags, Mode::empty())?;
                let check = fstat(&child)?;
                if check.st_dev != dev || check.st_ino != stat.st_ino {
                    return Err(io::Error::other("Torrent directory changed"));
                }
                found += inspect(child, &path, files, dirs, flags, witness)?;
            }
            SFlag::S_IFREG if stat.st_nlink == 1 => {
                let file = files
                    .get(path.as_path())
                    .ok_or_else(|| io::Error::other("Unexpected torrent file"))?;
                let fd = openat(
                    &anchor,
                    name,
                    OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK,
                    Mode::empty(),
                )?;
                check_file(&File::from(fd).metadata()?, file, witness)?;
                if file.selected {
                    found += 1;
                }
            }
            _ => return Err(io::Error::other("Unexpected entry in torrent folder")),
        }
    }
    Ok(found)
}

pub(super) fn inspect_payload(
    stash: &Path,
    name: &Path,
    files: &TorrentFiles,
    witness: &ReleaseWitness,
    prune: bool,
) -> std::io::Result<std::fs::Metadata> {
    use nix::errno::Errno;
    use nix::fcntl::{AtFlags, OFlag, open, openat};
    use nix::sys::stat::{Mode, SFlag, fstatat};
    use nix::unistd::{UnlinkatFlags, unlinkat};

    if name.components().count() != 1
        || !name
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return Err(io::Error::other("Invalid torrent folder"));
    }
    let nofollow = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK;
    let flags = nofollow | OFlag::O_DIRECTORY;
    let root = open(stash, flags, Mode::empty())?;
    let downloads = openat(&root, "Downloads", flags, Mode::empty())?;
    let in_progress = openat(&downloads, "In Progress", flags, Mode::empty())?;
    let torrent = openat(&in_progress, name, nofollow, Mode::empty())?;
    let meta = File::from(torrent.try_clone()?).metadata()?;
    check_root(&meta, witness)?;
    if meta.is_file() {
        if let [file] = files.files.as_slice()
            && file.selected
            && Path::new(&file.path) == name
        {
            check_file(&meta, file, witness)?;
            return Ok(meta);
        }
        return Err(std::io::Error::other(
            "Single-file torrent manifest mismatch",
        ));
    }
    check_tree(&torrent, files, witness)?;
    if !prune {
        return Ok(meta);
    }
    'files: for file in files.files.iter().filter(|f| !f.selected) {
        let relative = Path::new(&file.path);
        let parts = relative.components().collect::<Vec<_>>();
        // Retain directory handles throughout traversal and unlinking. A parent
        // swapped for a symlink after validation cannot redirect a deletion.
        let mut directories = vec![openat(&torrent, ".", flags, Mode::empty())?];
        for part in &parts[..parts.len() - 1] {
            match openat(
                directories.last().unwrap(),
                part.as_os_str(),
                flags,
                Mode::empty(),
            ) {
                Ok(dir) => directories.push(dir),
                Err(Errno::ENOENT) => continue 'files,
                Err(error) => return Err(error.into()),
            }
        }
        let parent = directories.last().unwrap();
        let filename = parts.last().unwrap().as_os_str();
        match fstatat(parent, filename, AtFlags::AT_SYMLINK_NOFOLLOW) {
            Ok(meta)
                if SFlag::from_bits_truncate(meta.st_mode) & SFlag::S_IFMT == SFlag::S_IFREG =>
            {
                let fd = openat(parent, filename, nofollow, Mode::empty())?;
                check_file(&File::from(fd).metadata()?, file, witness)?;
                unlinkat(parent, filename, UnlinkatFlags::NoRemoveDir)?;
            }
            Ok(_) => {
                return Err(std::io::Error::other(
                    "Unselected path is not a regular file",
                ));
            }
            Err(Errno::ENOENT) => {}
            Err(error) => return Err(error.into()),
        }
        // Empty directories only, never the torrent root or its siblings.
        for index in (0..parts.len() - 1).rev() {
            if unlinkat(
                &directories[index],
                parts[index].as_os_str(),
                UnlinkatFlags::RemoveDir,
            )
            .is_err()
            {
                break;
            }
        }
    }
    File::from(torrent).metadata()
}

#[cfg(test)]
pub(in crate::downloads) fn fixture_witness(
    root: &Path,
    manifest: &TorrentFiles,
) -> ReleaseWitness {
    let meta = root.symlink_metadata().unwrap();
    ReleaseWitness {
        root_dev: meta.dev(),
        root_ino: meta.ino(),
        files: manifest
            .files
            .iter()
            .map(|f| {
                let m = if meta.is_dir() {
                    root.join(&f.path).symlink_metadata().unwrap()
                } else {
                    meta.clone()
                };
                (f.index, stash_core::internal_api::FileWitness::from(&m))
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stash_core::internal_api::TorrentFileKind;
    use std::fs;

    #[test]
    fn durable_witness_rejects_replacements_and_foreign_entries() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("nested")).unwrap();
        let file = source.join("nested/selected");
        fs::write(&file, b"original").unwrap();
        let manifest = TorrentFiles {
            ready: true,
            files: vec![TorrentFile {
                index: 12,
                path: "nested/selected".into(),
                size: 8,
                kind: TorrentFileKind::Other,
                selected: true,
                complete: true,
                verified_bytes: Some(8),
            }],
            ..Default::default()
        };
        let durable = serde_json::to_string(&Frozen {
            release_witness: Some(fixture_witness(&source, &manifest)),
            files: manifest.clone(),
        })
        .unwrap();
        assert_eq!(
            serde_json::from_str::<TorrentFiles>(&durable).unwrap(),
            manifest
        );
        let restored: Frozen = serde_json::from_str(&durable).unwrap();
        let witness = restored.release_witness.unwrap();
        let verify = || -> io::Result<()> {
            let dir = File::open(&source)?;
            check_root(&dir.metadata()?, &witness)?;
            check_tree(&dir.into(), &restored.files, &witness)
        };
        verify().unwrap();
        fs::create_dir(source.join("unrelated")).unwrap();
        assert!(verify().is_err());
        fs::remove_dir(source.join("unrelated")).unwrap();
        let foreign = source.join("nested/extra");
        fs::write(&foreign, b"keep").unwrap();
        assert!(verify().is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        fs::remove_file(foreign).unwrap();

        let original = File::open(&file).unwrap();
        fs::rename(&file, temp.path().join("aside")).unwrap();
        fs::write(&file, b"replaced").unwrap();
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(original.metadata().unwrap().modified().unwrap()),
            )
            .unwrap();
        assert!(verify().is_err());
        assert_eq!(fs::read(&file).unwrap(), b"replaced");

        fs::rename(&source, temp.path().join("old-root")).unwrap();
        fs::create_dir(&source).unwrap();
        assert!(check_root(&source.metadata().unwrap(), &witness).is_err());
    }

    #[test]
    fn interrupted_pruning_rechecks_both_kept_and_omitted_files() {
        let tmp = tempfile::tempdir().unwrap();
        let leaf = Path::new("fixture");
        let source = tmp.path().join("Downloads/In Progress").join(leaf);
        fs::create_dir_all(&source).unwrap();
        let keep = source.join("keep");
        let skipped = source.join("skip");
        fs::write(&keep, b"keep").unwrap();
        fs::write(&skipped, b"skip").unwrap();
        let files = TorrentFiles {
            ready: true,
            files: ["keep", "skip"]
                .into_iter()
                .enumerate()
                .map(|(index, path)| TorrentFile {
                    index,
                    path: path.into(),
                    size: 4,
                    kind: TorrentFileKind::Other,
                    selected: index == 0,
                    complete: index == 0,
                    verified_bytes: Some(if index == 0 { 4 } else { 1 }),
                })
                .collect(),
            ..Default::default()
        };
        let proof = fixture_witness(&source, &files);
        let run = |prune| inspect_payload(tmp.path(), leaf, &files, &proof, prune);
        run(false).unwrap();
        // The selected file still matches, but a foreign file now occupies the skipped name.
        let original = tmp.path().join("held");
        fs::rename(&skipped, &original).unwrap();
        fs::write(&skipped, b"same").unwrap();
        assert!(run(true).is_err());
        assert_eq!(fs::read(&skipped).unwrap(), b"same");
        fs::remove_file(&skipped).unwrap();
        fs::rename(original, &skipped).unwrap();
        run(true).unwrap();
        run(true).unwrap();
        assert!(!skipped.exists());
        assert_eq!(fs::read(keep).unwrap(), b"keep");
    }

    #[test]
    fn a_manifest_without_release_evidence_is_never_a_witness() {
        let old: Frozen = serde_json::from_str(r#"{"ready":true,"files":[]}"#).unwrap();
        assert!(old.release_witness.is_none());
        assert!(
            serde_json::from_str::<Frozen>(r#"{"ready":true,"files":[],"release_witness":{}}"#)
                .is_err()
        );
    }
}
