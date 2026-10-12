## Context

This is a new repository on a Windows 10 machine with Node, git and Python 3.12 installed. Rust is not installed yet. Python is no longer needed. The owner chose a **Rust backend with a web UI**: one executable, no cross-language boundary. The app is strictly read-only, but it still runs a local web server. A browser page on any site can send requests to `127.0.0.1`, so "it's only on localhost" isn't enough on its own. The owner also asked for a dashboard that "looks modern and good", so the design system is part of the foundation, not a later polish pass.

## Goals / Non-Goals

**Goals:**
- A skeleton where every later phase only adds routes, job kinds, migrations, crates/modules and pages.
- The security boundary (loopback, Host allow-list, CSRF) is enforced in middleware and tested.
- The read-only rule is enforced by the build, not by convention.
- One design system that makes every later screen look consistent and polished by default.
- Release = a single `diskdash.exe`.

**Non-Goals:**
- A desktop shell (Tauri/Electron). The owner chose the browser UI. Tauri could wrap the same frontend and backend later.
- Authentication or multiple users.
- Any scanning or drive logic.

## Decisions

### D1. Single Rust binary, frontend embedded
axum serves the API and the Vite build, which is embedded with `rust-embed`. Release builds are self-contained. In `-Dev` mode, Vite (HMR) proxies `/api` to the running binary.
- *Alternative: Python/FastAPI with a Rust scanner extension.* The owner rejected the mixed stack.
- *Alternative: all Python.* Simpler to build, but scans and the MFT reader would be slower, and the owner prefers Rust.
- *Alternative: Tauri.* A native window with no localhost surface, but it adds a WebView2 dependency and a packaging step. It is easy to add later because the backend is already Rust.

### D2. Workspace layout
- `diskdash` is the binary: HTTP, jobs, settings and wiring.
- `diskdash-store` holds rusqlite access, migrations, and the single allow-listed `own_files` writer.
- `diskdash-core` holds the scanner, which has no HTTP dependency.
- `diskdash-inventory` and `diskdash-analysis` are added by later phases.
- `test-fixtures` builds fixture trees for integration tests.

Separate crates keep compile times reasonable, keep `diskdash-core` testable without a server, and let clippy's read-only config apply per crate.

### D3. SSE rather than WebSocket for progress
Progress only flows from server to browser. axum's `Sse` passes through the same tower middleware, so the Host check applies automatically. SSE reconnects with `Last-Event-ID` for free. Commands such as start and cancel are normal POSTs, so they pass through CSRF checks.
- *Alternative: WebSocket.* It is bidirectional, which we don't need, and its upgrade bypasses CORS, so it would need its own Origin check.

### D4. Job runner on worker threads, panics caught
A `JobRunner` owns a bounded pool of OS threads (default: CPU count capped at 4, plus per-disk scheduling inside scan jobs). Each job runs inside `catch_unwind` and receives a context with:
- a progress sender (a channel, throttled to 4 Hz);
- a cancel token (`AtomicBool`);
- its acquired path locks.

The runner persists state changes to `jobs` and `job_events` and fans events out to SSE subscribers through a `tokio::sync::broadcast` channel, plus a 1000-event ring buffer for `Last-Event-ID` replay.
- *Alternative: separate worker processes.* They give stronger isolation, but in Rust a panic can be caught and memory safety removes most crash classes. Processes would add IPC for no benefit. The elevated helper (phase 11) is the only separate process, because it needs a different token.
- *Alternative: running blocking work on tokio's `spawn_blocking` pool directly.* That pool has no queueing semantics, priorities, or per-path locks. We still use it for short blocking calls (PowerShell inventory).

### D5. Security middleware stack
In order:
1. Host allow-list (421).
2. For unsafe methods: an Origin check plus a constant-time comparison of `X-Diskdash-Token` (403).
3. `tracing` request spans with token redaction.

The token is 32 random bytes from the OS RNG, generated at start. It is injected into `index.html` as a `<meta>` tag when the page is served, so a page on another origin can't read it. CORS is not enabled.
- *Alternative: cookie-based CSRF.* Cookies on `localhost` are shared across ports. A header token is simpler.

