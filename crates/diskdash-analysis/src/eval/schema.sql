-- Evaluation results live inside the per-snapshot database, so they disappear
-- together with the snapshot.

CREATE TABLE IF NOT EXISTS rule_defs (
    rule_id      TEXT PRIMARY KEY,
    title        TEXT NOT NULL,
    category     TEXT NOT NULL,
    explanation  TEXT NOT NULL,
    risk         TEXT NOT NULL,
    kind         TEXT NOT NULL,
    instructions TEXT NOT NULL            -- JSON array of strings
);

CREATE TABLE IF NOT EXISTS rule_matches (
    id           INTEGER PRIMARY KEY,
    rule_id      TEXT    NOT NULL,
    entry_id     INTEGER,                 -- NULL for virtual candidates
    virtual_key  TEXT,                    -- recycle-bin | shadow-storage
    path         TEXT    NOT NULL,
    is_dir       INTEGER NOT NULL DEFAULT 0,
    logical      INTEGER,                 -- NULL = size unknown
    allocated    INTEGER,
    file_count   INTEGER,
    modified     INTEGER,                 -- unix seconds (newest file for folders)
    risk         TEXT    NOT NULL,
    kind         TEXT    NOT NULL,
    category     TEXT    NOT NULL,
    counted      INTEGER NOT NULL,        -- 0 when nested in another match (no double counting)
    parent_match INTEGER,
    note         TEXT
);

CREATE INDEX IF NOT EXISTS rule_matches_rank ON rule_matches(counted, risk, allocated DESC);
CREATE INDEX IF NOT EXISTS rule_matches_parent ON rule_matches(parent_match) WHERE parent_match IS NOT NULL;
