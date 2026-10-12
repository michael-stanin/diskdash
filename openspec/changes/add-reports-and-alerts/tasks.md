## 1. Reports (crates/diskdash::reports)

- [ ] 1.1 Report model assembler (sections including plans/tiering, schema version, "not available" handling); `cargo test`
- [ ] 1.2 JSON (serde) and CSV-in-ZIP (`csv`, `zip`) writers; download response plus atomic save-to-folder; tests including the unwritable target
- [ ] 1.3 Self-contained HTML via askama (inline CSS, Rust-generated inline SVG charts with the shared category palette, row caps, instructions as text); test asserting no `http(s)://` references and no script extensions
- [ ] 1.4 Export route (POST + CSRF, ts-rs types) and Export dialog. States: section availability, generation progress, "nothing scanned yet" empty state, error with retry. Component tests

## 2. Alerts

- [ ] 2.1 Catalog migration `alert_state`, `alerts`; low-space (% OR 20 GiB) and growth evaluators with cooldown, hysteresis and dismiss (crates/diskdash-analysis::alerts); `cargo test`
- [ ] 2.2 Hook evaluation into inventory refresh and manual scan/quick-rescan completion; SSE `alert` events; integration test, plus a check that the code has no OS-notification, shortcut or scheduler dependencies
- [ ] 2.3 Header bell with count, Alerts page (active/resolved, filters, "only while running" note) and home/drive-view banners with dismiss. States: skeleton; "All drives look healthy" empty state; error with retry and an unknown bell state. Component tests and a Playwright smoke test in light and dark
