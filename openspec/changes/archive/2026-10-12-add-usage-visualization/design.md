## Context

A snapshot (one SQLite file per scan, owned by `diskdash-store`) of a system drive can hold more than 1M entries. A browser cannot render or hold that tree. ECharts' treemap also becomes unreadable, not just slow, with more than a few dozen rectangles per level. The owner set a cap of **20 children per level plus "Other"** (configurable down to 5) and **20 Top-N rows with "Show more"**. The design system (tokens, Tailwind + shadcn/ui, Geist, app shell) is defined in `add-project-skeleton`. This change consumes it for the analysis views.

## Goals / Non-Goals

**Goals:**
- smooth drill-down on 1M+ entry snapshots with bounded browser memory
- the same numbers in every view
- a logical/allocated toggle
- a polished, consistent look in light and dark

**Non-Goals:**
- comparing snapshots (that is `history-trends`)
- any file operation (the app is strictly read-only; suggestions go to `cleanup-plans`)
- live updates while a scan runs
- redefining global tokens or components

## Decisions

### D1. Lazy per-level loading capped at 20 + "Other"
The request is `GET /api/v1/snapshots/{id}/tree?node=<entry_id>&depth=1..2&limit=20&size_mode=allocated|logical`. The server returns each level's children sorted by the chosen size, capped at `limit`. The default `limit` comes from settings: 20, minimum 5, maximum 20. The remainder is folded into a synthetic `other` node carrying `count`, `logical` and `allocated`. With depth 2 at most 20 + 20×20 = 420 nodes reach the chart. That is far below ECharts canvas limits, and each node stays big enough to read.
- *Alternative: send the whole tree once.* It lost because 1M nodes is over 100 MB of JSON and freezes the tab.
- *Alternative: a higher cap (200) with a size threshold.* It lost because the owner wants at most 20 per level for readability.
- *Alternative: a server-side squarified layout.* It lost because it ties the backend to chart geometry.

### D2. "Other" is explorable
Clicking "Other" opens a paged table of the folded children (20 rows per page, "Show more"). Clicking a directory in that table re-roots the treemap at it. Nothing is unreachable.

### D3. ECharts themed from tokens; one category palette
A `chartTheme(tokens, mode)` function builds the ECharts theme from the skeleton's CSS variables: text, grid, tooltip surface, borders and font (Geist, tabular numerals). The theme is re-registered when the theme changes. A `categoryColor(category)` map lives in one place: video, images, audio, archives, ISOs/installers, documents, code/dev artefacts, VM/disk images, system, other. It uses token-derived hues with AA contrast in both themes. The treemap, sunburst, breakdown chart, table chips and legends all use it. Other changes (stale, duplicates, reports) import the same map.
- *Alternative: ECharts default palettes.* They lost because colours would be inconsistent between views and poor in dark mode.

### D4. Motion and states
Drill-down uses ECharts' built-in transition at 150–200 ms. It is turned off (`animation: false`) when `prefers-reduced-motion` is set. Each view has a skeleton for loading (chart-shaped and table-row skeletons from the shared components), an empty state ("No scan yet" with a Scan button, or "This folder is empty"), and an error state (message, retry, and a link to the log). The views use the skeleton app's shared `DataState` pattern.

### D5. Size mode is a query parameter
The server sorts and caps by the selected size. Re-sorting on the client would make the "Other" buckets wrong.

### D6. Open in Explorer through the backend, resolved from the snapshot
`POST /api/v1/shell/reveal {snapshot_id, entry_id}` resolves the path from the snapshot and checks that it exists (`GetFileAttributesExW`, metadata only). It then spawns `explorer.exe` with `/select,"<path>"`, built with `CommandExt::raw_arg`, because Explorer's argument parsing does not follow MSVCRT quoting. No shell is involved, and no client-supplied path is accepted.

### D7. Top-N from indexes
Per-snapshot indexes cover `(allocated DESC)` and `(logical DESC)` on file entries and dir aggregates. Paging uses `offset/limit` (20 by default). Memory per request is bounded by `limit`.

## Risks / Trade-offs

- A 20-child cap hides detail → "Other" is explorable (D2), the tooltip shows its count and size, and the setting can lower the cap further.
- The snapshot may be stale compared with disk → the snapshot time is shown, and reveal reports "path no longer exists".
- Deep paths → breadcrumb segments collapse in the middle, and the full path is in a tooltip and copyable.

## Open Questions

None.
