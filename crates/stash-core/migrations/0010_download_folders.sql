-- Keep only the original display name, never a torrent URL or credentials.
-- Download history may be cleared independently of its imported media.
CREATE TABLE download_origins (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL
);
INSERT INTO download_origins SELECT id, name FROM downloads;
CREATE TRIGGER download_origin_insert AFTER INSERT ON downloads BEGIN
    INSERT INTO download_origins(id, name) VALUES(NEW.id, NEW.name)
    ON CONFLICT(id) DO UPDATE SET name = excluded.name;
END;
CREATE TRIGGER download_origin_name AFTER UPDATE OF name ON downloads BEGIN
    INSERT INTO download_origins(id, name) VALUES(NEW.id, NEW.name)
    ON CONFLICT(id) DO UPDATE SET name = excluded.name;
END;

-- These are presentation folders; an owner rename/move never renames disk files.
CREATE TABLE library_download_folders (
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    group_key TEXT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY(library_id, group_key)
);
CREATE TABLE library_download_members (
    item_id TEXT PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    group_key TEXT NOT NULL,
    FOREIGN KEY(library_id, group_key) REFERENCES library_download_folders(library_id, group_key) ON DELETE CASCADE
);
