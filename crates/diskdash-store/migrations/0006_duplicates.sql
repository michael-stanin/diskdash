-- 0006_duplicates: duplicate detection. Groups span drives and snapshots, so
-- results live in the catalog. The hash cache outlives every run.

CREATE TABLE hash_cache (
    volume_serial INTEGER NOT NULL,
    file_id       INTEGER NOT NULL,
    size          INTEGER NOT NULL,
    mtime         INTEGER NOT NULL,
    algo          TEXT    NOT NULL,
    partial       TEXT,
    full          TEXT,
    last_run      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (volume_serial, file_id, size, mtime, algo)
);

CREATE TABLE dup_runs (
    id               INTEGER PRIMARY KEY,
    status           TEXT NOT NULL CHECK (status IN ('queued', 'running', 'complete', 'failed', 'cancelled')),
    algo             TEXT NOT NULL,
    min_size         INTEGER NOT NULL,
    scope            TEXT NOT NULL,            -- JSON: snapshot ids and roots
    near_enabled     INTEGER NOT NULL DEFAULT 0,
    near_threshold   REAL,
    started_at       TEXT NOT NULL,
    finished_at      TEXT,
    error            TEXT,
    job_id           INTEGER,
    bytes_planned    INTEGER NOT NULL DEFAULT 0,
    bytes_hashed     INTEGER NOT NULL DEFAULT 0,
    files_hashed     INTEGER NOT NULL DEFAULT 0,
    cache_hits       INTEGER NOT NULL DEFAULT 0,
    candidates       INTEGER NOT NULL DEFAULT 0,
    group_count      INTEGER NOT NULL DEFAULT 0,
    wasted           INTEGER NOT NULL DEFAULT 0,
    skips            TEXT NOT NULL DEFAULT '{}', -- JSON: reason -> count
    near_truncated   INTEGER NOT NULL DEFAULT 0,
    atime_roots      TEXT NOT NULL DEFAULT '[]'  -- roots where reading may update last-access times
);

CREATE TABLE dup_groups (
    id           INTEGER PRIMARY KEY,
    run_id       INTEGER NOT NULL REFERENCES dup_runs(id) ON DELETE CASCADE,
    hash         TEXT    NOT NULL,
    size         INTEGER NOT NULL,
    alloc        INTEGER NOT NULL,
    member_count INTEGER NOT NULL,
    wasted       INTEGER NOT NULL
);
CREATE INDEX dup_groups_rank ON dup_groups(run_id, wasted DESC);

CREATE TABLE dup_members (
    id           INTEGER PRIMARY KEY,
    group_id     INTEGER NOT NULL REFERENCES dup_groups(id) ON DELETE CASCADE,
    snapshot_id  INTEGER NOT NULL,
    entry_id     INTEGER NOT NULL,
    path         TEXT    NOT NULL,
    drive        TEXT    NOT NULL,
    volume_id    TEXT,
    mtime        INTEGER NOT NULL,
    category     TEXT
);
CREATE INDEX dup_members_group ON dup_members(group_id);

-- A suggestion only: keep this member, the others are for the user to remove by hand.
CREATE TABLE dup_suggestions (
    group_id    INTEGER PRIMARY KEY REFERENCES dup_groups(id) ON DELETE CASCADE,
    rule        TEXT    NOT NULL,
    keep_member INTEGER NOT NULL REFERENCES dup_members(id) ON DELETE CASCADE,
    created_at  TEXT    NOT NULL
);

CREATE TABLE dup_skips (
    run_id  INTEGER NOT NULL REFERENCES dup_runs(id) ON DELETE CASCADE,
    reason  TEXT    NOT NULL,
    path    TEXT    NOT NULL
);
CREATE INDEX dup_skips_run ON dup_skips(run_id, reason);

CREATE TABLE dup_near (
    run_id          INTEGER NOT NULL REFERENCES dup_runs(id) ON DELETE CASCADE,
    dir_a           TEXT    NOT NULL,
    dir_b           TEXT    NOT NULL,
    similarity      REAL    NOT NULL,
    shared_files    INTEGER NOT NULL,
    shared_bytes    INTEGER NOT NULL,
    differing_bytes INTEGER NOT NULL,
    files_a         INTEGER NOT NULL,
    files_b         INTEGER NOT NULL
);
CREATE INDEX dup_near_run ON dup_near(run_id, similarity DESC);
