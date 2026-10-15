-- A person's optional destination. Deleting a library restores automatic sorting.
ALTER TABLE downloads ADD COLUMN library_id TEXT REFERENCES libraries(id) ON DELETE SET NULL;
ALTER TABLE uploads ADD COLUMN library_id TEXT REFERENCES libraries(id) ON DELETE SET NULL;
