-- Browser activity is separate from API traffic (polling must not unlock it).
ALTER TABLE sessions ADD COLUMN last_activity_at INTEGER NOT NULL DEFAULT 0;
UPDATE sessions SET last_activity_at = last_seen_at;
