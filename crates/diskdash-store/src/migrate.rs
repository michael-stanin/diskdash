//! Embedded, numbered SQL migrations: one transaction each, version recorded
//! in `schema_version`, and refusal to run against a newer database.

use rusqlite::{Connection, OptionalExtension};

#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// All migrations shipped with this build, in order.
pub const MIGRATIONS: &[Migration] =
    &[Migration { version: 1, name: "base", sql: include_str!("../migrations/0001_base.sql") }];

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error(
        "the database is at schema version {found}, but this diskdash only knows up to {known}; \
         it was created by a newer version"
    )]
    NewerDatabase { found: u32, known: u32 },
    #[error("migration {version} ({name}) failed: {source}")]
    Failed { version: u32, name: &'static str, source: rusqlite::Error },
    #[error("migrations are not numbered 1..n in order")]
    BadSequence,
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

/// Current schema version (0 for an empty database).
pub fn current_version(conn: &Connection) -> Result<u32, rusqlite::Error> {
    let has_table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_version')",
        [],
        |r| r.get(0),
    )?;
    if !has_table {
        return Ok(0);
    }
    Ok(conn
        .query_row("SELECT max(version) FROM schema_version", [], |r| r.get::<_, Option<u32>>(0))
        .optional()?
        .flatten()
        .unwrap_or(0))
}

/// Apply all pending migrations. Returns the resulting version.
pub fn migrate(conn: &mut Connection, migrations: &[Migration]) -> Result<u32, MigrationError> {
    if migrations.iter().enumerate().any(|(i, m)| m.version as usize != i + 1) {
        return Err(MigrationError::BadSequence);
    }
    let known = migrations.len() as u32;
    let found = current_version(conn)?;
    if found > known {
        return Err(MigrationError::NewerDatabase { found, known });
    }
    for m in migrations.iter().filter(|m| m.version > found) {
        let tx = conn.transaction()?;
        let applied = tx.execute_batch(m.sql).and_then(|()| {
            tx.execute(
                "INSERT INTO schema_version(version, name, applied_at) VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
                rusqlite::params![m.version, m.name],
            )
        });
        match applied {
            Ok(_) => tx.commit()?,
            Err(source) => {
                // Dropping `tx` rolls back.
                return Err(MigrationError::Failed { version: m.version, name: m.name, source });
            }
        }
    }
    Ok(current_version(conn)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const M1: Migration = Migration { version: 1, name: "base", sql: include_str!("../migrations/0001_base.sql") };
    const M2: Migration = Migration { version: 2, name: "two", sql: "CREATE TABLE two(x INTEGER);" };
    const M3: Migration = Migration { version: 3, name: "three", sql: "CREATE TABLE three(x INTEGER);" };
    const M2_BROKEN: Migration =
        Migration { version: 2, name: "broken", sql: "CREATE TABLE half(x INTEGER); THIS IS NOT SQL;" };

    fn mem() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)", [name], |r| r.get(0)).unwrap()
    }

    #[test]
    fn upgrade_applies_pending_in_order() {
        let mut conn = mem();
        assert_eq!(migrate(&mut conn, &[M1]).unwrap(), 1);
        assert_eq!(migrate(&mut conn, &[M1, M2, M3]).unwrap(), 3);
        assert!(table_exists(&conn, "two") && table_exists(&conn, "three"));
        // Idempotent
        assert_eq!(migrate(&mut conn, &[M1, M2, M3]).unwrap(), 3);
    }

    #[test]
    fn failed_migration_rolls_back() {
        let mut conn = mem();
        migrate(&mut conn, &[M1]).unwrap();
        let err = migrate(&mut conn, &[M1, M2_BROKEN]).unwrap_err();
        assert!(matches!(err, MigrationError::Failed { version: 2, .. }));
        assert_eq!(current_version(&conn).unwrap(), 1);
        assert!(!table_exists(&conn, "half"), "partial migration must be rolled back");
    }

    #[test]
    fn newer_database_is_refused() {
        let mut conn = mem();
        migrate(&mut conn, &[M1, M2, M3]).unwrap();
        let err = migrate(&mut conn, &[M1]).unwrap_err();
        assert!(matches!(err, MigrationError::NewerDatabase { found: 3, known: 1 }));
        assert!(err.to_string().contains("newer version"));
    }

    #[test]
    fn shipped_migrations_are_sequential() {
        let mut conn = mem();
        assert_eq!(migrate(&mut conn, MIGRATIONS).unwrap(), MIGRATIONS.len() as u32);
    }
}
