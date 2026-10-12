## 1. Repository scaffolding

- [x] 1.1 Create the Cargo workspace (`crates/diskdash`, `diskdash-store`, `diskdash-core`, `test-fixtures`), `rust-toolchain.toml` (stable, MSVC), `.gitignore`, `.editorconfig`, and a README stub with prerequisites; verify that `cargo build` and `cargo test` pass on the empty workspace
- [x] 1.2 Initialise the frontend (Vite React-TS strict, ESLint, Vitest, Tailwind, shadcn/ui init, lucide-react, Geist via @fontsource-variable); verify that `npm run build` and `npm test` pass
- [x] 1.3 Add `clippy.toml` with `disallowed-methods` for the std::fs mutators, plus the `xtask readonly-check` Win32 deny-list scan; test that a planted `std::fs::remove_file` and a planted `DeleteFileW` each fail the check

## 2. Persistence

- [x] 2.1 Implement data-dir resolution (`%LOCALAPPDATA%\diskdash`, `DISKDASH_DATA_DIR` override), the `own_files` allow-listed writer, and the connection factory (WAL, foreign_keys, busy_timeout); test the pragmas and that `own_files` refuses paths outside the data dir
- [x] 2.2 Implement the embedded migration runner (ordered, transaction per migration, `schema_version`, downgrade refusal); test upgrade, rollback-on-failure and downgrade refusal
- [x] 2.3 Write `0001_base.sql` (`schema_version`, `settings`, `jobs`, `job_events`); test that the schema exists after first start

## 3. App runtime and security boundary

- [x] 3.1 Implement the axum app builder and a config loader that refuses non-loopback bind addresses; test that `0.0.0.0` is rejected and that a busy port gives a clear error
- [x] 3.2 Add Host allow-list middleware (421); test good and bad Host headers
- [x] 3.3 Add token generation, `<meta>` injection into the embedded `index.html`, and Origin + token middleware for unsafe methods (403); test missing token, wrong token, cross-origin and GET passthrough
- [x] 3.4 Add `tracing` with a rolling file appender (10 MiB × 5) and token redaction; test the redaction
- [x] 3.5 Implement elevation detection (token elevation via the `windows` crate) and `GET /api/v1/system` with `reduced_features`; unit test with the detector behind a trait
- [x] 3.6 Add typed settings (serde, with bounds validation, defaults incl. 10%/20 GiB, chart cap 20, system-volume hard-link detection) and `GET`/`PUT /api/v1/settings` (422 with field errors); tests for defaults and out-of-range values
- [x] 3.7 Embed the frontend (rust-embed) with SPA fallback; test that a client route returns `index.html` and that assets have correct content types
- [x] 3.8 Add ts-rs export of the API types to `frontend/src/api/types`, with a CI diff check

## 4. Background jobs

- [x] 4.1 Implement the JobRunner (bounded thread pool, FIFO queue, `catch_unwind`, state persisted to `jobs`, `interrupted` marking on startup); test the transitions and that a panicking job fails while the server keeps serving
- [x] 4.2 Implement progress (done/total/unit/phase/ETA, throttled to 4 Hz) and the `noop-sleep` test job; test the throttling
- [x] 4.3 Implement cancellation (AtomicBool checked at least once per second); test that the cancelled state is reached within 2 s
- [x] 4.4 Implement path-scoped locks (normalised, case-insensitive, ancestor/descendant overlap); test overlapping vs disjoint paths
- [x] 4.5 Implement the SSE endpoint `/api/v1/events` (broadcast channel, event ids, 1000-event replay buffer for `Last-Event-ID`); test replay after reconnect

## 5. UI foundation

- [x] 5.1 Write the design tokens (light + dark CSS variables: surfaces, text, accent, borders, chart palette, risk and fill states), the Tailwind mapping, the type scale with tabular numerals, and the ESLint rule banning colour literals in components
- [x] 5.2 Build the theme provider (light/dark/system, persisted, applied before first paint by an inline script) and the ECharts theme built from tokens; Vitest for the theme resolution
- [x] 5.3 Build the app shell: collapsible sidebar (Home, Drives, Recommendations, Plans, Settings), header (title/breadcrumb slot, scan indicator, reduced-mode badge with tooltip, connection status, theme toggle), in-app alert area; verify that 1280×720 has no horizontal scroll
- [x] 5.4 Build the shared components (stat card, size display with exact-bytes tooltip, relative time, fill bar, media badge, risk badge, skeletons, empty state, error state with retry, in-app toast) with Vitest for the size formatting rules
- [x] 5.5 Build the virtualised data table (TanStack Table + Virtual: sort, filter, column chooser, row actions slot); Vitest with 100k rows rendering within budget
- [x] 5.6 Add the API client (token from `<meta>` on unsafe methods), TanStack Query setup, an SSE hook with reconnect, and the connection indicator; Vitest with mocked fetch/EventSource
- [x] 5.7 Build the Settings page (all settings, field-level validation errors) and the placeholder pages with designed empty states
- [x] 5.8 Add the dev-only component gallery `/dev/components` showing each component in both themes

## 6. Start script and CI

- [x] 6.1 Write `start.ps1`: prerequisite checks (cargo + MSVC target, Node 20+) with install hints and no downloads, `npm ci` + build when stale, `cargo build --release` when stale, start the server, wait for health, open the browser; `-Dev` (Vite HMR + `cargo run`) and `-Port` switches
- [x] 6.2 Manually verify a fresh clone with `.\start.ps1`: the browser opens, health is green, both themes look right; Ctrl+C stops cleanly
- [x] 6.3 Add the CI workflow on `windows-latest`: fmt, clippy, test, readonly-check, `cargo tree` HTTP-client check, eslint, tsc, vitest, build, the bundle external-URL scan, ts-rs diff, Playwright shell + gallery smoke in both themes with axe and screenshot artifacts
- [x] 6.4 Update the README: how to start, dev mode, data location, the security and read-only model in five bullets, and the design-system pointers
