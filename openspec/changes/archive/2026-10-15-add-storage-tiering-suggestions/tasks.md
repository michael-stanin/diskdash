## 1. Analysis (crates/diskdash-analysis::tiering)

- [x] 1.1 Cold-on-SSD finder (outermost qualifying folder, time basis, min size, internal-only, protection/exclusion filter); `cargo test` on a test-fixtures snapshot with a fake media map
- [x] 1.2 Hot-on-HDD heuristic (share of recently modified bytes/files); unit tests
- [x] 1.3 Steam (`libraryfolders.vdf`, `appmanifest_*.acf`) and Epic manifest parsers, read-only; tests with sample files including a malformed one
- [x] 1.4 App-install flagging via installed-apps InstallLocation; unit test

## 2. Targeting, projection and instructions

- [x] 2.1 Greedy target assignment (internal drives, % and absolute limit plus headroom, same-physical-disk exclusion, `no_target_with_space`); unit tests
- [x] 2.2 Projected free-space calculator for the planned set; unit tests
- [x] 2.3 Instruction templates (launcher move, reinstall, robocopy + verify/delete via Explorer) with path quoting; unit tests

## 3. API and UI

- [x] 3.1 Endpoints: suggestions (with reasons and instructions), plan/unplan, projections, add to cleanup plan; ts-rs types; integration tests asserting no filesystem writes or process launches
- [x] 3.2 Tiering page: suggestion lists, planned toggles, before/after free-space bars, copy buttons, Add to plan. States: skeleton; "No tiering suggestions" empty state with reason; error with retry. Component tests
