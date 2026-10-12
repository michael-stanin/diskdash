## Context

Rules must be easy to extend without code. They must also be evaluated quickly against snapshots with 1M+ entries, and they must not touch the filesystem beyond a few metadata calls. Some candidates depend on context ("node_modules only when a sibling package.json exists"). Others are not real files at all: Recycle Bin contents, shadow storage, and WinSxS reclaimable space.

## Goals / Non-Goals

**Goals:** a declarative rule format, a deterministic evaluator over snapshot rows, bounded memory, per-rule error isolation, and savings figures that never double-count.

**Non-Goals:** executing anything. The app never deletes files and never runs suggested commands; the user acts manually (checklists are `cleanup-plans`). Also out of scope: running elevated diagnostic commands, or fetching rule updates over the network.

## Decisions

### D1. YAML deserialised into serde types, then validated
Rules are YAML (readable, comment-friendly). They are deserialised with a maintained serde YAML crate into `#[serde(deny_unknown_fields)]` Rust structs, then semantically validated (risk enum, kind, instructions non-empty, glob syntax via `globset`). `serde_yaml` is archived, so a maintained fork or an alternative is chosen at implementation time, behind a small `parse_rules(&str)` boundary so it can be swapped. Errors carry the file, line/column (from the deserialiser's location), rule index or id, and field. Built-in rules are embedded with `include_str!` at build time.
- *Alternative: JSON.* It lost because it has no comments, and users would write rules less easily.
- *Alternative: scripted rules (Lua/WASM plugins).* They lost because they allow arbitrary code, which is unsafe to load from a user directory, and add complexity.

### D2. Path matching compiled to snapshot queries
Each `paths` entry is expanded at evaluation time: `%TEMP%`, `%LOCALAPPDATA%`, `%USERPROFILE%`, `%WINDIR%`, `%SystemDrive%`, and known folders such as Downloads via `SHGetKnownFolderPath`. It is then normalised (case-folded, `\\?\` stripped) and resolved to a snapshot entry by walking `parent_id/name`. `globs` (`**/node_modules`, `*.dmp`) are matched against the entries of the subtree. Root-anchored rules run as a cheap subtree aggregate. Unanchored globs like `**/node_modules` are evaluated in one streaming pass over directory entries by name, using an index on `name_lower` in the snapshot DB. Globs are compiled once with `globset`.
- *Alternative: re-walk the filesystem.* It lost because it is slow, it disagrees with the snapshot, and it breaks the read-only, no-extra-IO guarantee.

### D3. Conditions as a small fixed vocabulary
The supported conditions are `sibling_exists: <name>` (package.json, Cargo.toml), `child_exists: <name>` (pyvenv.cfg), `min_age_days` (based on mtime, or atime when `stale-data-analysis` says atime is reliable), `min_size`, and `extracted_sibling` (the Downloads heuristic: an archive `X.zip` with a sibling directory `X` whose total size is at least 50% of the archive size). All are resolved from snapshot rows.
- *Alternative: an expression language.* It lost because it is over-engineered and harder to validate.

### D4. Virtual candidates for non-file items
The Recycle Bin (`SHQueryRecycleBinW` per drive root), hiberfil/pagefile/swapfile (metadata only via `GetFileAttributesExW`; they are never opened) and shadow storage (needs admin; the size is filled in only if the phase-11 read-only helper provides it, otherwise "not measured" with manual `vssadmin list shadowstorage` instructions) produce *virtual* candidates. The Recycle Bin is `manual-cleanup` ("empty it from Explorer"); the others are `system-setting`. WinSxS gets a `system-setting` entry with instructions. Its true reclaimable size is only available via `DISM /AnalyzeComponentStore`, which needs admin and is suggested to the user, never run.

### D5. No double counting
When candidates from different rules overlap (for example `%TEMP%` and `*.tmp` older than N days inside `%TEMP%`), the most specific *ancestor* match wins for savings totals. Nested matches are listed under it but are not summed twice. Savings are computed per (drive, risk) on allocated size.

### D6. Results stored in the snapshot DB
`rule_matches(rule_id, entry_id | virtual_key, logical, allocated, risk, kind)` lives inside the per-snapshot SQLite file, so it is removed together with the snapshot. Per-drive savings totals are cached in the catalog for the home cards. A job on a worker thread fills the table after each scan, or on demand when rules change. Memory bound: streaming inserts in transactions of 10k rows; the matcher holds only the current directory frame.

## Risks / Trade-offs

- A heuristic false positive (for example a Downloads archive that is *not* really extracted) → Downloads triage is `risk: Review`, and the explanation states the heuristic.
- Path expansion depends on the user profile → expansion uses the server's own (non-elevated, interactive) user environment and known folders, and the expanded paths are shown on the Rules page.
- Browser/app cache paths change between versions → rules are data, so users can patch them in their own rule dir.

## Open Questions

None for the product owner. Implementation-time: choose the maintained serde YAML crate (D1). Accepted defaults: logs/tmp older than 30 days, Prefetch at Expert risk, and built-ins can be disabled via `disabled: [ids]`.
