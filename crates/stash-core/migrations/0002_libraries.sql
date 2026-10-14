-- Libraries the owner defines (docs/architecture.md sections 3.1, 4, 6 and 7).
--
-- sqlx runs this file inside one transaction on a connection with foreign keys on, so
-- nothing here may rebuild a table that other tables reference: dropping `items` would
-- cascade away item_progress, download_items and trash. Tables and columns are only
-- added and filled.

-- 1. The libraries, the kinds of new file each claims, and who sees a `selected` one.
CREATE TABLE libraries (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    icon TEXT NOT NULL,
    content_type TEXT NOT NULL CHECK (content_type IN ('video', 'photos')),
    structure TEXT CHECK (structure IN ('episodes', 'one_offs')),
    folder TEXT NOT NULL COLLATE NOCASE UNIQUE,
    visibility TEXT NOT NULL CHECK (visibility IN ('everyone', 'selected')),
    position INTEGER NOT NULL,
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK ((content_type = 'video') = (structure IS NOT NULL))
);
CREATE UNIQUE INDEX libraries_name ON libraries(name COLLATE NOCASE);

CREATE TABLE library_claims (
    category TEXT PRIMARY KEY CHECK (category IN ('movie', 'episode', 'home_video', 'photo')),
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE
);
CREATE INDEX library_claims_library ON library_claims(library_id);

CREATE TABLE library_members (
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (library_id, user_id)
);
CREATE INDEX library_members_user ON library_members(user_id);

-- 2. Where each item is. `place` is 'library' exactly when `library_id` is set (the
-- CHECK names the other column, which SQLite allows and tests on every write). No
-- ON DELETE action on `library_id`: a library is deleted only after its items left it.
ALTER TABLE items ADD COLUMN library_id TEXT REFERENCES libraries(id);
ALTER TABLE items ADD COLUMN place TEXT NOT NULL DEFAULT 'folder'
    CHECK (place IN ('library', 'other_files', 'incoming', 'folder', 'trash')
           AND (place = 'library') = (library_id IS NOT NULL));
ALTER TABLE items ADD COLUMN added_by TEXT REFERENCES users(id) ON DELETE SET NULL;

-- 3. The four starting libraries, matching the old fixed folders, seen by everyone.
INSERT INTO libraries (id, name, icon, content_type, structure, folder, visibility, position, created_by, created_at, updated_at)
VALUES
    ('0192a000-0000-7000-8000-000000000001', 'Movies', 'film', 'video', 'one_offs', 'Movies', 'everyone', 0, NULL,
     CAST(strftime('%s', 'now') AS INTEGER), CAST(strftime('%s', 'now') AS INTEGER)),
    ('0192a000-0000-7000-8000-000000000002', 'Shows', 'tv', 'video', 'episodes', 'Shows', 'everyone', 1, NULL,
     CAST(strftime('%s', 'now') AS INTEGER), CAST(strftime('%s', 'now') AS INTEGER)),
    ('0192a000-0000-7000-8000-000000000003', 'Home Videos', 'clapperboard', 'video', 'one_offs', 'Home Videos', 'everyone', 2, NULL,
     CAST(strftime('%s', 'now') AS INTEGER), CAST(strftime('%s', 'now') AS INTEGER)),
    ('0192a000-0000-7000-8000-000000000004', 'Photos', 'image', 'photos', NULL, 'Photos', 'everyone', 3, NULL,
     CAST(strftime('%s', 'now') AS INTEGER), CAST(strftime('%s', 'now') AS INTEGER));

INSERT INTO library_claims (category, library_id) VALUES
    ('movie', '0192a000-0000-7000-8000-000000000001'),
    ('episode', '0192a000-0000-7000-8000-000000000002'),
    ('home_video', '0192a000-0000-7000-8000-000000000003'),
    ('photo', '0192a000-0000-7000-8000-000000000004');

