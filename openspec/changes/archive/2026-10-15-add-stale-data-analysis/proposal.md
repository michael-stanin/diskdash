## Why

A large share of a full disk is usually data nobody has touched in years, plus applications that are no longer used. The user needs these ranked by size and age. The ranking must be honest about how reliable Windows last-access times are, because they are disabled on many systems.

## What Changes

- **Last-access reliability check** per volume, using `fsutil behavior query disablelastaccess`. The result is interpreted as 0/1/2/3, including the system-managed values (`0x80000002` / `0x80000003`). On Windows 10 1803+ the system-managed default enables last-access updates only on volumes of 128 GB or less. When atime is not reliable, the UI shows a warning and the analysis falls back to last-modified time.
- **Stale files and folders** that have not been accessed (when reliable) or modified in N months. Presets are 6/12/24/36 months, and the default is 12 (confirmed). When atime is unreliable, the warning includes the manual command `fsutil behavior set disablelastaccess 0` as copyable text. The app never runs it. Results are ranked by size, with a **size × age score** for "old large files", and folder roll-ups use the newest descendant time.
- **Installed applications** from the Uninstall registry keys (HKLM 64-bit, HKLM WOW6432Node, HKCU): name, publisher, version, install date and size. Size comes from `EstimatedSize`, or by matching `InstallLocation` to snapshot aggregates. Last used is best effort: UserAssist (ROT13-decoded, per user) always, and Prefetch only when the read-only elevated helper from `add-fast-ntfs-scan` is running (the server itself never runs elevated).
- An "Open Apps & features" link (`ms-settings:appsfeatures`, opened by the browser on the user's click; the backend launches nothing). The app **never uninstalls anything** and never runs `UninstallString`.
- All analysis is read-only and works from snapshot data plus registry (and, optionally, Prefetch) metadata.

## Capabilities

### New Capabilities
- `stale-data-analysis`: Last-access reliability detection, stale file/folder queries with age presets, size × age scoring, and the time-basis fallback.
- `installed-apps`: Read-only inventory of installed applications with size, install date, best-effort last-used, and a link to Windows Apps & features.

### Modified Capabilities
None

## Impact

- `crates/diskdash-analysis` (`stale` module), `crates/diskdash-inventory` (`apps` module: registry via the `windows` crate registry APIs or `winreg`), `crates/diskdash-store` (`time_basis` in snapshot metadata), and `crates/diskdash` routes with ts-rs types.
- Frontend: a Stale tab in the drive view and an Installed apps page, built from shared components with designed loading, empty and error states.
- `cleanup-rules` uses the per-volume atime reliability flag for `min_age_days`.
- Safety rules touched: **3** (read-only: registry is only read; Prefetch is read only through the read-only helper; no uninstall; `fsutil ... set` is shown as text only), **4** (stale results exclude protected and excluded paths).

## Depends on

`add-project-skeleton`, `add-drive-inventory`, `add-filesystem-scan`, `add-usage-visualization`.
