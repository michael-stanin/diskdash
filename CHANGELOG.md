# Changelog

All notable changes to diskdash are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- Drive queries start PowerShell with a plain `-Command` instead of `-EncodedCommand -ExecutionPolicy Bypass`, which some antivirus products (for example Avast) flagged as suspicious.
- `DISKDASH_NO_POWERSHELL=1` makes diskdash never start PowerShell; it then uses built-in Windows APIs only (less drive detail).

## [0.1.3] - 2026-10-22

### Added
- `--exit-when-idle`: diskdash stops by itself about 30 seconds after the last browser page is closed, and never while a scan or other job is running. `Start diskdash.cmd` uses it, so closing the page also closes the black window. `--idle-secs <n>` changes the wait.

## [0.1.2] - 2026-10-22

### Changed
- Starting diskdash while it is already running no longer fails with "port in use": it opens the browser on the running copy and exits. The portable zip's README and launcher say how to reopen the page and how to stop diskdash.

## [0.1.1] - 2026-10-22

### Fixed
- The Export dialog could open before the drive list had loaded and leave **Drive summary** disabled; report sections and scan modes now refresh when the drive list updates.

### Changed
- CI: Playwright failures are reported as annotations; the contrast check no longer races a hover transition.
- Dependabot proposes only minor and patch updates; GitHub Actions updated to setup-node 7.
- Open-source files: LICENSE (MIT), CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, third-party notices, issue and PR templates, release workflow.

## [0.1.0] - First public release

### Added
- Drive overview: capacity, free space, media type, health, TRIM state, low-space warnings.
- Folder scan with treemap and sunburst usage explorer, largest files and folders, file-type breakdown, hard-link and junction handling.
- Cleanup suggestions from 48 rules, ranked by space freed and risk, with manual steps and protected locations.
- Stale data, duplicate finder (BLAKE3), installed-apps view, storage tiering suggestions and cleanup plans with verification.
- History: compare two scans, free-space trend and forecast, automatic shrinking of old scans.
- Optional elevated read-only helper for fast NTFS MFT scans and a manual USN quick rescan.
- In-app alerts for low space and fast-growing folders.
- Report export as self-contained HTML, CSV (zip) and JSON, downloadable or saved to a folder.
- Portable Windows zip (`cargo xtask package`) and light/dark themes.

### Known limitations
- Windows 10/11 x64 only; binaries are not code-signed.
- The fast MFT scan has been tested on synthetic volumes; the real-volume test is opt-in and needs administrator rights, so treat it as experimental.

[Unreleased]: https://github.com/michael-stanin/diskdash/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/michael-stanin/diskdash/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/michael-stanin/diskdash/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/michael-stanin/diskdash/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/michael-stanin/diskdash/releases/tag/v0.1.0
