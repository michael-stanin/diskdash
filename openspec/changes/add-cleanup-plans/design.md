## Context

The product owner has ruled that the app performs no file operations: "the purpose is identifying and suggesting, not acting in any case." Users still need a path from "suggestion" to "space freed". The plan bridges that gap. It collects choices, tells the user exactly what to do by hand, and then measures the result from a later manual rescan. Suggestions come from several capabilities (cleanup-rules, stale-data-analysis, duplicate-finder, storage-tiering), each with its own data shape.

## Goals / Non-Goals

**Goals:** one uniform plan item model across suggestion sources; clear, copyable manual instructions; no executable artefacts; protection applied centrally in the backend; accurate "actually freed" figures.

**Non-Goals:** any file operation (delete, recycle, move, compress, archive, rename, link); running shell commands or opening files; generating scripts (PowerShell/batch); watching the filesystem for changes (a background "watchdog" is not planned); undo (nothing is done, so nothing needs undoing).

## Decisions

### D1. Plan item = snapshot reference + suggestion reference + baseline
Plans live in the catalog DB. Snapshots are separate SQLite files that retention may summarise or remove, so items copy what they need. `plan_items(plan_id, snapshot_id, entry_id, path, kind, source, source_ref, risk, baseline_logical, baseline_allocated, baseline_mtime, baseline_file_id, instructions_key, status[todo|done|skipped], note)`. Items are created from snapshot entry ids or stored suggestion ids (rule matches, dup proposals, stale results, tiering suggestions), never from free-form client paths. The path and baseline sizes are copied at add time, so that the plan stays readable after retention summarises the snapshot.
- *Alternative: store only references.* It lost because retention (history-trends) may delete entry rows, which would make old plans unreadable.

### D2. Instructions are templates rendered as text, never executed
Each suggestion kind maps to an instruction template:
- *manual-cleanup* in Explorer: "Select in Explorer (button) → Delete (goes to Recycle Bin)"
- *system-setting*: for example `powercfg /h /type reduced` shown verbatim with "run in an elevated terminal yourself"
- *tool*: "Use Settings → System → Storage → Temporary files", or "Steam → Properties → Installed files → Move install folder"
- *manual-move*: a `robocopy "<src>" "<dst>" /E /COPY:DAT /DCOPY:T` line shown as text, with advice to verify before deleting the source

Templates live in YAML alongside the rules, so they can be edited without code. The UI provides "Copy" buttons only. No endpoint runs any command except `explorer.exe /select,` (from usage-visualization).
- *Alternative: export a commented PowerShell script defaulting to Recycle Bin.* It lost on the owner's direction. An exported script is one double-click away from executing, so export carries paths and instructions only.

### D3. Exports: CSV and TXT
CSV columns: status, risk, kind, path, logical_bytes, allocated_bytes, instructions, note. TXT is a human-readable checklist grouped by drive and kind, with `[ ]` boxes. Files are written through a browser download (or to a user-chosen folder), never to the paths in the plan. Paths with commas or quotes are CSV-escaped. Instruction text has no shell-executable header, and the file extension is never `.ps1`, `.bat` or `.cmd`.

### D4. Central protection filter
`protection::is_suggestable(&Path) -> Result<(), Blocked>` in `diskdash-analysis` is a pure function over normalised, case-folded full paths with component-boundary prefix matching. It is built once per job from the settings snapshot, so it holds no global mutable state. It is applied:
1. by every suggestion producer (rules, stale, duplicates, tiering) before results are stored, and
2. by the plan API on add, as a defence against crafted requests.

The built-in list covers the `%WINDIR%` tree (except documented temp locations such as `%WINDIR%\Temp` and `SoftwareDistribution\Download`, which are suggestable with instructions), Program Files and (x86), the ProgramData root itself, each user profile root itself, drive roots, `System Volume Information`, `$Recycle.Bin`, `Recovery`, and `Boot`. The active pagefile/hiberfil/swapfile are excluded from manual-cleanup and appear only as system-setting suggestions. User exclusions and user-protected paths are stored in settings. A suggestion inside a junction target is evaluated on the snapshot's recorded path (the scanner does not follow reparse points, so no redirection is possible).

### D5. Verification by snapshot comparison
After a manual rescan whose root covers a plan item, a job on a worker thread opens the new snapshot DB read-only and looks up each item's path (via the `path_key` index) in it. The outcomes are:
- *removed*: freed = baseline allocated
- *still present*: freed = baseline − current, which can be negative ("grew")
- *partially removed*: directories with smaller totals
- *moved* (manual-move items): the source is gone and the destination path is present with a similar size

Per-plan totals are reported against the volume free-space delta between the two scans, as a sanity check (other activity also changes free space, so it is labelled "volume change, includes unrelated activity"). Items not covered by the rescan root stay "unverified".
- *Alternative: stat each path live.* It lost because it would add filesystem reads outside a scan. It is cheap but inconsistent with snapshot-based reporting, and a rescan is needed anyway for the dashboard to update.

## Risks / Trade-offs

- The user deletes a different file at the same path between scans → verification reports by path and size only; documented.
- Stale plans (the snapshot is old) → the plan shows the baseline age and suggests a rescan before acting.
- Instructions get out of date for third-party apps (Steam UI changes) → templates are data, and the user can edit them.

## Open Questions

None.
