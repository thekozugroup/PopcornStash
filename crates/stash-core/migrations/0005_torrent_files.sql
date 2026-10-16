-- Keep the completion manifest before the engine forgets the torrent.
ALTER TABLE downloads ADD COLUMN torrent_files_json TEXT;
