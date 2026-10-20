-- 0009_history: where each free-space sample came from, snapshot retention state,
-- and the directory-level summaries kept after a snapshot file is removed.

ALTER TABLE volume_samples ADD COLUMN source TEXT NOT NULL DEFAULT 'inventory';

-- full = the snapshot file exists; summary = reduced to directory totals up to summary_depth.
ALTER TABLE snapshots ADD COLUMN detail_level TEXT NOT NULL DEFAULT 'full' CHECK (detail_level IN ('full', 'summary'));
ALTER TABLE snapshots ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
ALTER TABLE snapshots ADD COLUMN summary_depth INTEGER;

CREATE TABLE snapshot_summaries (
    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    path_key    TEXT    NOT NULL,              -- lower-case full path
    path        TEXT    NOT NULL,
    depth       INTEGER NOT NULL,
    logical     INTEGER NOT NULL,
    allocated   INTEGER NOT NULL,
    file_count  INTEGER NOT NULL,
    PRIMARY KEY (snapshot_id, path_key)
) WITHOUT ROWID;

CREATE TABLE snapshot_summary_categories (
    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    category    INTEGER NOT NULL,
    file_count  INTEGER NOT NULL,
    size        INTEGER NOT NULL,
    alloc       INTEGER NOT NULL,
    PRIMARY KEY (snapshot_id, category)
) WITHOUT ROWID;
