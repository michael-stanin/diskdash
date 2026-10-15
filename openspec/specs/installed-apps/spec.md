# installed-apps Specification

## Purpose
TBD - created by archiving change add-stale-data-analysis. Update Purpose after archive.
## Requirements
### Requirement: Installed applications inventory
The system SHALL enumerate installed applications by reading the Uninstall registry keys under HKLM (64-bit view), HKLM WOW6432Node (32-bit view) and HKCU. Entries with `SystemComponent=1` or without `DisplayName` SHALL be omitted. Each application SHALL report name, publisher, version, install date and install location where present. Registry access MUST be read-only.

#### Scenario: Same app in 32- and 64-bit hives
- **WHEN** an application appears in both HKLM views with the same DisplayName and version
- **THEN** it is listed once, with both sources noted

#### Scenario: Unreadable key
- **WHEN** one Uninstall subkey cannot be opened due to access denied
- **THEN** that entry is skipped, the error is logged, and the rest of the list is returned

### Requirement: Application size
The system SHALL report each application's size from `EstimatedSize` when present. Otherwise, when `InstallLocation` matches a directory in the latest snapshot, it SHALL use that directory's allocated size. Otherwise it SHALL report "unknown". The source of the size SHALL be shown.

#### Scenario: Size from snapshot
- **WHEN** an app has no EstimatedSize and its InstallLocation `D:\Games\Foo` exists in the latest D: snapshot with 40 GiB
- **THEN** the app shows 40 GiB with source "scan"

### Requirement: Best-effort last-used time
The system SHALL estimate an application's last-used time from Prefetch data (only through the read-only elevated helper, when it is running) or from UserAssist entries (ROT13-decoded, current user), and SHALL show the source or "unknown". The non-elevated server MUST NOT attempt to read `C:\Windows\Prefetch`.

#### Scenario: Reduced mode (no helper)
- **WHEN** the read-only elevated helper is not running
- **THEN** last-used comes from UserAssist only, and the page notes that Prefetch data needs "Enable elevated features"

#### Scenario: Helper running with Prefetch
- **WHEN** the helper is running and a Prefetch entry exists for the app's main executable
- **THEN** last-used shows the most recent Prefetch run time with source "Prefetch"

### Requirement: Uninstall via Windows only
The system SHALL provide an "Open Apps & features" link to `ms-settings:appsfeatures` that the browser opens on the user's click. The backend MUST NOT launch it, and MUST NOT execute `UninstallString`, `QuietUninstallString`, or any uninstaller.

#### Scenario: User wants to remove an app
- **WHEN** the user clicks "Open Apps & features" on an application row
- **THEN** the browser hands the link to Windows, Settings opens at Apps & features, and the app starts no process

### Requirement: Installed apps view states
The Installed apps page SHALL use the shared design-system table and badge components. It SHALL show a skeleton while loading, an empty state when no applications are found, and an error state with retry when the registry cannot be read.

#### Scenario: Registry read fails entirely
- **WHEN** none of the Uninstall keys can be read
- **THEN** the page shows the error state with the reason and a Retry button

