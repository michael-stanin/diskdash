-- 0005_time_basis: which timestamp the stale-data analysis trusts for a scan
-- (last-access when NTFS keeps it up to date, otherwise last-modified).
ALTER TABLE snapshots ADD COLUMN time_basis TEXT NOT NULL DEFAULT 'mtime';
ALTER TABLE snapshots ADD COLUMN time_basis_reason TEXT NOT NULL DEFAULT 'unknown';