### D6. Read-only guard: clippy + Win32 deny-list + one writer module
- `clippy.toml` sets `disallowed-methods` for `std::fs` mutators and `File::create`/`OpenOptions`.
- A small `xtask` scans sources for Win32 mutator names (the `windows` crate exposes them as plain functions).
- The only exception is `diskdash-store::own_files`, carrying an `#[allow]` with a comment. It canonicalises the target path and refuses anything outside the data dir or a user-picked export path.
- `test-fixtures` is excluded, because building fixtures needs links, ACLs and files.

This makes safety rule 3 a build failure, not a review comment.

### D7. Data directory and migrations
- The data lives in `%LOCALAPPDATA%\diskdash` (per user, not roamed). `DISKDASH_DATA_DIR` overrides it for tests.
- Migrations are numbered `.sql` files, embedded with `include_str!` and applied by a ~60-line runner in one transaction each.
- *Alternative: `refinery` or `sqlx` migrations.* These work, but sqlx would pull in async SQLite and a second driver. rusqlite + a tiny runner is enough, and the scan tables need hand-tuned SQL anyway.

### D8. UI stack and design system
- **Tailwind CSS + shadcn/ui** (Radix primitives, copied into the repo rather than added as a dependency, so we own and style them). This gives accessible, modern components (dialogs, menus, tooltips, tabs, command palette) without a heavy UI kit's look.
- **Tokens** are CSS variables (`--background`, `--card`, `--primary`, `--muted`, `--chart-1..10`, `--risk-safe/review/expert`, `--fill-normal/caution/warning`). Tailwind maps to them, and an ECharts theme object is built from `getComputedStyle` at runtime, so charts follow the theme. The dark theme is designed with its own neutral surfaces and accent tuning.
- **Geist Variable (with Geist Mono for paths)** comes via `@fontsource-variable/geist`, which is bundled with no Google Fonts call. Tabular numerals (`font-variant-numeric: tabular-nums`) are used for all figures.
- **Icons:** lucide-react, tree-shaken.
- **Visual direction:** a calm neutral base, one accent colour, and data colour reserved for meaning (category palette, risk, fill states). Cards have subtle borders instead of heavy shadows, the information density is generous, and big numbers are reserved for the figures that matter (free space, potential savings).
- *Alternative: MUI or Ant Design.* They're complete, but they look generic and are heavier to theme.
- *Alternative: plain CSS modules.* Too much hand-rolling to reach a polished result.

### D9. Generated API types
`ts-rs` exports TypeScript definitions for every API type into `frontend/src/api/types/`. CI regenerates them and fails on a diff, so the frontend and backend can't drift.

### D10. CI on `windows-latest`
- Rust: fmt, clippy `-D warnings` (including disallowed methods), test, the Win32 deny-list xtask, and a `cargo tree` check that no HTTP client crate is present.
- Frontend: eslint (including no colour literals), tsc, vitest, build, and a bundle scan for external URLs.
- e2e: Playwright starts the release binary and opens the shell and component gallery in both themes, with axe accessibility checks. Screenshots are uploaded as artifacts for visual review.

## Risks / Trade-offs

- [Rust compile times slow down iteration] → Workspace split, `cargo check` in dev, and Vite HMR for UI work, so most UI iteration doesn't rebuild Rust.
- [Another local process can still call the API, since any local process can reach loopback] → Accepted. That process already has the user's file permissions, and the API can't change files anyway.
- [Browsers allow 6 SSE connections per origin over HTTP/1.1] → Use a single multiplexed stream. The UI opens one EventSource.
- [shadcn components copied into the repo drift from upstream] → Accepted. We own them, and they are few and stable.
- [Rust toolchain install friction (MSVC Build Tools)] → The README walks through rustup and the VS Build Tools "Desktop development with C++" workload. `start.ps1` detects both.

## Migration Plan

Not applicable. This is a new repository.

## Open Questions

None. Resolved: Rust backend + browser UI, no Python, port 8765, alerts in-app only, elevation via a separate read-only helper (phase 11).
