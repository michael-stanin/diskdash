# diskdash

A local dashboard for the drives in one Windows PC. It shows what is on each drive, where the space goes, and what you could remove or move. It is **strictly read-only**: diskdash finds and explains, and you carry out every step yourself.

## Start

Prerequisites (the script checks them and tells you how to install anything missing; it never downloads anything itself):

- Rust stable with the MSVC toolchain (`winget install Rustlang.Rustup`) and the Visual C++ build tools
- Node.js 20 or newer

```bash
.\start.cmd
```

`start.cmd` runs `start.ps1` with PowerShell's script policy bypassed for that one script, so it works on a default Windows install without changing any system setting. (If you have already allowed scripts, `.\start.ps1` works too.)

This builds the UI and the server when they are out of date, starts `diskdash.exe` on <http://127.0.0.1:8765> and opens your browser. Press Ctrl+C to stop. Options: `-Port 9000`, `-NoBrowser`.

The release build is a single `target\release\diskdash.exe` with the UI embedded; you can copy it anywhere and run it.

## Share it (portable zip)

```bash
cargo xtask package
```

Builds the UI and release binaries, smoke-tests the packaged exe, and writes `dist\diskdash-<version>-windows-x64.zip` plus a `.sha256`. Friends unzip it and double-click `Start diskdash.cmd`: no install, nothing in the background, nothing leaves their PC. It needs `frontend\node_modules` (run `npm ci` once). The files are not code-signed, so SmartScreen may warn; the zip's README explains.

## Develop

```bash
.\start.cmd -Dev
```

Runs the backend with `cargo run -- --dev` and the Vite dev server with hot reload on <http://127.0.0.1:5173>.

| Check | Command |
|---|---|
| Rust tests (also regenerates `frontend/src/api/types`) | `cargo test --workspace` |
| Lints incl. the read-only list | `cargo clippy --workspace --all-targets -- -D warnings` |
| Win32 read-only guard | `cargo xtask readonly-check` |
| No HTTP client in the server | `cargo xtask no-http-client` |
| UI unit tests / lint / types | `npm test`, `npm run lint`, `npm run typecheck` (in `frontend/`) |
| No external URLs in the bundle | `npm run build && npm run check:bundle` |
| Smoke test, both themes + contrast | `npx playwright test` (uses the installed Microsoft Edge) |

Specs and plans live in `openspec/` (OpenSpec). The project context and safety rules are in `openspec/config.yaml`.

## Where data lives

Everything diskdash stores is under `%LOCALAPPDATA%\diskdash`: the SQLite database (`diskdash.db`), scan snapshots and rotating logs (`logs\`). Set `DISKDASH_DATA_DIR` to use another folder (tests do this).

## Security and read-only model

- **Localhost only.** The server binds to `127.0.0.1` and refuses any other address. Requests with a foreign `Host` header get 421, which blocks DNS rebinding.
- **No cross-site requests.** Every state-changing request needs a per-process token that only the page served by diskdash can read, plus a same-origin `Origin`.
- **No network.** The server contains no HTTP client (checked in CI), and the UI bundle loads nothing from the internet, not even fonts (checked in CI).
- **Read-only by construction.** Only one module (`diskdash_store::own_files`) can write files, and only inside the data folder. Clippy rejects `std::fs` mutators everywhere else, and `cargo xtask readonly-check` rejects Win32 APIs that delete, move, rename, link or change files, ACLs or the registry.
- **Reduced mode.** diskdash runs without admin rights. Features that need admin (disk health counters, the fast MFT scan) show as unavailable; the web server never elevates itself.

## Design system

- Tokens (all colours, light and dark): `frontend/src/styles/tokens.css`. Components must use token-backed Tailwind classes; ESLint rejects colour literals.
- Shared components: `frontend/src/components/` (stat card, size display, fill bar, badges, empty/error states, virtualised data table, toasts).
- See every component in both themes at <http://127.0.0.1:8765/dev/components>.
- Charts read the same tokens at runtime (`frontend/src/lib/chart-theme.ts`).
- Fonts: Geist and Geist Mono, bundled locally.
