## Why

A directory walk of a 1M+ file NTFS volume takes minutes, and much longer on HDDs. Reading the Master File Table directly, as WizTree does, takes seconds. The USN change journal then makes a "Quick rescan" incremental. Both need admin rights. The server never runs elevated, so this capability adds a separate, strictly read-only elevated helper.

## What Changes

- **MFT reader** in `crates/diskdash-core`, module `ntfs`: opens `\\.\X:` read-only (`GENERIC_READ`, share read/write) and reads `$MFT` via its data runs. It parses FILE records: fixups, `$STANDARD_INFORMATION`, `$FILE_NAME` (all links), `$DATA` sizes (including non-resident, sparse and compressed allocated size) and `$ATTRIBUTE_LIST`. It produces the same entry model as the walk for the snapshot writer. Link counts come from the MFT, so **hard links are exact on every MFT-scanned volume**, regardless of the walk's per-drive hard-link toggle. `FSCTL_ENUM_USN_DATA` was evaluated and rejected because it returns no sizes.
- **Elevated read-only helper** `crates/diskdash-helper`: a separate small executable. The user starts it explicitly through a UAC prompt ("Enable elevated features"). Over a named pipe restricted to the current user and authenticated with a per-launch secret, it exposes only a closed set of read-only operations: volume info, MFT scan (streamed record batches), USN journal read, and the elevated reads other capabilities need (SMART reliability counters for `drive-health`, Prefetch last-run times for `installed-apps`). Both ends are Rust. The helper writes nothing, not even the snapshot: the server writes it.
- **Manual "Quick rescan"**: reads the USN journal from the last stored USN and re-reads only changed records. If the journal id changed, the journal wrapped or is inactive, or too much changed → full MFT scan. There is no continuous monitoring.
- **Automatic fallback** to the directory walk, with the reason shown: helper not running, non-NTFS (ReFS/exFAT), folder target, volume open failure, or parse error rate over 1%.
- **Equivalence tests**: MFT totals must equal walk totals on a fixture VHDX created by the test (admin-only, opt-in).

## Capabilities

### New Capabilities
- `fast-ntfs-scan`: MFT-based full scan in `diskdash-core::ntfs`, manual USN quick rescan, fallback rules, equivalence with the directory walk, and the strictly read-only elevated helper `diskdash-helper`.

### Modified Capabilities
None

## Impact

- `crates/diskdash-core` (`ntfs` module), `crates/diskdash-helper` (new binary), `crates/diskdash` (helper client, scan-mode selection, Quick rescan route), `crates/diskdash-store` (catalog `volume_usn_state`; copy-on-write snapshot creation), `crates/test-fixtures` (VHDX fixture builder, admin-only).
- CI: build the helper binary; `cargo test` for parser fixtures; an opt-in admin job.
- Frontend: scan dialog shows "fast scan (MFT)" / "directory walk" and the fallback reason; an "Enable elevated features" button; a Quick rescan button. All use shared components with designed states.
- Safety rules touched: **1** (the helper uses a current-user-ACL'd named pipe, no TCP; the server stays non-elevated), **3** (the volume handle is read-only; nothing is written to the volume or files; the USN journal is never created, resized or deleted; the helper has no write, path-I/O or exec operations), **6** (VHDX tests create and remove their own images).

## Depends on

`add-project-skeleton` (elevation detection, job runner), `add-filesystem-scan` (diskdash-core entry model, snapshot writer, walk fallback, hard-link rules), `add-history-and-trends` (quick rescans produce snapshots that feed diffs and retention).
