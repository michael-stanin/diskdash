# Contributing to diskdash

Thanks for helping. diskdash touches people's disks, so a few rules matter more than usual. Please read the safety rules before you write code.

## The safety rules (every change must keep them)

1. **Read-only.** diskdash must never create, modify, move or delete anything of the user's. The only files it writes are its own data directory (through `diskdash-store`'s `OwnFiles`) and a report the user explicitly exports (`write_report_file`: new files only, allow-listed extensions). Do not add other write paths. Clippy's `disallowed-methods` list and `cargo xtask readonly-check` will fail the build if you try, and that is intentional.
2. **No network.** The server binds to `127.0.0.1` only and makes no outbound connections. Do not add an HTTP client, telemetry, update checks or CDN/font/script URLs. `cargo xtask no-http-client` and `npm run check:bundle` enforce this.
3. **No OS integration.** No notifications, tray icons, shortcuts, services, scheduled tasks or background processes. `cargo xtask no-os-integration` enforces this.
4. **The server never runs elevated.** Anything that needs administrator rights goes through `diskdash-helper`, which only exposes read-only operations that take IDs the helper listed itself, never paths. Do not add an operation that writes, deletes, runs a program or takes a path.
5. **Suggest, never act.** Cleanup, tiering, duplicate and plan features show text instructions. They do not execute them. Protected locations must stay protected.
6. **Honest numbers.** Show sizes as measured, say when something is unknown or an estimate, and never present a forecast or a saving as a promise.

If a change needs to bend one of these, open an issue to discuss it first.

## Getting set up

You need Windows 10/11 x64, Rust stable (MSVC) with the Visual C++ build tools, and Node.js 20 or newer.

```bash
git clone https://github.com/michael-stanin/diskdash.git
cd diskdash/frontend && npm ci && cd ..
.\start.cmd -Dev      # backend + Vite dev server with hot reload on http://127.0.0.1:5173
```

## Before you open a pull request

Run what CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask readonly-check
cargo xtask no-http-client
cargo xtask no-os-integration
cargo test --workspace          # also regenerates frontend/src/api/types: commit any changes

cd frontend
npm run lint
npm run typecheck
npm test
npm run build && npm run check:bundle
npx playwright test             # uses your installed Microsoft Edge
```

Stop any running `diskdash.exe` from the build folder before `cargo test`, otherwise Windows blocks the linker.

### Tests we expect

- Logic goes in `diskdash-analysis` or `diskdash-core` with unit tests; HTTP behaviour gets an integration test in `crates/diskdash/tests/`.
- Anything that parses data from disk must be bounds-checked and covered by tests with damaged input.
- UI changes need component tests for loading, empty, error and success states, and must pass the Playwright contrast check in both themes.
- Use colours from the design tokens (`frontend/src/styles/tokens.css`); a lint rule rejects colour literals.
- Real-volume tests need administrator rights and are opt-in (`DISKDASH_ADMIN_TESTS=1`); they create and remove their own VHDX and never touch real drives.

## How changes are planned

The project uses [OpenSpec](https://github.com/Fission-AI/OpenSpec). Requirements live in `openspec/specs/`; each feature was proposed as a change with a design, tasks and spec deltas, then archived. For anything larger than a small fix, describe the behaviour you want in an issue first. For bigger features a short OpenSpec proposal (`openspec/changes/<name>/`) is welcome, and the maintainer will help with it.

## Commit and PR conventions

- Small, focused pull requests with a clear description of *what* and *why*.
- Imperative commit subjects ("Add duplicate keep rule for newest file").
- Do not commit scan databases, `target/`, `frontend/dist/` or anything from `%LOCALAPPDATA%\diskdash`. Do not commit data from your own machine (paths with your user name, drive serial numbers).

## Reporting bugs and security issues

Use the issue templates for bugs and ideas. For anything that could affect data safety or privacy, **do not open a public issue**; follow [SECURITY.md](SECURITY.md).

## License

By contributing you agree that your contribution is licensed under the [MIT License](LICENSE).
