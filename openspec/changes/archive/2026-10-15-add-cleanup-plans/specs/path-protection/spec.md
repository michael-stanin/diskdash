## ADDED Requirements

### Requirement: Built-in protected paths are never suggested
The system SHALL never produce suggestions for built-in protected paths: the Windows directory tree (except documented temp locations such as `%WINDIR%\Temp` and `%WINDIR%\SoftwareDistribution\Download`), `Program Files`, `Program Files (x86)`, the `ProgramData` root itself, each user profile root itself, every drive root, `System Volume Information`, `$Recycle.Bin`, `Recovery`, and `Boot`. Matching MUST be case-insensitive on normalised full paths at path-component boundaries.

#### Scenario: Windows temp allowed
- **WHEN** a cleanup rule matches `C:\Windows\Temp`
- **THEN** it is suggested

#### Scenario: System32 never suggested
- **WHEN** a stale or duplicate analysis finds a match under `C:\Windows\System32`
- **THEN** that match is not stored as a suggestion

### Requirement: Active system files only as system-setting suggestions
The active pagefile, hibernation file and swapfile SHALL never appear as manual-cleanup items. They SHALL appear only as system-setting suggestions with instructions.

#### Scenario: Pagefile in a duplicates or stale list
- **WHEN** `C:\pagefile.sys` would qualify as an old large file
- **THEN** it is excluded from the stale list and appears only under the system-setting suggestion

### Requirement: User exclusions and protected paths
The user SHALL be able to add and remove excluded and protected paths in Settings. Suggestions at or under those paths SHALL be filtered out of every suggestion source.

#### Scenario: Excluded folder
- **WHEN** the user excludes `D:\Photos`
- **THEN** duplicates, stale data, cleanup candidates and tiering suggestions no longer include anything under `D:\Photos`

### Requirement: Backend enforcement
Protection SHALL be applied in the backend by every suggestion producer and again when an item is added to a plan. The plan API MUST reject protected or excluded paths even if a crafted request references them.

#### Scenario: Crafted add request
- **WHEN** a request adds a snapshot entry under `C:\Program Files` to a plan
- **THEN** the API responds 422 with reason `protected:program_files` and the plan is unchanged

#### Scenario: Exclusion added after planning
- **WHEN** the user excludes a path that already has items in a plan
- **THEN** those items are flagged "now excluded" and dropped from the plan totals
