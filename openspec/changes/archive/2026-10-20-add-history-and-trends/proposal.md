## Why

Disks fill up gradually. The user wants to know *what grew since last time* and *when each drive will be full*, so they can act before it happens. Snapshots already exist per scan (one SQLite file each), but they are not compared, and keeping every full snapshot forever would waste the very space the user is trying to free.

## What Changes

- A **snapshot diff** between two completed snapshots of the same root: top growing and shrinking folders, new and removed large files, and per-category deltas. Directories are matched by full path.
- A **free-space time series** per local fixed internal volume, recorded on every inventory refresh and every manual scan completion (not only on scans), with de-duplication of near-identical samples.
- A **days-until-full forecast** per volume, using linear regression over the time series. It requires at least 3 points spanning at least 7 days, and shows a confidence caveat (R², range). "Not enough data" is shown otherwise, and "not filling" for flat or shrinking trends.
- A **snapshot retention policy**: keep the last N full snapshots per root (default 5). Older snapshots are collapsed into directory-level summaries up to depth K (default 4) in the catalog, and then their snapshot files (the app's own files) are removed. Long-range diffs and charts still work. Free-space samples are kept indefinitely (they are small), and are downsampled to daily values after 90 days.
- UI: a "What grew" view (pick two snapshots, default latest vs. previous), a free-space chart with a forecast line, and forecast badges on home cards.

## Capabilities

### New Capabilities
- `history-trends`: Snapshot-to-snapshot diff, volume free-space time series, days-until-full forecast, and snapshot retention/downsampling.

### Modified Capabilities
None

## Impact

- `crates/diskdash-analysis` (`history` module: diff, forecast, retention), `crates/diskdash-store` (catalog tables `volume_samples` and `snapshot_summaries`; read-only ATTACH of snapshot files), `crates/diskdash` (routes with ts-rs types, and retention as a job after each scan). `drive-inventory` refresh and `filesystem-scan` completion hook into sample recording.
- Frontend: a History page, diff view, free-space chart (token-themed ECharts time series) and home card forecast badges. All are built from shared components with designed loading, empty and error states.
- Safety rules touched: **3** (retention removes only the app's *own* snapshot files and rows under `%LOCALAPPDATA%\diskdash`, never user files). Rescans are manual; there is no scheduler or watchdog.

## Depends on

`add-drive-inventory`, `add-filesystem-scan`, `add-usage-visualization`.
