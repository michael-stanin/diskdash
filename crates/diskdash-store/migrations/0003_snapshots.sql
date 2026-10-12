-- 0003_snapshots: catalog of scan snapshots. Each snapshot's entries live in
-- their own SQLite file (snapshots/<id>.db) so deleting one is instant.

CREATE TABLE snapshots (
    id                   INTEGER PRIMARY KEY,
    root                 TEXT    NOT NULL,
    volume_id            TEXT,                  -- inventory drive id, when known
    volume_serial        INTEGER,
    filesystem           TEXT,
    cluster_size         INTEGER,
    method               TEXT    NOT NULL DEFAULT 'walk',
    elevated             INTEGER NOT NULL DEFAULT 0,
    hardlinks            TEXT    NOT NULL,      -- detected | not_deduplicated
    status               TEXT    NOT NULL CHECK (status IN ('running', 'partial', 'complete', 'failed')),
    started_at           TEXT    NOT NULL,
    finished_at          TEXT,
    resumed              INTEGER NOT NULL DEFAULT 0,
    file_count           INTEGER NOT NULL DEFAULT 0,
    dir_count            INTEGER NOT NULL DEFAULT 0,
    link_count           INTEGER NOT NULL DEFAULT 0,
    total_size           INTEGER NOT NULL DEFAULT 0,
    total_alloc          INTEGER NOT NULL DEFAULT 0,
    error_count          INTEGER NOT NULL DEFAULT 0,
    access_denied_count  INTEGER NOT NULL DEFAULT 0,
    link_unknown_count   INTEGER NOT NULL DEFAULT 0,
    job_id               INTEGER,
    db_file              TEXT    NOT NULL,      -- relative to the data dir
    error                TEXT
);

CREATE INDEX snapshots_root ON snapshots(root, status, finished_at);
CREATE INDEX snapshots_volume ON snapshots(volume_id, status, finished_at);
