-- 0004_cleanup: state and cached totals of cleanup-rule evaluation. The matches
-- themselves live inside each snapshot's own database.

CREATE TABLE cleanup_runs (
    snapshot_id        INTEGER PRIMARY KEY REFERENCES snapshots(id) ON DELETE CASCADE,
    status             TEXT    NOT NULL CHECK (status IN ('queued', 'running', 'complete', 'failed')),
    rules_fingerprint  TEXT,
    started_at         TEXT,
    finished_at        TEXT,
    error              TEXT,
    job_id             INTEGER,
    match_count        INTEGER NOT NULL DEFAULT 0,
    rule_errors        INTEGER NOT NULL DEFAULT 0
);

-- Potential savings per snapshot, risk and kind (allocated bytes, each byte once).
CREATE TABLE cleanup_savings (
    snapshot_id  INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    risk         TEXT    NOT NULL,
    kind         TEXT    NOT NULL,
    allocated    INTEGER NOT NULL,
    logical      INTEGER NOT NULL,
    matches      INTEGER NOT NULL,
    unknown      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (snapshot_id, risk, kind)
);
