-- Observed local filesystem identities. NULL means the item predates observation.
ALTER TABLE items ADD COLUMN file_dev INTEGER;
ALTER TABLE items ADD COLUMN file_ino INTEGER;
CREATE TABLE library_root_identity (
    library_id TEXT PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,
    parent_dev INTEGER NOT NULL, parent_ino INTEGER NOT NULL,
    dev INTEGER NOT NULL, ino INTEGER NOT NULL
);
-- NULL is a legacy label whose original purpose was not recorded.
ALTER TABLE library_download_folders ADD COLUMN follows_directory INTEGER
    CHECK (follows_directory IN (0,1));

-- A converter or importer can publish replacement bytes before the next scan.
CREATE TRIGGER items_invalidate_file_identity AFTER UPDATE OF size,mtime,quick_hash ON items
WHEN NEW.size<>OLD.size OR NEW.mtime<>OLD.mtime OR NEW.quick_hash IS NOT OLD.quick_hash
BEGIN
    UPDATE items SET file_dev=NULL,file_ino=NULL WHERE id=NEW.id;
END;
