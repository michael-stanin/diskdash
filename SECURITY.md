# Security policy

diskdash runs on your own PC, reads your drives and (optionally) starts a small elevated helper. We take reports about data safety, privacy and privilege seriously.

## Supported versions

Only the latest release receives fixes.

## Reporting a vulnerability

**Please do not open a public issue.** Report it privately:

- On GitHub: **Security → Report a vulnerability** on this repository (private vulnerability reporting).

Include what you found, how to reproduce it, the diskdash version, and your Windows version. You can expect an acknowledgement within a few days. We will agree on a fix and a disclosure date with you, and credit you in the release notes unless you prefer not to be named.

## What counts

Of particular interest, in rough order of severity:

1. **Anything that makes diskdash write, delete, move or execute something it should not**, including through the report-saving path, the HTTP API, or a crafted file or folder name.
2. **Elevation or sandbox problems in `diskdash-helper`**: reaching the helper's pipe as another user, bypassing its secret, getting it to read arbitrary paths, or making it write.
3. **Local web security**: cross-site requests that change state, DNS-rebinding, a bind to anything other than `127.0.0.1`, token leaks.
4. **Parsing bugs** in the NTFS record parser or other on-disk data that cause crashes, hangs or memory problems.
5. **Privacy**: any network connection, telemetry, or data leaving the machine. There should be none.

## Out of scope

- Attacks that need administrator access to the machine already.
- Warnings about the binaries not being code-signed (this is known and documented).
- Findings that need the user to run an untrusted build.

## Design notes for reviewers

- The server never runs elevated; the helper only exposes the closed `Request` set in `crates/diskdash-helper/src/proto.rs`.
- The helper's named pipe has a DACL granting only the launching user's SID, rejects remote clients, allows one instance, and requires a per-launch 256-bit secret compared in constant time.
- `cargo xtask readonly-check`, `no-http-client` and `no-os-integration` guard the build against the most serious regressions.
