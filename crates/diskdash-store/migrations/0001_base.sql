-- 0001_base: schema versioning, settings and the job runner.

CREATE TABLE schema_version (
    version     INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL,
    applied_at  TEXT    NOT NULL
);

-- One row per setting; value is JSON.
CREATE TABLE settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE jobs (
    id            INTEGER PRIMARY KEY,
    kind          TEXT    NOT NULL,
    params        TEXT    NOT NULL DEFAULT '{}',     -- JSON
    status        TEXT    NOT NULL
                  CHECK (status IN ('queued','running','succeeded','failed','cancelled','interrupted')),
    phase         TEXT,
    done          INTEGER NOT NULL DEFAULT 0,
    total         INTEGER,
    unit          TEXT,
    eta_seconds   INTEGER,
    error         TEXT,
    created_at    TEXT    NOT NULL,
    started_at    TEXT,
    finished_at   TEXT
);
CREATE INDEX jobs_status ON jobs(status);

CREATE TABLE job_events (
    job_id      INTEGER NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL,
    ts          TEXT    NOT NULL,
    type        TEXT    NOT NULL,
    payload     TEXT    NOT NULL DEFAULT '{}',       -- JSON
    PRIMARY KEY (job_id, seq)
) WITHOUT ROWID;
