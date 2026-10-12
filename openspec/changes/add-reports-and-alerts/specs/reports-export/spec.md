## ADDED Requirements

### Requirement: Report formats and sections
The system SHALL export reports as HTML, CSV and JSON. Each report SHALL contain the sections the user selects from: drive summary, Top-N folders and files, cleanup candidates with savings by risk and manual instructions, duplicate groups, stale data, tiering suggestions, cleanup plans, and free-space forecast. JSON exports SHALL include a schema version. CSV exports SHALL provide one file per section, bundled in a ZIP. Sizes SHALL be integer bytes in CSV/JSON, and timestamps UTC RFC 3339.

#### Scenario: JSON export
- **WHEN** the user exports a JSON report for drive C:
- **THEN** the file contains `schema_version` and the selected sections, with sizes in bytes

#### Scenario: Section without data
- **WHEN** the user includes duplicates but no duplicate analysis has run
- **THEN** the section is present with status "not available" rather than failing the export

### Requirement: Self-contained, non-executable HTML
HTML reports SHALL be fully self-contained: inline CSS and inline charts, with no external scripts, stylesheets, fonts, or remote URLs. They MUST open correctly offline. Reports MUST NOT contain executable script files or be produced with script extensions (`.ps1`, `.bat`, `.cmd`). Instruction commands SHALL appear as text only.

#### Scenario: Offline report
- **WHEN** the HTML report is opened on a machine without network access
- **THEN** all content and charts render, and the document references no `http(s)://` resources (verified by a test that scans the output)

### Requirement: Read-only report generation
Report generation SHALL read only stored data. It MUST NOT access or modify user files beyond writing the report file atomically to the location the user chose (or delivering it as a browser download).

#### Scenario: Write location not writable
- **WHEN** the chosen output folder denies write access
- **THEN** the export fails with a clear error and no partial file is left behind

### Requirement: Export dialog design and states
The Export dialog SHALL use the shared design-system dialog, checkbox and button components. Sections without data SHALL be marked "not available" with the reason. It SHALL show progress while the report is generated, and an error state with the reason and Retry on failure.

#### Scenario: Nothing scanned yet
- **WHEN** the user opens Export before any scan has completed
- **THEN** the dialog shows an empty state explaining that only the drive summary is available, and offers "Scan a drive"
