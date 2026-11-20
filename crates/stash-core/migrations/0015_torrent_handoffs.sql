-- The rename and SQLite commit cannot be atomic together. Reserve both the final
-- name and the identity of the released, adopted root before moving it.
CREATE TABLE torrent_handoffs (
    download_id TEXT PRIMARY KEY NOT NULL REFERENCES download_origins(id) ON DELETE RESTRICT,
    source TEXT NOT NULL,
    destination TEXT NOT NULL COLLATE NOCASE,
    root_identity TEXT NOT NULL,
    root_stamp TEXT NOT NULL,
    -- A plain rename is allowed only into this exclusively claimed 0700 wrapper.
    private_key TEXT,
    wrapper_identity TEXT,
    payload_leaf TEXT,
    published INTEGER NOT NULL DEFAULT 0,
    import_started INTEGER NOT NULL DEFAULT 0,
    -- active reserves the public name. retiring is a durable instruction to
    -- remove only the empty owned wrapper, whose private cleanup leaf is its key.
    -- staged means the wrapper is durably in the private directory; the
    -- public name is free. retired keeps only the stale-job tombstone.
    retirement TEXT NOT NULL DEFAULT 'active' CHECK (retirement IN ('active','retiring','staged','retired')),
    CHECK (wrapper_identity IS NULL OR private_key IS NOT NULL)
);
CREATE UNIQUE INDEX torrent_handoffs_live_destination
    ON torrent_handoffs(destination) WHERE retirement IN ('active','retiring');
