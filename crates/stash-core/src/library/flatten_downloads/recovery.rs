use super::*;
use std::io::Read;

#[derive(Debug, Serialize, Deserialize)]
pub(in crate::library) struct Journal {
    item_id: String,
    library_id: String,
    key: String,
    pub(super) files: Vec<JournalFile>,
}
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct JournalFile {
    pub choice: FlattenFile,
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
impl JournalFile {
    pub fn check(&self, root: &Root, path: &str) -> Result<()> {
        if identity(&root.metadata(path)?) != self.identity {
            return Err(CoreError::conflict(
                "A file changed during organization. Recovery kept every copy.",
            ));
        }
        Ok(())
    }
}
impl Journal {
    pub fn new(
        item_id: &str,
        library_id: &str,
        key: &str,
        files: &[&FlattenFile],
        root: &Root,
    ) -> Result<Self> {
        let files = files
            .iter()
            .map(|f| {
                Ok(JournalFile {
                    choice: (*f).clone(),
                    identity: identity(&root.metadata(&f.from)?),
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            item_id: item_id.into(),
            library_id: library_id.into(),
            key: key.into(),
            files,
        })
    }
}
pub(in crate::library) fn sync_parent(root: &Root, path: &str) -> Result<()> {
    let parent = Path::new(path)
        .parent()
        .and_then(Path::to_str)
        .ok_or(CoreError::NotFound)?;
    root.directory(parent)?.sync_all()?;
    Ok(())
}
fn exists(root: &Root, path: &str) -> Result<bool> {
    match root.metadata(path) {
        Ok(_) => Ok(true),
        Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

pub(super) async fn recover_one(
    lib: &Library,
    root: &Root,
    stage: &str,
    journal: &Journal,
) -> Result<()> {
    let def = defs::load_one(&lib.db, &journal.library_id)
        .await?
        .ok_or(CoreError::NotFound)?;
    let video = journal
        .files
        .iter()
        .find(|f| matches!(f.choice.kind.as_str(), "video" | "item"))
        .ok_or(CoreError::NotFound)?;
    if journal.files.is_empty() || journal.files.len() > 2000 {
        return Err(CoreError::invalid("Invalid flatten recovery journal."));
    }
    for f in &journal.files {
        for path in [&f.choice.from, &f.choice.to] {
            if scanner::normalize_rel(path)? != *path
                || !path.starts_with(&format!("{}/", def.folder()))
            {
                return Err(CoreError::invalid(
                    "Flatten recovery stays inside its original library.",
                ));
            }
        }
    }
    let item = repo::get(&lib.db, &journal.item_id)
        .await?
        .ok_or(CoreError::NotFound)?;
    if item.library_id.as_deref() != Some(&journal.library_id)
        || ![&video.choice.from, &video.choice.to].contains(&&item.rel_path)
    {
        return Err(CoreError::conflict(
            "This library entry changed. Organization recovery kept every copy.",
        ));
    }
    let committed = item.rel_path == video.choice.to;
    for (index, f) in journal.files.iter().enumerate() {
        let private = format!("{stage}/file-{index}");
        let from = &f.choice.from;
        let to = &f.choice.to;
        if committed || from == to {
            f.check(root, to)?;
            if exists(root, &private)? {
                f.check(root, &private)?;
                root.remove_flatten_leaf(&private)?;
            }
        } else {
            if exists(root, from)? {
                f.check(root, from)?;
            } else if exists(root, &private)? {
                f.check(root, &private)?;
                root.rename_trash(&private, from)?;
                sync_parent(root, from)?;
            } else {
                f.check(root, to)?;
                root.rename_trash(to, &private)?;
                sync_parent(root, to)?;
                f.check(root, &private)?;
                root.rename_trash(&private, from)?;
                sync_parent(root, from)?;
            }
            // A crash between linkat and unlinkat can leave a duplicate reference.
            // Claim it privately before removing anything; verify the restored source again.
            if exists(root, to)? {
                f.check(root, to)?;
                if exists(root, &private)? {
                    f.check(root, &private)?;
                    root.remove_flatten_leaf(&private)?;
                }
                root.rename_trash(to, &private)?;
                sync_parent(root, to)?;
            }
            f.check(root, from)?;
            if exists(root, &private)? {
                f.check(root, &private)?;
                root.remove_flatten_leaf(&private)?;
            }
        }
        sync_parent(root, &private)?;
    }
    root.remove_flatten_leaf(&format!("{stage}/preparing.json"))?;
    root.remove_flatten_leaf(&format!("{stage}/journal.json"))?;
    root.remove_flatten_dir(stage)?;
    Ok(())
}

impl Library {
    pub async fn recover_flattened_files(&self) -> Result<()> {
        let _moves = self.moves.lock().await;
        self.recover_flattened_files_locked().await
    }

    pub(in crate::library) async fn recover_flattened_files_locked(&self) -> Result<()> {
        self.flatten_pending
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let root = Root::open(self.layout.root())?;
        let fd = root.directory(".popcornstash/Trash")?;
        let mut entries = nix::dir::Dir::from_fd(fd.into()).map_err(std::io::Error::from)?;
        let mut stages = Vec::new();
        for entry in entries.iter() {
            let entry = entry.map_err(std::io::Error::from)?;
            if let Ok(name) = entry.file_name().to_str()
                && name.starts_with("flatten-")
            {
                stages.push(format!(".popcornstash/Trash/{name}"));
            }
            if stages.len() > 500 {
                return Err(CoreError::invalid(
                    "Too many interrupted organization journals. Files were kept.",
                ));
            }
        }
        for stage in stages {
            let path = format!("{stage}/journal.json");
            let file = match root.file(&path) {
                Ok(f) => f,
                Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    root.remove_flatten_leaf(&format!("{stage}/preparing.json"))?;
                    root.remove_flatten_dir(&stage)?;
                    continue;
                }
                Err(e) => return Err(e),
            };
            let mut bytes = Vec::new();
            file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(CoreError::invalid(
                    "Invalid organization journal size. Files were kept.",
                ));
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
                CoreError::invalid("Organization journal could not be read. Files were kept.")
            })?;
            if value.get("tree_version").is_some() {
                let journal: super::super::rename_folder::TreeJournal =
                    serde_json::from_value(value).map_err(|_| {
                        CoreError::invalid(
                            "Folder rename journal could not be read. Files were kept.",
                        )
                    })?;
                super::super::rename_folder::recover(self, &root, &stage, &journal).await?;
                continue;
            }
            let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| {
                CoreError::invalid(
                    "Organization recovery could not read its journal. Files were kept.",
                )
            })?;
            recover_one(self, &root, &stage, &journal).await?;
        }
        sqlx::query("DELETE FROM library_folder_edits")
            .execute(&self.db)
            .await?;
        self.flatten_pending
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}