-- 4. Every item by folder, place and library together in one write. The library folder
-- must match the first path component exactly (BINARY, whatever the column's collation).
UPDATE items SET
    library_id = CASE WHEN status = 'trashed' THEN NULL ELSE (
        SELECT l.id FROM libraries l
        WHERE instr(items.rel_path, '/') > 0
          AND l.folder = substr(items.rel_path, 1, instr(items.rel_path, '/') - 1) COLLATE BINARY) END,
    place = CASE
        WHEN status = 'trashed' THEN 'trash'
        WHEN EXISTS (
            SELECT 1 FROM libraries l
            WHERE instr(items.rel_path, '/') > 0
              AND l.folder = substr(items.rel_path, 1, instr(items.rel_path, '/') - 1) COLLATE BINARY) THEN 'library'
        WHEN substr(rel_path, 1, 6) = 'Other/' THEN 'other_files'
        WHEN substr(rel_path, 1, 6) = 'Inbox/' OR substr(rel_path, 1, 20) = 'Downloads/Completed/' THEN 'incoming'
        ELSE 'folder' END;

-- 5. Who brought each file in: the download it was sorted from, else a download linked to it.
UPDATE items SET added_by = COALESCE(
    (SELECT d.created_by FROM downloads d WHERE d.id = items.source_download_id),
    (SELECT d.created_by FROM download_items di JOIN downloads d ON d.id = di.download_id
      WHERE di.item_id = items.id AND d.created_by IS NOT NULL
      ORDER BY d.created_at, d.id LIMIT 1));

-- 6. Indexes.
CREATE INDEX items_library_added ON items(library_id, status, added_at DESC);
CREATE INDEX items_library_category ON items(library_id, category, status);
CREATE INDEX items_place ON items(place, status);
CREATE INDEX items_added_by ON items(added_by);

-- 7. From now on `library_id` and `place` follow the path in the same write that
-- records it: any insert, and any change of `rel_path` or `status`, works them out
-- again from the final path and the library rows as they are at that moment.
CREATE TRIGGER items_place_after_insert AFTER INSERT ON items
BEGIN
    UPDATE items SET
        library_id = CASE WHEN status = 'trashed' THEN NULL ELSE (
            SELECT l.id FROM libraries l
            WHERE instr(items.rel_path, '/') > 0
              AND l.folder = substr(items.rel_path, 1, instr(items.rel_path, '/') - 1) COLLATE BINARY) END,
        place = CASE
            WHEN status = 'trashed' THEN 'trash'
            WHEN EXISTS (
                SELECT 1 FROM libraries l
                WHERE instr(items.rel_path, '/') > 0
                  AND l.folder = substr(items.rel_path, 1, instr(items.rel_path, '/') - 1) COLLATE BINARY) THEN 'library'
            WHEN substr(rel_path, 1, 6) = 'Other/' THEN 'other_files'
            WHEN substr(rel_path, 1, 6) = 'Inbox/' OR substr(rel_path, 1, 20) = 'Downloads/Completed/' THEN 'incoming'
            ELSE 'folder' END
    WHERE id = NEW.id;
END;

CREATE TRIGGER items_place_after_update AFTER UPDATE OF rel_path, status ON items
BEGIN
    UPDATE items SET
        library_id = CASE WHEN status = 'trashed' THEN NULL ELSE (
            SELECT l.id FROM libraries l
            WHERE instr(items.rel_path, '/') > 0
              AND l.folder = substr(items.rel_path, 1, instr(items.rel_path, '/') - 1) COLLATE BINARY) END,
        place = CASE
            WHEN status = 'trashed' THEN 'trash'
            WHEN EXISTS (
                SELECT 1 FROM libraries l
                WHERE instr(items.rel_path, '/') > 0
                  AND l.folder = substr(items.rel_path, 1, instr(items.rel_path, '/') - 1) COLLATE BINARY) THEN 'library'
            WHEN substr(rel_path, 1, 6) = 'Other/' THEN 'other_files'
            WHEN substr(rel_path, 1, 6) = 'Inbox/' OR substr(rel_path, 1, 20) = 'Downloads/Completed/' THEN 'incoming'
            ELSE 'folder' END
    WHERE id = NEW.id;
END;
