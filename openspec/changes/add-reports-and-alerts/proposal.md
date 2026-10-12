## Why

Users want a record of the findings that they can keep or share. They also want the dashboard to warn them clearly when a drive gets low or a folder balloons, without any network traffic and without anything running outside the app.

## What Changes

- **Report export** as HTML, CSV (one file per section, zipped) and JSON (versioned schema). HTML is self-contained: inline CSS, inline SVG charts, and no external assets, scripts or web fonts. Sections: drive summary, Top-N folders/files, cleanup candidates and savings with manual instructions, duplicates, stale data, tiering suggestions, cleanup plans, and forecast. Files are written only to a location the user picks, or offered as a browser download.
- **In-app alerts only**: an alert centre (header bell with a count, and an Alerts page) plus dismissible banners on the home page and the affected drive view. Alerts are evaluated after every inventory refresh and every manual scan or quick rescan while the app is running:
  - **low space**: free space below 10% **or** below 20 GiB (the drive-inventory limits)
  - **fast growth**: a folder grows faster than 5 GiB/day (configurable) between the two latest snapshots of its root

  De-duplication and a cooldown prevent repeats.
- **No OS notifications** (no toasts, no AppUserModelID, no Start-menu shortcut), no scheduled scans, no Task Scheduler entries, and no background watchdog. Rescans are manual.

## Capabilities

### New Capabilities
- `reports-export`: HTML/CSV/JSON export of dashboard findings, self-contained and non-executable, written only to a user-chosen location.
- `alerts`: Low-space and fast-growth detection inside the running app, with an in-app alert centre and banners, de-duplication and cooldown.

### Modified Capabilities
None

## Impact

- `crates/diskdash-analysis` (`alerts` evaluators), `crates/diskdash` (`reports` module: askama templates compiled into the binary, CSV via the `csv` crate, ZIP via the `zip` crate, JSON via serde; report and alert routes with ts-rs types; SSE `alert` events), `crates/diskdash-store` (catalog tables `alert_state`, `alerts`).
- Frontend: Export dialog, alert bell, Alerts page and banners. All are built from the skeleton's shared components with designed loading, empty and error states.
- Safety rules touched: **1** (export and alert endpoints are POST + CSRF where they change state), **2** (the HTML report embeds everything; no network), **3** (reports read stored data only and are written only where the user chose; no OS-level side effects), **7** (local logs only).

## Depends on

`add-project-skeleton` (design system, app shell), `add-drive-inventory` (low-space limits), `add-filesystem-scan`, `add-cleanup-rules-engine`, `add-stale-data-analysis`, `add-duplicate-finder`, `add-cleanup-plans`, `add-storage-tiering-suggestions`, `add-history-and-trends` (growth rates and forecast).
