## Why

The app is strictly read-only: it identifies and suggests, and it never acts. To free space, the user needs one place where the chosen suggestions (cleanup candidates, duplicates, stale data, tiering moves) become a concrete checklist: what to remove or move, how big it is, and exactly how to do it by hand. They also need confirmation afterwards of what was actually freed. This replaces the earlier "safe cleanup actions" idea. The product owner decided the app never deletes, recycles, moves, compresses or archives user files.

## What Changes

- **Cleanup plans**: the user adds suggestions from any analysis view to a named plan (checklist). For each item, a plan shows path, kind, size (logical and allocated), risk, source suggestion, and **manual instructions** (for example "Delete via Explorer → Recycle Bin", "Use Disk Cleanup", "Use Steam → Move install folder"). It also shows item count and total bytes. Items can be marked *done* or *skipped*, with an optional note.
- **Export** of a plan as CSV or TXT containing **paths and instructions only**. No executable script is generated, because the export must never be something that can be run directly.
- **Path protection**: built-in protected paths (Windows tree, Program Files, ProgramData root, profile roots, drive roots, system volume folders, the active pagefile/hiberfil/swapfile) and user exclusions **filter suggestions**. Protected or excluded paths are never suggested and are rejected when added to a plan. This is enforced in the backend.
- **Cleanup verification**: after the user's next *manual* rescan of the affected root, the system compares the new snapshot with the plan's baseline. It reports the bytes actually freed per item and per plan, marks items that still exist, and flags items that grew or were only partly removed.
- Wires an "Add to plan" button on Recommendations, cleanup candidates, duplicates, stale data and tiering.

## Capabilities

### New Capabilities
- `cleanup-plans`: User-curated cleanup checklists built from suggestions, with sizes, manual per-item instructions, done/skipped tracking, and CSV/TXT export (paths and instructions only).
- `path-protection`: Built-in protected paths and user exclusions that filter every suggestion source and are enforced in the backend.
- `cleanup-verification`: Post-rescan comparison that reports what was actually freed per plan item, and what still exists.

### Modified Capabilities
None

## Impact

- `crates/diskdash-analysis` (`plans` module: item model, instruction templates per suggestion kind, CSV/TXT writers; `protection` module: shared filter used by rules, stale, duplicates and tiering), `crates/diskdash-store` (catalog tables `plans`, `plan_items`, `plan_verifications`), and `crates/diskdash` routes with ts-rs types.
- Frontend: a Plans page (checklist, totals, export, verification results), "Add to plan" buttons, and Settings for exclusions and protected paths. All are built from the skeleton's shared components with designed loading, empty and error states.
- Safety rules touched:
  - **1**: plan endpoints are POST + CSRF.
  - **3**: the app never changes user files. Its only writes are its own DB rows and export files at a user-chosen location. Commands in instructions are shown as copyable text and never executed.
  - **4**: this change owns `path-protection`, enforced in the backend.
  - **5**: system files get instructions only.

## Depends on

`add-project-skeleton`, `add-drive-inventory`, `add-filesystem-scan`, `add-usage-visualization`, `add-cleanup-rules-engine`, `add-stale-data-analysis`, `add-duplicate-finder`.
