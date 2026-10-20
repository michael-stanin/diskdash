## Context

The owner ruled that alerts appear only inside the dashboard: no OS notifications, no Start-menu shortcut, no scheduled scans. Reports must open offline years later, must never contain anything executable, and must be produced by the single Rust binary with no external template files.

## Goals / Non-Goals

**Goals:** clear, non-spammy in-app alerts; offline, self-contained reports; no side effects outside the app's own data and the user-chosen export location.

**Non-Goals:**
- OS toasts or tray icons
- scheduled scans or a background watchdog (not planned)
- emailing reports
- executable exports (scripts)

## Decisions

### D1. Alert evaluation hooks
The server evaluates alerts after (a) every drive-inventory refresh and (b) every completed manual scan or quick rescan. New or changed alerts are pushed to open browser tabs as SSE `alert` events, so the bell count and banners update live. When the server is not running, nothing is evaluated, and the Alerts page says so.
- *Alternative: Windows toast notifications.* Rejected by the owner (no OS notifications).
- *Alternative: a tray process or Task Scheduler checks.* Rejected (no background work outside the app).

### D2. Alert centre UX
- **Header bell:** shows the count of active alerts.
- **Alerts page:** lists active and resolved alerts with filters.
- **Banners:** low-space alerts show a banner on the home page and on the affected drive view, with "View suggestions" (Recommendations filtered to that drive) and "Dismiss". Dismissing hides the banner until the condition clears and recurs, or the cooldown passes.
- **Components:** the skeleton's shared `Alert`/`Banner` and badge components, with severity colours from the tokens.

### D3. De-duplication and cooldown
State is kept in catalog `alert_state(alert_key, last_raised_at, last_value, active, dismissed_at)`, with keys `low_space:<volume_guid>` and `growth:<path_key>`. An alert is not re-raised until the cooldown (24 h) elapses, or the condition clears and recurs. Low space clears only when free space is above both limits plus a hysteresis of 2% / 2 GiB. The growth rate is Δallocated / Δdays between the two latest snapshots of the same root, at least 1 day apart (from history-trends).

### D4. Report templating: askama
HTML reports use `askama` templates, compiled into the binary and type-checked against the Rust report model. CSS is inlined. Charts are generated as inline SVG by small Rust functions (bars, a treemap of the top 20 children, forecast line), using the same category colours as the UI (exported from a shared JSON palette at build time).
- *Alternative: `minijinja`.* It would allow runtime templates, which are not needed. It also has weaker compile-time checking.
- *Alternative: render the React UI to static HTML.* It lost because it needs a JS runtime at export time.

### D5. Safe export writing
Exports are offered as a browser download (`Content-Disposition`) by default. The optional "save to folder" writes atomically (temp file in the same folder, then rename) to a user-chosen directory. Tables are capped at the top 100 rows per section, with a size cap of about 5 MB and a truncation note. Instructions and commands appear as text in `<pre>` blocks. Extensions are only `.html`, `.csv`, `.zip` and `.json`, never `.ps1`, `.bat` or `.cmd`.

## Risks / Trade-offs

- No alerts while the app is closed → stated on the Alerts page. A watchdog is not planned.
- Alert fatigue → cooldown, hysteresis and dismiss.

## Open Questions

None.
