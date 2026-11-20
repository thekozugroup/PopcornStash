-- Capture the exact selected files before flattening. Journaled renames advance
-- current_path in the catalog commit; NULL means the file left Completed.
CREATE TABLE torrent_import_files (
    download_id TEXT NOT NULL REFERENCES download_origins(id) ON DELETE RESTRICT,
    original_path TEXT NOT NULL,
    current_path TEXT,
    identity TEXT NOT NULL,
    PRIMARY KEY (download_id, original_path),
    UNIQUE (download_id, current_path)
);

-- A directory can remain empty between the flatten commit and cleanup.
-- Forget its claim after rmdir so a later entry at that name is protected.
CREATE TABLE torrent_import_dirs (
    download_id TEXT NOT NULL REFERENCES download_origins(id) ON DELETE RESTRICT,
    path TEXT NOT NULL,
    identity TEXT NOT NULL,
    PRIMARY KEY (download_id, path)
);
