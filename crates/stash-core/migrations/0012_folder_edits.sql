-- Durable commit markers for explicitly previewed physical folder edits.
-- A marker is removed only after the matching file journal is reconciled.
CREATE TABLE library_folder_edits (
    id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE RESTRICT
);
