# Changelog

All notable changes to diskdash are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/michael-stanin/diskdash/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/michael-stanin/diskdash/releases/tag/v0.1.0
