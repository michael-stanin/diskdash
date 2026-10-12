## Why

Every later phase needs the same foundation:
- a backend that only listens on localhost;
- a polished frontend shell that talks to it;
- a SQLite database with migrations;
- a way to run long jobs off the request path;
- one command to start it all.

Putting these in place first, together with the safety guards (Host/CSRF checks, no outbound network, the read-only check in CI) and the design system, means no feature phase has to invent them, none can skip them, and every screen looks like part of the same product.

## What Changes

- Create a Cargo workspace:
  - `crates/diskdash` (server binary)
  - `crates/diskdash-store` (SQLite + migrations)
  - `crates/diskdash-core` (empty scanner crate for phase 3)
  - `crates/test-fixtures`

  Also add `frontend/` (React + Vite + TypeScript), `rules/`, `start.ps1`, `README.md`.
- Add an axum server that binds to `127.0.0.1` only. It serves the frontend embedded in the executable and exposes `/api/v1/health` and `/api/v1/system` (app version, whether it is running elevated, OS build).
- Add tower middleware: a Host-header allow-list (DNS-rebinding defence), and a CSRF token plus Origin check on every state-changing method.
- Add typed settings persisted in SQLite. Defaults: low-space 10% / 20 GiB floor, stale 12 months, HDD/SSD scan concurrency, chart child cap 20.
- Add the SQLite layer (rusqlite, WAL): embedded numbered SQL migrations and the base tables (`schema_version`, `settings`, `jobs`, `job_events`).
- Add a job runner: jobs run on worker threads, report progress, can be cancelled, contain panics, take path-scoped locks, and stream events over SSE.
- Add local rotating logs (`tracing`) under `%LOCALAPPDATA%\diskdash\logs`.
- Add the **UI foundation**:
  - design tokens for light and dark themes, using Tailwind + shadcn/ui and bundled Geist;
  - the app shell (sidebar navigation, header with reduced-mode badge and connection status, alert area);
  - shared components (stat card, size display, fill bar, skeletons, empty/error states, data table);
  - a theme toggle;
  - TS API types generated from Rust (ts-rs).
- Add `start.ps1`: checks for the Rust toolchain and Node 20+, builds when stale, starts `diskdash.exe`, and opens the browser. `-Dev` runs Vite with HMR and an API proxy.
- Add CI on `windows-latest`:
  - Rust: fmt, clippy `-D warnings`, test
  - frontend: ESLint, tsc, Vitest, build
  - a Playwright smoke test of the shell in both themes
  - a bundle scan for external URLs
  - the **read-only guard**

Safety rules touched: **1** (loopback, Host/CSRF), **2** (no outbound network, self-contained bundle with no web fonts), **3** (read-only guard in CI), **7** (local logs).

## Capabilities

### New Capabilities
- `app-runtime`: Local server lifecycle and security boundary: loopback binding, Host/Origin/CSRF guards, the CI read-only guard, elevation detection, settings, logging, the start script, and serving the embedded frontend.
- `persistence`: SQLite location, WAL configuration, embedded versioned migrations, and the base schema.
- `background-jobs`: Running long operations on worker threads with progress, cancellation, panic containment, path locks and SSE delivery.
- `ui-foundation`: Design system and app shell: tokens, themes, typography, shared components, state patterns (loading/empty/error), accessibility and motion rules that every screen uses.

### Modified Capabilities
None. This is the first change.

## Impact

- New code: `crates/`, `frontend/`, `start.ps1`, `.github/workflows/ci.yml`, `clippy.toml`.
- Rust dependencies: tokio, axum, tower, serde, serde_json, rusqlite (bundled), tracing, tracing-appender, rust-embed, ts-rs, thiserror/anyhow, windows.
- Frontend dependencies: react, react-router, @tanstack/react-query, tailwindcss, shadcn/ui (Radix), lucide-react, @fontsource-variable/geist, echarts; dev: vite, vitest, eslint, playwright.
- Developer prerequisites: the Rust stable toolchain (rustup, MSVC build tools) and Node 20+. **Rust is not installed on this machine yet.** `start.ps1` prints install instructions instead of installing anything itself. Python is no longer needed.
- Creates `%LOCALAPPDATA%\diskdash\` (database and logs) on first run.

## Depends on

Nothing.
