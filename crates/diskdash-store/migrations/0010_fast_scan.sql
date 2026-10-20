-- 0010_fast_scan: why a scan used the directory walk instead of the MFT, and where the
-- USN change journal stood after each volume's last MFT scan (for manual quick rescans).

ALTER TABLE snapshots ADD COLUMN scan_note TEXT;

CREATE TABLE volume_usn_state (
    volume_guid TEXT    PRIMARY KEY,
    journal_id  INTEGER NOT NULL,
    next_usn    INTEGER NOT NULL,
    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    updated_at  TEXT    NOT NULL
);
