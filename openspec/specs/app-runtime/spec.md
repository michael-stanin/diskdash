# app-runtime Specification

## Purpose
TBD - created by archiving change add-project-skeleton. Update Purpose after archive.
## Requirements
### Requirement: Loopback-only binding
The server SHALL bind only to `127.0.0.1` on a configurable port (default 8765). It MUST refuse to start if it is configured with any other bind address, including `0.0.0.0`, `::`, or a LAN address.

#### Scenario: Default start binds loopback
- **WHEN** the server starts with default settings
- **THEN** it listens on `127.0.0.1:8765`
- **AND** a connection attempt to the machine's LAN IP on port 8765 is refused

#### Scenario: Non-loopback bind is rejected
- **WHEN** the server is started with bind address `0.0.0.0`
- **THEN** the process exits with a non-zero code and logs "only 127.0.0.1 is allowed"

#### Scenario: Port already in use
- **WHEN** port 8765 is already taken by another process
- **THEN** the server exits with a clear error naming the port and suggesting `-Port <n>`

### Requirement: Host header allow-list
The server MUST reject any HTTP request whose `Host` header is not `127.0.0.1:<port>` or `localhost:<port>`, and MUST respond with status 421 to such requests.

#### Scenario: Rebinding attempt is blocked
- **WHEN** a request arrives with `Host: evil.example:8765`
- **THEN** the server responds 421 and does not run the route handler

#### Scenario: Legitimate localhost request
- **WHEN** a request arrives with `Host: localhost:8765`
- **THEN** the request is processed normally

### Requirement: CSRF protection for state-changing requests
Every request using POST, PUT, PATCH or DELETE MUST carry an `X-Diskdash-Token` header matching the per-process session token, and an `Origin` header (when present) equal to the server's own origin. Otherwise the server SHALL respond 403. The token SHALL be delivered only inside the HTML page the server itself serves.

#### Scenario: Missing token
- **WHEN** a POST to `/api/v1/settings` arrives without `X-Diskdash-Token`
- **THEN** the server responds 403 and the settings are unchanged

#### Scenario: Cross-origin form post
- **WHEN** a POST arrives with a valid-looking token but `Origin: https://other.site`
- **THEN** the server responds 403

#### Scenario: Read-only requests need no token
- **WHEN** a GET to `/api/v1/health` arrives without a token
- **THEN** the server responds 200

### Requirement: No outbound network access
The application MUST NOT make any outbound network connection to anything other than the loopback interface. The served frontend MUST NOT reference any external URL: no CDN scripts, fonts, images, or analytics.

#### Scenario: Built bundle is self-contained
- **WHEN** the production frontend bundle is built
- **THEN** a CI check finds no `http://` or `https://` URLs in the emitted HTML/JS/CSS, other than `localhost`/`127.0.0.1` and an explicit allow-list of non-fetched strings such as SVG namespaces

#### Scenario: No HTTP client in the backend
- **WHEN** CI checks the dependency tree (`cargo tree`)
- **THEN** no HTTP client crate (reqwest, hyper client features, ureq, surf) is present in `diskdash.exe`, and the check fails the build if one appears

### Requirement: Read-only guarantee enforced in CI
The application MUST NOT contain code that modifies user files or system settings. CI SHALL enforce this over every workspace crate except `test-fixtures` and `#[cfg(test)]` code, in two ways:
- clippy `disallowed-methods`/`disallowed-types`: `std::fs::remove_*`, `std::fs::rename`, `std::fs::write`, `std::fs::copy`, `std::fs::set_permissions`, `File::create`, and `OpenOptions` (use is restricted to the allow-listed module);
- a source deny-list scan for Win32 APIs: `DeleteFileW`, `MoveFileExW`, `CopyFileW`, `SetFileTime`, `SetFileAttributesW`, `SetFileInformationByHandle`, `SHFileOperationW`, `IFileOperation`, `CreateSymbolicLinkW`, `CreateHardLinkW`, `RegSetValueExW`, `RegDeleteKeyW`, and `CreateFileW` with any write or delete access.

Writes SHALL be possible only through one allow-listed module (`diskdash-store::own_files`), which accepts only paths under the app data directory or a report path the user picked in a save dialog.

