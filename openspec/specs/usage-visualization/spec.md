# usage-visualization Specification

## Purpose
TBD - created by archiving change add-usage-visualization. Update Purpose after archive.
## Requirements
### Requirement: Lazy hierarchical tree API with capped levels
The system SHALL expose a read-only endpoint that returns the children of a given snapshot entry, up to a requested depth (max 2). Each level SHALL be sorted by the selected size mode, descending, and capped at the configured child limit (default 20; the setting accepts 5 to 20). The remaining children SHALL be folded into one synthetic "Other" node carrying their count and summed logical and allocated sizes.

#### Scenario: Drill into a large directory
- **WHEN** the client requests children of a directory with 5,000 direct children with the default limit
- **THEN** the response contains 20 real child nodes sorted by size and one "Other" node with count 4,980
- **AND** the sizes of all returned nodes sum to the directory's aggregate size

#### Scenario: Lower cap in settings
- **WHEN** the user sets the per-level cap to 5
- **THEN** every treemap and sunburst level shows at most 5 children plus "Other"

#### Scenario: Unknown snapshot or entry
- **WHEN** the client requests a snapshot id or entry id that does not exist
- **THEN** the API responds 404 with an error body and does not return partial data

### Requirement: Treemap and sunburst drill-down
The drive view SHALL render the snapshot as an ECharts treemap and as a sunburst. Clicking a directory node SHALL drill into it and load its children lazily. A breadcrumb SHALL show the current path and allow navigation to any ancestor. Clicking "Other" SHALL open a paged table of the folded children, from which any directory can be opened in the chart.

#### Scenario: Click into a folder
- **WHEN** the user clicks a directory node in the treemap
- **THEN** the chart re-roots at that directory, the breadcrumb gains that segment, and only that directory's children are fetched

#### Scenario: Snapshot with scan errors
- **WHEN** the snapshot contains directories skipped due to access denied
- **THEN** those directories show an "incomplete" marker, and their tooltip states that the size is a lower bound

### Requirement: Top-N folders and files tables
The system SHALL provide Top-N largest folders (by recursive size) and largest files for a snapshot or subtree, served with paging from indexed queries. The UI SHALL show 20 rows by default with a "Show more" control. It SHALL render them in virtualised tables supporting sorting, path text filter, category filter, and drill-down into the treemap.

#### Scenario: Show more
- **WHEN** the user clicks "Show more" under the largest-files table
- **THEN** the next 20 rows are fetched and appended, and the DOM row count stays bounded by virtualisation

### Requirement: Category and extension breakdown
The system SHALL show per-category and per-extension totals (count, logical size, allocated size) for a snapshot or subtree, using the category computed at scan time. Selecting a category SHALL filter the Top-N files table to that category.

#### Scenario: Filter by category
- **WHEN** the user clicks the "Video" segment of the category chart
- **THEN** the largest-files table shows only files whose category is Video

### Requirement: Logical vs. on-disk size toggle
The UI SHALL provide one global toggle between logical size and allocated ("size on disk") size, and every chart, table and total SHALL use the selected mode. The server SHALL sort and cap by the selected mode.

#### Scenario: Toggle to size on disk
- **WHEN** the user switches the toggle from logical to on-disk
- **THEN** the treemap, tables and breakdown are re-queried with `size_mode=allocated`, and the toggle state persists across reloads

### Requirement: Open in Explorer and copy path
The UI SHALL offer "Copy path" and "Open in Explorer" for every file and folder row and node. "Open in Explorer" SHALL be a CSRF-protected POST that identifies the item by snapshot id and entry id, never by a raw client-supplied path. The backend SHALL resolve the path from the snapshot, verify that it exists using metadata only, and launch `explorer.exe /select,<path>` without a shell. It MUST NOT open, read or modify the target.

#### Scenario: Reveal an existing file
- **WHEN** the user chooses "Open in Explorer" on a file that still exists
- **THEN** Explorer opens with that file selected and the API responds 204

#### Scenario: Path no longer exists
- **WHEN** the entry was removed after the scan
- **THEN** the API responds 410 "path no longer exists" and Explorer is not launched

#### Scenario: Request without CSRF token
- **WHEN** a reveal request arrives without a valid CSRF token or with a foreign Origin header
- **THEN** the server rejects it with 403 and launches nothing

### Requirement: Designed loading, empty and error states
Every analysis view in this change (treemap, sunburst, Top-N tables, breakdown, drive cards) SHALL use the shared design-system components from the project skeleton. Each view SHALL present a skeleton placeholder while loading, a designed empty state with a next action, and an error state with message and retry. Raw spinners alone or blank panels MUST NOT be shown.

#### Scenario: Drive never scanned
- **WHEN** the user opens the drive view of a volume with no snapshot
- **THEN** an empty state explains that no scan exists and offers a "Scan now" button

#### Scenario: API error
- **WHEN** the tree request fails with a server error
- **THEN** the chart area shows the error state with the message and a Retry button, and the rest of the page stays usable

### Requirement: Themed charts and consistent category colours
ECharts charts SHALL be themed from the design tokens for both light and dark themes, and SHALL switch theme without reloading. A single category colour map SHALL be used by the treemap, sunburst, breakdown chart, table category chips and legends, and SHALL be exported for reuse by other views. Colours MUST meet WCAG AA contrast against their background in both themes.

#### Scenario: Switch to dark theme
- **WHEN** the user switches to the dark theme
- **THEN** all charts re-render with dark token colours, and each category keeps the same hue identity as in light mode

#### Scenario: Same colour everywhere
- **WHEN** "Archives" appears in the treemap, the breakdown chart and a table chip
- **THEN** all three use the same category colour

### Requirement: Numeric presentation
Sizes SHALL be shown with binary prefixes (B, KiB, MiB, GiB, TiB; 1 KiB = 1024 B) using tabular numerals. Hovering or focusing a size SHALL reveal the exact byte count. The API SHALL continue to return integer bytes.

#### Scenario: Hover exact bytes
- **WHEN** the user hovers "1.50 GiB" in a table
- **THEN** a tooltip shows "1,610,612,736 bytes"

### Requirement: Motion and navigation
Drill-down and breadcrumb transitions SHALL complete within 200 ms and SHALL be disabled when the user prefers reduced motion. The breadcrumb, tables and chart drill-down SHALL be keyboard navigable.

#### Scenario: Reduced motion
- **WHEN** the OS has reduced motion enabled and the user drills into a folder
- **THEN** the chart switches levels without animation

#### Scenario: Keyboard drill-up
- **WHEN** the user focuses a breadcrumb segment and presses Enter
- **THEN** the chart re-roots at that ancestor

### Requirement: Home page drive cards
The home page SHALL show one card per local fixed internal volume from `drive-inventory`. Each card SHALL show letter, label, media-type badge (HDD / SATA SSD / NVMe SSD), health status, colour-coded fill bar, and free space (absolute and %). It SHALL show a low-space warning when free space is below the percentage threshold (default 10%) or the absolute floor (default 20 GiB), and the latest snapshot time with a link to its drive view. A "Potential savings" area SHALL be shown when cleanup-rules data exists and hidden otherwise.

#### Scenario: Small drive below absolute floor
- **WHEN** a 128 GiB drive has 18 GiB free (14%)
- **THEN** the card shows the low-space warning because free space is below the 20 GiB floor

#### Scenario: Health limited in reduced mode
- **WHEN** SMART counters are unavailable because the elevated helper is not running
- **THEN** the health badge reads "limited (not elevated)" instead of an error

