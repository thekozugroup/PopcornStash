-- PopcornStash schema v1. See docs/architecture.md section 6.

CREATE TABLE users (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner', 'member')),
    webauthn_id TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);

CREATE TABLE passkeys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    credential_id TEXT NOT NULL UNIQUE,
    passkey_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER
);
CREATE INDEX passkeys_user ON passkeys(user_id);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('cookie', 'bearer')),
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    user_agent TEXT,
    ip TEXT
);
CREATE INDEX sessions_user ON sessions(user_id);

CREATE TABLE api_tokens (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    prefix TEXT NOT NULL,
    scopes TEXT NOT NULL,
    client_kind TEXT NOT NULL DEFAULT 'other',
    created_at INTEGER NOT NULL,
    last_used_at INTEGER,
    expires_at INTEGER
);
CREATE INDEX api_tokens_user ON api_tokens(user_id);

CREATE TABLE invites (
    id TEXT PRIMARY KEY,
    code_hash TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'member')),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at INTEGER,
    used_by TEXT REFERENCES users(id) ON DELETE SET NULL
);

CREATE TABLE recovery_codes (
    id TEXT PRIMARY KEY,
    code_hash TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('setup', 'owner_reset')),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at INTEGER
);

CREATE TABLE pairing_requests (
    id TEXT PRIMARY KEY,
    user_code TEXT NOT NULL UNIQUE,
    poll_secret_hash TEXT NOT NULL,
    client_name TEXT NOT NULL,
    client_kind TEXT NOT NULL,
    ip TEXT,
    status TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'denied', 'delivered')),
    token_id TEXT REFERENCES api_tokens(id) ON DELETE SET NULL,
    pending_token TEXT,
    approved_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at INTEGER NOT NULL,
    user_id TEXT,
    event TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '{}',
    ip TEXT
);
CREATE INDEX audit_log_at ON audit_log(at);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE items (
    id TEXT PRIMARY KEY,
    rel_path TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('video', 'photo', 'audio', 'other')),
    category TEXT NOT NULL CHECK (category IN ('movie', 'episode', 'home_video', 'photo', 'other')),
    status TEXT NOT NULL CHECK (status IN ('ready', 'needs_review', 'missing', 'trashed')),
    title TEXT NOT NULL,
    sort_title TEXT NOT NULL,
    year INTEGER,
    show_name TEXT,
    season INTEGER,
    episode INTEGER,
    episode_title TEXT,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    quick_hash TEXT,
    mime TEXT,
    duration_s REAL,
    width INTEGER,
    height INTEGER,
    media_json TEXT,
    taken_at INTEGER,
    camera TEXT,
    thumb_state TEXT NOT NULL DEFAULT 'pending' CHECK (thumb_state IN ('pending', 'ready', 'failed', 'none')),
    suggestion_json TEXT,
    favorite INTEGER NOT NULL DEFAULT 0,
    source_download_id TEXT,
    missing_since INTEGER,
    added_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX items_category_added ON items(category, added_at DESC);
CREATE INDEX items_category_taken ON items(category, taken_at DESC);
CREATE INDEX items_show ON items(show_name, season, episode);
CREATE INDEX items_status ON items(status);
CREATE INDEX items_hash ON items(size, quick_hash);

CREATE TABLE item_progress (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    position_s REAL NOT NULL,
    duration_s REAL NOT NULL,
    finished INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, item_id)
);
CREATE INDEX item_progress_recent ON item_progress(user_id, updated_at DESC);

CREATE TABLE downloads (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('torrent', 'http', 'upload')),
    name TEXT NOT NULL,
    source TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('queued', 'downloading', 'paused', 'seeding', 'completed', 'sorting', 'sorted', 'failed', 'cancelled')),
    progress REAL NOT NULL DEFAULT 0,
    total_bytes INTEGER,
    done_bytes INTEGER NOT NULL DEFAULT 0,
    down_bps INTEGER NOT NULL DEFAULT 0,
    up_bps INTEGER NOT NULL DEFAULT 0,
    peers INTEGER,
    eta_s INTEGER,
    error TEXT,
    rel_path TEXT,
    info_hash TEXT,
    engine_id TEXT,
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    sorted_at INTEGER
);
CREATE INDEX downloads_state ON downloads(state, created_at DESC);

CREATE TABLE download_items (
    download_id TEXT NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    PRIMARY KEY (download_id, item_id)
);

CREATE TABLE uploads (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    filename TEXT NOT NULL,
    size INTEGER NOT NULL,
    received INTEGER NOT NULL DEFAULT 0,
    mime TEXT,
    last_modified INTEGER,
    state TEXT NOT NULL CHECK (state IN ('receiving', 'complete', 'cancelled')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    item_id TEXT,
    payload TEXT NOT NULL DEFAULT '{}',
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'done', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    run_after INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX jobs_pending ON jobs(state, kind, run_after);

CREATE TABLE trash (
    item_id TEXT PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
    original_rel_path TEXT NOT NULL,
    trashed_rel_path TEXT NOT NULL,
    trashed_at INTEGER NOT NULL,
    trashed_by TEXT REFERENCES users(id) ON DELETE SET NULL
);