#### Scenario: Forbidden call introduced
- **WHEN** a change adds `std::fs::remove_dir_all(path)` in `crates/diskdash-analysis`
- **THEN** clippy fails the build and names the file and line

#### Scenario: Own data directory writes allowed
- **WHEN** code writes the SQLite database or log files through `own_files`
- **THEN** the check passes

### Requirement: Elevation detection and reduced mode
The system SHALL detect at startup whether the process is running elevated (administrator token). It SHALL report the result via `GET /api/v1/system` as `elevated: true|false`, along with the list of features that are unavailable in reduced mode. The UI SHALL show a persistent "Reduced mode" badge when the process is not elevated.

#### Scenario: Normal user start
- **WHEN** the server starts in a non-elevated session
- **THEN** `/api/v1/system` returns `elevated: false` and a non-empty `reduced_features` list
- **AND** the UI header shows a "Reduced mode" badge with a tooltip listing those features

#### Scenario: Elevated start
- **WHEN** the server starts from an elevated PowerShell
- **THEN** `/api/v1/system` returns `elevated: true` and an empty `reduced_features` list

### Requirement: Persistent settings
The system SHALL store user settings in the database and expose them via `GET /api/v1/settings` and `PUT /api/v1/settings`. It SHALL validate every value against a typed schema with bounds. Defaults SHALL be: low-space threshold 10% of capacity, low-space absolute floor 20 GiB, stale threshold 12 months, HDD scan concurrency 1 per physical disk, SSD scan concurrency 4, chart children per level 20 (allowed range 5–50), and hard-link detection on the system volume only. No setting SHALL exist that enables file-modifying behaviour, network drives or removable drives.

#### Scenario: First run defaults
- **WHEN** the application starts with an empty database
- **THEN** `GET /api/v1/settings` returns the documented defaults

#### Scenario: Invalid value rejected
- **WHEN** the user submits a low-space threshold of 150%
- **THEN** the server responds 422 with a field-level error and stores nothing

### Requirement: Local rotating logs
The system SHALL write logs to `%LOCALAPPDATA%\diskdash\logs\diskdash.log`, rotating at 10 MiB and keeping 5 files. Logs MUST NOT contain file contents or the CSRF token.

#### Scenario: Log rotation
- **WHEN** the active log file exceeds 10 MiB
- **THEN** it is rotated, and at most 5 rotated files remain

#### Scenario: Token never logged
- **WHEN** a request carrying `X-Diskdash-Token` is logged at DEBUG level
- **THEN** the token value appears as `***` in the log line

### Requirement: One-command start
The repository SHALL provide `start.ps1`, which in one invocation SHALL check for a Rust stable toolchain (cargo, MSVC target) and Node 20+; build the frontend and then `diskdash.exe` (release) when sources are newer than the build; start the server; and open the default browser at the server URL once `/api/v1/health` responds. A `-Dev` switch SHALL start the Vite dev server with API proxying instead of serving the built bundle.

#### Scenario: Node missing
- **WHEN** `start.ps1` runs and `node` is not on PATH
- **THEN** it stops before starting anything, prints the official install instructions, and exits non-zero

#### Scenario: Rust toolchain missing
- **WHEN** `start.ps1` runs and `cargo` is not on PATH
- **THEN** it stops, prints the rustup install instructions, and exits non-zero without downloading anything itself

#### Scenario: Happy path
- **WHEN** `start.ps1` runs with all prerequisites present
- **THEN** the browser opens the dashboard after the health check passes, and Ctrl+C in the console stops the server

### Requirement: Embedded frontend and API client
The release executable SHALL embed the built frontend and serve it with SPA fallback to `index.html` for client routes. The frontend API client SHALL use TypeScript types generated from the Rust API types, SHALL attach the CSRF token to state-changing requests, and SHALL show a connection indicator driven by the health endpoint and the SSE stream.

#### Scenario: Single executable
- **WHEN** `diskdash.exe` is copied alone to another folder and started
- **THEN** the full UI loads with no missing assets

#### Scenario: Backend stops
- **WHEN** the backend process stops while the UI is open
- **THEN** within 10 seconds the connection indicator shows "Disconnected", and API-dependent views show a retry prompt instead of stale data presented as live

#### Scenario: Generated types stay in sync
- **WHEN** a Rust API type changes without regenerating TS types
- **THEN** CI fails on the `ts-rs` export diff check

