use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use crate::migrate::{migrate, MigrationError, MIGRATIONS};
use crate::{DataDir, OwnFiles};

/// Busy timeout for every connection: readers and writers wait instead of
/// failing with `database is locked`.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    OwnFiles(#[from] crate::OwnFilesError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Migration(#[from] MigrationError),
}

/// Opens connections to one database file with the standard pragmas.
#[derive(Debug, Clone)]
pub struct Db {
    path: std::path::PathBuf,
}

impl Db {
    /// `path` must be inside the data dir (checked by [`OwnFiles`]).
    pub fn new(own: &OwnFiles, path: &Path) -> Result<Self, DbError> {
        Ok(Self { path: own.prepare_db_path(path)? })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Open a read/write connection with WAL, foreign keys and busy timeout.
    pub fn connect(&self) -> Result<Connection, rusqlite::Error> {
        let conn = Connection::open(&self.path)?;
        configure(&conn)?;
        Ok(conn)
    }

    /// Open a read-only connection (API request handlers).
    pub fn connect_read_only(&self) -> Result<Connection, rusqlite::Error> {
        let conn = Connection::open_with_flags(
            &self.path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(conn)
    }
}

pub fn configure(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// Open (creating if needed) and migrate the catalog database.
pub fn open_catalog(data_dir: &DataDir, own: &OwnFiles) -> Result<Db, DbError> {
    let db = Db::new(own, &data_dir.catalog_db())?;
    let mut conn = db.connect()?;
    migrate(&mut conn, MIGRATIONS)?;
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_start_creates_wal_database_with_base_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let dd = DataDir::at(tmp.path().join("dd"));
        let own = OwnFiles::new(&dd).unwrap();
        let db = open_catalog(&dd, &own).unwrap();
        assert!(dd.catalog_db().exists());

        let conn = db.connect().unwrap();
        let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
        let fk: i64 = conn.pragma_query_value(None, "foreign_keys", |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);
        let timeout: i64 = conn.pragma_query_value(None, "busy_timeout", |r| r.get(0)).unwrap();
        assert!(timeout >= 5000);

        for table in ["schema_version", "settings", "jobs", "job_events"] {
            let n: i64 = conn
                .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [table], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "missing table {table}");
        }
    }

    #[test]
    fn db_outside_data_dir_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let own = OwnFiles::new(&DataDir::at(tmp.path().join("dd"))).unwrap();
        assert!(Db::new(&own, &tmp.path().join("elsewhere.db")).is_err());
    }

    #[test]
    fn readers_do_not_block_on_writer() {
        let tmp = tempfile::tempdir().unwrap();
        let dd = DataDir::at(tmp.path().join("dd"));
        let own = OwnFiles::new(&dd).unwrap();
        let db = open_catalog(&dd, &own).unwrap();

        let mut writer = db.connect().unwrap();
        let tx = writer.transaction().unwrap();
        tx.execute("INSERT INTO settings(key, value, updated_at) VALUES ('k', '1', '2026-01-01T00:00:00Z')", [])
            .unwrap();
        // Uncommitted write in progress; a reader still gets a consistent view.
        let reader = db.connect_read_only().unwrap();
        let n: i64 = reader.query_row("SELECT count(*) FROM settings", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        tx.commit().unwrap();
        let n: i64 = reader.query_row("SELECT count(*) FROM settings", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }
}
