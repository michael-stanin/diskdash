## ADDED Requirements

### Requirement: Snapshot record
Each scanned root SHALL produce one snapshot. The snapshot SHALL record: id, root path, volume id, volume serial, filesystem, cluster size, scan method (`walk` in this phase), elevation state, start and end timestamps, status (`running`, `partial`, `complete`, `failed`), and statistics (file count, directory count, link count, logical total, allocated total, error count, access-denied count, `link_count_unknown` count).

#### Scenario: Completed snapshot
- **WHEN** a scan of a fixture root finishes
- **THEN** `GET /api/v1/snapshots/{id}` returns status `complete`, and its file count and totals equal the values computed independently by the test fixture

### Requirement: Entry storage
The system SHALL store one row per file, directory and link in the snapshot. Each row SHALL hold: an entry id unique within the snapshot, parent entry id, name, kind (`file`, `dir`, `link`), logical size, allocated size, created/modified/accessed timestamps, attributes, reparse tag, file id, `hardlink_of`, extension, category, and flags (`access_denied`, `cloud_placeholder`, `link_count_unknown`). Directories SHALL additionally store their full path, recursive logical total, recursive allocated total, recursive file count, recursive directory count, and the newest modified and accessed timestamps among all descendants (`max_mtime`, `max_atime`). Every entry SHALL store a lowercase copy of its name (`name_lower`), indexed for case-insensitive rule matching. A file's full path SHALL be derivable as its parent directory's path joined with its name.

#### Scenario: Path reconstruction
- **WHEN** the API returns a file entry
- **THEN** its `path` equals the parent directory path joined with its name, and matches the real path on disk

#### Scenario: Newest descendant time
- **WHEN** a fixture directory contains a file modified in 2020 and a nested file modified in 2025
- **THEN** the directory's `max_mtime` equals the 2025 timestamp

### Requirement: Aggregate correctness
For each directory in a complete snapshot, the recursive totals SHALL equal the sum of its direct files' sizes plus its child directories' recursive totals. Link entries contribute 0. Hard-link repeats contribute 0. The root directory's totals SHALL equal the snapshot statistics.

#### Scenario: Aggregates match fixture expectation
- **WHEN** a fixture tree with known sizes (including a junction, a hard link pair and an access-denied folder) is scanned
- **THEN** every directory's recursive totals match the fixture's expected totals exactly

### Requirement: Browse children
`GET /api/v1/snapshots/{id}/children?path=<dir>` SHALL return the direct children of a directory, sorted by a requested size measure (logical or allocated) in descending order, with pagination (`limit` up to 1000, cursor). Each child SHALL include its kind, sizes, counts (for directories), modified time, category (for files) and flags.

#### Scenario: Drill into a folder
- **WHEN** the client requests the children of `D:\Media`, sorted by allocated size, with limit 50
- **THEN** it receives at most 50 children in descending allocated-size order, plus a cursor if more exist

#### Scenario: Path not in snapshot
- **WHEN** the client requests the children of a path that is not a directory in the snapshot
- **THEN** the server responds 404

### Requirement: Top-N largest folders and files
The system SHALL return the N largest files (`/top-files`) and the N largest directories (`/top-dirs`) in a snapshot, optionally limited to a subtree (`under=<path>`). N defaults to 100, with a maximum of 1000. The size measure is selectable. For `/top-dirs`, an optional `max_depth` relative to the subtree limits results; without it, ancestors and descendants can both appear.

#### Scenario: Top files on a volume
- **WHEN** the client requests the top 100 files of a `D:\` snapshot
- **THEN** it receives 100 files sorted by size descending, each with its full path
- **AND** the response returns within 1 second for a snapshot of 1 M entries

#### Scenario: Top folders under a subtree
- **WHEN** the client requests top directories under `C:\Users\me` with `max_depth=2`
- **THEN** only directories at depth 1–2 below that path are returned

### Requirement: Breakdown by category and extension
The system SHALL return, for a snapshot or a subtree, per-category totals (file count, logical, allocated) and per-extension totals. Extensions SHALL be sorted by allocated size, with a `limit` and an `other` bucket for the remainder. Hard-link repeats SHALL be excluded.

#### Scenario: Category breakdown sums to total
- **WHEN** the client requests the category breakdown for a complete snapshot
- **THEN** the sum of category logical totals equals the snapshot's logical total

### Requirement: Error log access
The system SHALL expose a snapshot's errors via `GET /api/v1/snapshots/{id}/errors`, with pagination and filtering by Windows error code, and SHALL summarise them by error code.

#### Scenario: Viewing access-denied paths
- **WHEN** the user opens the error list for a `C:\` snapshot taken without elevation
- **THEN** access-denied paths are listed with error code 5, and the summary shows their count

### Requirement: Snapshot listing and deletion
The system SHALL list snapshots with their root, status, timestamps and totals (`GET /api/v1/snapshots`, filterable by volume and root). It SHALL delete a snapshot and all its rows on `DELETE /api/v1/snapshots/{id}`. Deleting a snapshot whose scan is running MUST be refused with 409.

#### Scenario: Delete old snapshot
- **WHEN** the user deletes a complete snapshot
- **THEN** its entries, errors and statistics are removed, and the database file is eligible for space reuse

#### Scenario: Delete running snapshot
- **WHEN** the user tries to delete a snapshot whose status is `running`
- **THEN** the server responds 409, and nothing is deleted

### Requirement: Latest snapshot per root
The system SHALL identify the latest `complete` snapshot for each root and volume. Downstream features (visualisation, rules, stale analysis) SHALL use it by default. Each API response built from snapshot data SHALL include the snapshot id and its end timestamp, so the UI can show "data as of …".

#### Scenario: Partial snapshot not used by default
- **WHEN** the most recent snapshot of `D:\` is `partial` and an older one is `complete`
- **THEN** default views use the complete one and show a notice that a newer partial scan exists
