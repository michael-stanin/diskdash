# persistence Specification

## Purpose
TBD - created by archiving change add-project-skeleton. Update Purpose after archive.
## Requirements
### Requirement: Database location and mode
The system SHALL store all state in a single SQLite database at `%LOCALAPPDATA%\diskdash\diskdash.db`, configured with `journal_mode=WAL`, `foreign_keys=ON`, and a busy timeout of at least 5 seconds. The location SHALL be overridable through the `DISKDASH_DATA_DIR` environment variable, which tests use.

#### Scenario: First start creates the database
- **WHEN** the server starts and no database exists
- **THEN** the directory and database are created, and `PRAGMA journal_mode` returns `wal`

#### Scenario: Tests use an isolated database
- **WHEN** the test suite runs with `DISKDASH_DATA_DIR` set to a temp directory
- **THEN** no file is created under the real `%LOCALAPPDATA%\diskdash`

### Requirement: Versioned migrations
The system SHALL apply numbered SQL migration files (`NNNN_<name>.sql`, embedded in the executable at build time) in order at startup, inside a transaction per migration, and SHALL record the applied version in `schema_version`. It MUST refuse to start against a database whose schema version is newer than the code knows.

#### Scenario: Upgrade applies pending migrations
- **WHEN** the database is at version 1 and the code ships migrations 1–3
- **THEN** migrations 2 and 3 are applied in order and `schema_version` reads 3

#### Scenario: Failed migration rolls back
- **WHEN** migration 2 fails partway through
- **THEN** its changes are rolled back, `schema_version` stays at 1, and the server exits with the migration error

#### Scenario: Downgrade protection
- **WHEN** the database is at version 7 and the code knows only up to version 5
- **THEN** the server refuses to start and tells the user that a newer version created the database

### Requirement: Base schema
The initial migration SHALL create the tables `schema_version`, `settings` (key, JSON value, updated_at), `jobs` (id, kind, params JSON, status, progress fields, created/started/finished timestamps, error), and `job_events` (job id, sequence, timestamp, type, payload JSON).

#### Scenario: Schema present after first start
- **WHEN** the server has started once
- **THEN** all four tables exist with the documented columns

### Requirement: Single-writer discipline
All writes from background jobs SHALL go through a dedicated connection per job thread using batched transactions. Request handlers SHALL use a small read-connection pool and SHALL NOT hold write transactions open across `.await` points. Reads from the API MUST NOT block on a running scan's writes.

#### Scenario: UI stays responsive during heavy writes
- **WHEN** a worker is inserting rows in batches
- **THEN** concurrent API read requests complete without `database is locked` errors

