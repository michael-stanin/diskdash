-- 0011_alerts: in-app alerts (low space, fast growth). State keeps one row per alert key for
-- de-duplication and cooldown; alerts keeps the history shown on the Alerts page.

CREATE TABLE alert_state (
    alert_key      TEXT    PRIMARY KEY,           -- low_space:<volume id> or growth:<lower-case folder path>
    last_raised_at INTEGER NOT NULL,              -- unix seconds
    last_value     REAL,
    active         INTEGER NOT NULL DEFAULT 0,
    dismissed_at   INTEGER
);

CREATE TABLE alerts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    alert_key   TEXT    NOT NULL,
    kind        TEXT    NOT NULL CHECK (kind IN ('low_space', 'growth')),
    severity    TEXT    NOT NULL CHECK (severity IN ('warning', 'critical')),
    volume_id   TEXT,
    drive       TEXT,                             -- letter such as C:
    path        TEXT,                             -- growth: the folder
    title       TEXT    NOT NULL,
    message     TEXT    NOT NULL,
    value       REAL    NOT NULL,                 -- low space: free bytes; growth: bytes per day
    threshold   REAL    NOT NULL,
    detail      TEXT,                             -- JSON: snapshot ids for the history link
    raised_at   INTEGER NOT NULL,
    resolved_at INTEGER,
    dismissed_at INTEGER
);

CREATE INDEX alerts_by_time ON alerts(raised_at DESC);
CREATE INDEX alerts_open ON alerts(resolved_at) WHERE resolved_at IS NULL;
