-- 0002_inventory: known volumes and disks, free-space samples, last inventory.

CREATE TABLE volumes (
    volume_id    TEXT PRIMARY KEY,          -- volume GUID (lowercase)
    volume_path  TEXT NOT NULL,             -- \\?\Volume{…}\
    letters      TEXT NOT NULL DEFAULT '',  -- e.g. "C" or "C,X"
    label        TEXT NOT NULL DEFAULT '',
    filesystem   TEXT NOT NULL DEFAULT '',
    serial       INTEGER,                   -- volume serial (detects reformat)
    capacity     INTEGER NOT NULL,
    media        TEXT NOT NULL,
    first_seen   TEXT NOT NULL,
    last_seen    TEXT NOT NULL
);

CREATE TABLE physical_disks (
    disk_key     TEXT PRIMARY KEY,          -- serial when known, else "disk-<n>"
    number       INTEGER NOT NULL,
    model        TEXT NOT NULL,
    bus_type     TEXT NOT NULL,
    media        TEXT NOT NULL,
    size         INTEGER NOT NULL,
    first_seen   TEXT NOT NULL,
    last_seen    TEXT NOT NULL
);

CREATE TABLE volume_disks (
    volume_id    TEXT NOT NULL REFERENCES volumes(volume_id) ON DELETE CASCADE,
    disk_key     TEXT NOT NULL REFERENCES physical_disks(disk_key) ON DELETE CASCADE,
    PRIMARY KEY (volume_id, disk_key)
) WITHOUT ROWID;

-- At most one sample per volume per 15-minute bucket (latest wins).
CREATE TABLE volume_samples (
    volume_id    TEXT    NOT NULL,
    bucket       INTEGER NOT NULL,          -- unix seconds / 900
    ts           TEXT    NOT NULL,
    capacity     INTEGER NOT NULL,
    free         INTEGER NOT NULL,
    PRIMARY KEY (volume_id, bucket)
) WITHOUT ROWID;

-- Last successful inventory, shown immediately after a restart.
CREATE TABLE inventory_cache (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    json         TEXT NOT NULL,
    refreshed_at TEXT NOT NULL
);
