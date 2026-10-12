## 1. Path protection

- [ ] 1.1 `protection::is_suggestable()` with built-in paths, system files from the PagingFiles registry key, user exclusions and protected paths; `cargo test` (Windows temp allowed, System32/Program Files blocked, component-boundary matching)
- [ ] 1.2 Apply the filter in the rules, stale, duplicates and tiering producers; tests showing excluded paths disappear from each
- [ ] 1.3 Settings API and UI for exclusions and protected paths, plus re-filtering of existing plan items. States: list skeleton, "no exclusions yet" empty state, validation error. Integration and component tests

## 2. Plans

- [ ] 2.1 Catalog migration: `plans`, `plan_items`, `plan_verifications`
- [ ] 2.2 Plan CRUD and add-item from suggestion ids with baseline capture, duplicate-add guard, protection check (422) and 404; integration tests
- [ ] 2.3 Instruction templates (YAML) per kind (manual-cleanup, system-setting, tool, manual-move) and rendering; unit tests
- [ ] 2.4 Status tracking and totals by status and drive; integration tests
- [ ] 2.5 CSV/TXT export (escaping, no script extensions, atomic write); tests
- [ ] 2.6 Test that enumerates the routes and asserts that no file-operation or command-execution endpoint exists

## 3. Verification

- [ ] 3.1 Post-rescan verification job (removed/present/partial/grown/moved/unverified, freed bytes, volume delta); unit tests on fixture snapshot pairs; assert no filesystem access

## 4. UI

- [ ] 4.1 "Add to plan" buttons on Recommendations, candidates, duplicates, stale and tiering; component tests
- [ ] 4.2 Plans page: checklist, instructions with copy and Open in Explorer, totals (tabular numerals, exact bytes on hover), export, verification results. States: skeleton; "No plans yet — add suggestions from Recommendations" empty state; error with retry. Component tests and a Playwright smoke test in light and dark
