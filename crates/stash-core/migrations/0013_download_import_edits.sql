-- Commit markers for recoverable, completed-download file organization.
CREATE TABLE download_import_edits (
    id TEXT PRIMARY KEY,
    download_id TEXT NOT NULL REFERENCES download_origins(id) ON DELETE RESTRICT
);
