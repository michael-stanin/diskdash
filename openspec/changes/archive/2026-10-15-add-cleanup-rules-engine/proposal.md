## Why

Seeing where the space goes is only half the job. The user also needs to know *what can be safely removed*, why, and how risky that is. This change adds an extensible, declarative rules engine that marks cleanup candidates in a snapshot and estimates potential savings per drive by risk level. It only suggests: the app is strictly read-only and never deletes anything. The user acts by hand, following the instructions shown. Checklists of chosen suggestions come in `add-cleanup-plans`.

## What Changes

- YAML rule files: built-in rules live in `rules/` in the repo and are embedded in `diskdash.exe` at build time; user rules live in `%LOCALAPPDATA%\diskdash\rules\`. A user rule with the same `id` overrides a built-in rule. Rules can also be disabled per id.
- Rule schema: `id`, `category`, `title`, `explanation`, `risk` (Safe/Review/Expert), `paths` (with `%ENV%` and known-folder expansion), `globs`, optional `min_age_days`, `kind` (`manual-cleanup` | `system-setting`), `instructions` (always required: text the user follows or copies, never executed by the app), and optional `conditions` (for example `sibling_exists: package.json`).
- The evaluator runs as a background job against **snapshot data only**. The only extra filesystem reads are existence and size checks of known system locations (hiberfil/pagefile/swapfile metadata, Recycle Bin size per drive via `SHQueryRecycleBinW`, and shadow-storage size only when the read-only elevated helper is available; otherwise "not measured"). It never reads file contents.
- All built-in rules from the brief: Windows temp and caches, upgrade leftovers, large system files (`system-setting` with powercfg/DISM/vssadmin instructions), Recycle Bin, browser caches, developer artifacts (with sibling-marker conditions), `.vhdx` compaction advice, app caches, Downloads triage (including the "archive already extracted" heuristic), and old logs/tmp files.
- **Potential savings** per drive and per risk, shown on the home cards and in a Recommendations list ranked by bytes vs. risk. Each item shows its instructions with copy buttons and "Open in Explorer". An "Add to plan" button arrives with `add-cleanup-plans`.
- Rule validation: malformed rules are reported in the UI and logs with file and line, and skipped. The engine never crashes because of a bad rule.

## Capabilities

### New Capabilities
- `cleanup-rules`: Declarative YAML rule definitions, loading/validation/override, evaluation against snapshots, built-in rule set, savings estimation by risk, and the Recommendations list (report only).

### Modified Capabilities
None

## Impact

- `crates/diskdash-analysis` (`rules` module: serde schema, loader, evaluator, built-ins), `crates/diskdash-store` (a `rule_matches` table in the per-snapshot DB), and `crates/diskdash` (API routes with ts-rs types).
- Frontend: a cleanup candidates tab in the drive view, the Recommendations page, a Rules page, and savings on the home cards. All are built from the skeleton's shared components, with designed loading, empty and error states.
- Repo: `rules/*.yaml` built-in definitions.
- Safety rules touched: **3** (read-only: no file changes, no content reads, cloud placeholders never opened, instruction commands shown as text and never executed), **4** (protected paths and exclusions are filtered by `path-protection` from `add-cleanup-plans`; until then a built-in protected list is applied), **5** (system files get instructions only; `DISM /AnalyzeComponentStore` is suggested, never run).

## Depends on

`add-project-skeleton`, `add-drive-inventory`, `add-filesystem-scan`, `add-usage-visualization` (home cards and drive view).
