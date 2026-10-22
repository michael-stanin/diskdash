## What and why

<!-- What does this change, and why? Link the issue if there is one. -->

## Safety checklist

- [ ] It adds no way to write, delete, move or run anything of the user's (reports and our own data directory excepted)
- [ ] It adds no network access, telemetry, external URL or OS notification/shortcut/scheduler
- [ ] Nothing that reads from disk can panic on damaged input, and there is a test with damaged input
- [ ] Suggestions are text only; protected locations stay protected

## Checks

- [ ] `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo xtask readonly-check`, `no-http-client` and `no-os-integration`
- [ ] `cargo test --workspace` (generated types in `frontend/src/api/types` committed)
- [ ] Frontend: `npm run lint`, `typecheck`, `test`, `build && check:bundle`, Playwright if the UI changed
- [ ] Tests added or updated, including loading/empty/error states for UI changes
- [ ] `CHANGELOG.md` updated under **Unreleased**

## Screenshots (UI changes, light and dark)
