# history-trends Specification

## Purpose
TBD - created by archiving change add-history-and-trends. Update Purpose after archive.
## Requirements
### Requirement: Snapshot diff
The system SHALL compute a diff between two completed snapshots of the same scan root by matching directories on normalised full path. It SHALL return the top growing and top shrinking folders by allocated-size delta, folders that appeared or disappeared, new files at or above a size threshold (default 100 MiB), and per-category deltas. Diffing snapshots of different roots MUST be rejected.

#### Scenario: Folder grew
- **WHEN** `D:\Downloads` was 10 GiB in the previous snapshot and 25 GiB in the latest
- **THEN** it appears among the top growing folders with a delta of +15 GiB

#### Scenario: Different roots
- **WHEN** the user requests a diff between a snapshot of `C:\` and one of `D:\Data`
- **THEN** the API responds 400 with "snapshots must share the same root"

#### Scenario: Older snapshot is summarised
- **WHEN** one side of the diff has been reduced to a directory summary of depth 4
- **THEN** the diff is computed to depth 4, the new-files list is omitted, and the UI notes the reduced detail

### Requirement: Free-space time series
The system SHALL record a free-space sample (volume GUID, timestamp, total, free, source) on every drive-inventory refresh and every manual scan completion, skipping samples that duplicate the previous one within 10 minutes and 0.1%. Samples SHALL be keyed by volume GUID, so that drive-letter changes do not break the series. The UI SHALL show free space over time per volume.

#### Scenario: Inventory refresh without scan
- **WHEN** the user refreshes the inventory twice an hour apart with no scan
- **THEN** two samples are stored for each volume and appear on the chart

#### Scenario: Volume temporarily unavailable
- **WHEN** an internal volume cannot be queried during a refresh (for example BitLocker-locked)
- **THEN** no sample is recorded for it and its chart shows a gap, not a zero value

### Requirement: Days-until-full forecast
The system SHALL forecast, per volume, the number of days until free space reaches zero and until it reaches the low-space limit (below 10% or below 20 GiB, whichever comes first). It SHALL use a linear regression over recent samples, and SHALL produce a forecast only when there are at least 3 samples spanning at least 7 days. The forecast SHALL include a confidence indicator (R²) and a caveat that it is a linear extrapolation. It SHALL report "not filling" when the trend is flat or increasing.

#### Scenario: Insufficient data
- **WHEN** a volume has 2 samples, or 5 samples spanning 3 days
- **THEN** the forecast shows "not enough history" instead of a number

#### Scenario: Steady growth
- **WHEN** free space dropped linearly by 1 GiB/day over 30 days with 50 GiB left
- **THEN** the forecast shows about 50 days until full with high confidence

### Requirement: Snapshot retention
The system SHALL keep the newest N full snapshots per scan root (default 5, configurable) and SHALL reduce older snapshots to directory-level summaries up to depth K (default 4), including category totals. Pinned snapshots SHALL be exempt. After its summary is committed, a summarised snapshot's file SHALL be removed. Retention MUST only remove the application's own snapshot files and rows inside its data directory, and MUST NOT touch user files. It SHALL run as a background job after each successful scan.

#### Scenario: Sixth scan
- **WHEN** a sixth full snapshot of `C:\` completes with N = 5
- **THEN** the oldest unpinned full snapshot is summarised to depth 4 in the catalog and its snapshot file is removed from the app's data directory

#### Scenario: Retention job fails midway
- **WHEN** the retention job is interrupted after writing the summary but before removing the snapshot file
- **THEN** the snapshot is marked summarised, its file is removed on the next run, and no partial summary is ever visible

### Requirement: History views design and states
The History page, the diff view and the free-space chart SHALL use the shared design-system components, the shared category colour map, and token-themed ECharts in light and dark. Each SHALL show a skeleton while loading, a designed empty state, and an error state with retry. Forecast numbers SHALL always be shown with their confidence caveat.

#### Scenario: Only one snapshot of a root
- **WHEN** the user opens the diff view for a root with a single snapshot
- **THEN** an empty state explains that two scans are needed and offers "Rescan now"

#### Scenario: Forecast unavailable
- **WHEN** a volume has too little history for a forecast
- **THEN** the chart shows the samples it has and a "Not enough history yet" label instead of a forecast line

