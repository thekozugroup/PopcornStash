-- Explicit owner cleanup includes files that are not media items (subtitles, NFOs,
-- artwork), and whole library folders. Keep their recovery separate from item Trash.
CREATE TABLE file_trash (
    id TEXT PRIMARY KEY,
    original_rel_path TEXT NOT NULL,
    trashed_rel_path TEXT NOT NULL UNIQUE,
    is_dir INTEGER NOT NULL,
    size INTEGER NOT NULL,
    file_count INTEGER NOT NULL,
    trashed_at INTEGER NOT NULL,
    trashed_by TEXT REFERENCES users(id) ON DELETE SET NULL
);
CREATE TABLE file_trash_items (
    trash_id TEXT NOT NULL REFERENCES file_trash(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    original_rel_path TEXT NOT NULL,
    original_status TEXT NOT NULL,
    PRIMARY KEY (trash_id, item_id)
);
