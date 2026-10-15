-- 0007_plans: cleanup plans (checklists the user carries out by hand) and the
-- results of verifying them against a later manual rescan. Items copy what they
-- need from the scan, so a plan stays readable after its snapshot is gone.

CREATE TABLE plans (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE plan_items (
    id                   INTEGER PRIMARY KEY,
    plan_id              INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    snapshot_id          INTEGER,                 -- baseline scan (not a foreign key: snapshots may be deleted)
    entry_id             INTEGER,
    path                 TEXT    NOT NULL,
    path_key             TEXT    NOT NULL,        -- lower-case path, unique per plan
    is_dir               INTEGER NOT NULL DEFAULT 0,
    kind                 TEXT    NOT NULL,        -- manual-cleanup | system-setting | duplicate | stale | manual-move
    source               TEXT    NOT NULL,        -- cleanup-rules | duplicates | stale | tiering
    source_ref           TEXT,
    title                TEXT    NOT NULL,
    category             TEXT,
    risk                 TEXT    NOT NULL,
    baseline_logical     INTEGER,                 -- NULL = size unknown
    baseline_allocated   INTEGER,
    baseline_mtime       INTEGER,
    baseline_file_id     TEXT,
    destination          TEXT,                    -- manual-move: where the user will put it
    instructions         TEXT    NOT NULL DEFAULT '[]',   -- JSON array of text lines
    status               TEXT    NOT NULL DEFAULT 'todo' CHECK (status IN ('todo', 'done', 'skipped')),
    note                 TEXT,
    added_at             TEXT    NOT NULL,
    verify_status        TEXT    NOT NULL DEFAULT 'unverified',
    verify_freed         INTEGER,
    verified_at          TEXT,
    verified_snapshot_id INTEGER,
    UNIQUE (plan_id, path_key)
);
CREATE INDEX plan_items_plan ON plan_items(plan_id, status);

CREATE TABLE plan_verifications (
    id                   INTEGER PRIMARY KEY,
    plan_id              INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    root                 TEXT    NOT NULL,
    snapshot_id          INTEGER NOT NULL,
    baseline_snapshot_id INTEGER,
    verified_at          TEXT    NOT NULL,
    items                INTEGER NOT NULL,
    freed                INTEGER NOT NULL,         -- bytes freed by the planned items on this root
    scanned_change       INTEGER,                  -- change in scanned size between the two scans (includes unrelated activity)
    UNIQUE (plan_id, root)
);
