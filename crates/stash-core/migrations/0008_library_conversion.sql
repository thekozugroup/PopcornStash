-- Opt-in, owner-controlled replacement. Journals survive library/item removal so
-- interrupted publication can always restore the original before scanning resumes.
CREATE TABLE library_conversion (
    library_id TEXT PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
    codec TEXT NOT NULL DEFAULT 'av1' CHECK(codec IN ('av1','hevc')),
    quality TEXT NOT NULL DEFAULT 'balanced' CHECK(quality IN ('high','balanced','small')),
    compute TEXT NOT NULL DEFAULT 'auto' CHECK(compute IN ('auto','hardware','cpu')),
    revision INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE conversion_jobs (
    id TEXT PRIMARY KEY,
    item_id TEXT REFERENCES items(id) ON DELETE SET NULL,
    library_id TEXT REFERENCES libraries(id) ON DELETE SET NULL,
    revision INTEGER NOT NULL,
    source_version TEXT NOT NULL,
    source_path TEXT NOT NULL,
    target_path TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('encoding','verifying','publishing','complete','skipped','failed','cancelled')),
    progress REAL NOT NULL DEFAULT 0,
    encoder TEXT,
    message TEXT,
    saved_bytes INTEGER NOT NULL DEFAULT 0,
    stage TEXT,
    source_hash TEXT,
    output_hash TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(item_id, revision, source_version)
);
CREATE INDEX conversion_jobs_library ON conversion_jobs(library_id, created_at DESC);
