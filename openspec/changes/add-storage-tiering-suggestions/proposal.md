## Why

The user has a mix of fast SSDs and large HDDs. A full SSD often holds cold data that would fit fine on an HDD, while active projects or game libraries sit on a slow HDD. Suggesting moves between tiers frees SSD space without deleting anything, and the projected free space makes the trade-off visible. The app only suggests and explains; the user moves data by hand.

## What Changes

- **Cold-on-SSD suggestions**: folders and files on SSD/NVMe volumes that have not been modified in at least N months (default 12; access time is used only when the volume's time basis is atime), with size at or above a minimum (default 1 GiB). Each is matched to an internal HDD target with enough free space, so that the target stays above its low-space limit (10% **and** 20 GiB) plus a headroom margin.
- **Hot-on-HDD suggestions**: folders on internal HDDs with many files modified in the last N days (default 30), and known app/game libraries detected from Steam `libraryfolders.vdf` / `appmanifest_*.acf` and Epic `Manifests\*.item`. They are suggested for an internal SSD only if the SSD stays above its limit.
- **Projected free space** per drive for the suggestions the user marks as "planned", recomputed as they are toggled.
- **Manual move instructions** per suggestion, shown as copyable text only: the app's own move feature where one exists (Steam → Properties → Installed Files → Move install folder; Epic → Move), otherwise a `robocopy "<src>" "<dst>" /E /COPY:DAT /DCOPY:T` line plus "verify, then delete the source via Explorer". The app never runs these commands.
- Suggestions can be added to a cleanup plan (`add-cleanup-plans`) as `manual-move` items, which are verified after the next manual rescan.
- Only local fixed internal drives are sources or targets (no USB, removable or network drives). Protected paths and exclusions are filtered out. Installed-app directories are flagged.

## Capabilities

### New Capabilities
- `storage-tiering`: Cold/hot classification per media type, internal target-drive selection with headroom, app/game library detection, projected free space, and manual move instructions with plan hand-off.

### Modified Capabilities
None

## Impact

- `crates/diskdash-analysis` (`tiering` module; read-only Steam VDF parser (a small KeyValues parser or a maintained crate) and Epic `.item` JSON via serde; instruction templates), `crates/diskdash-store` (catalog table `tiering_planned`), and `crates/diskdash` routes with ts-rs types.
- Frontend: a Tiering page with suggestion lists, "planned" toggles, projected free-space bars, copyable instructions, and "Add to plan". It is built from shared components with designed loading, empty and error states.
- Safety rules touched: **3** (analysis only; manifests are read, never written; move commands are shown as text and never executed), **4** (protected and excluded paths are never suggested).

## Depends on

`add-drive-inventory` (media type, low-space limits), `add-filesystem-scan`, `add-stale-data-analysis` (time basis, installed apps), `add-cleanup-plans` (plan items, path protection).
