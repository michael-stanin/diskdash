## Why

Changes 1–3 collect drive and scan data, but the user still cannot *see* where the space goes. This change turns snapshot data into the visual answers the brief asks for: an interactive treemap/sunburst, Top-N folders and files, a file-type breakdown, and drive cards on the home page. These are the core analysis views, so they must look modern and polished, not like a debug page.

## What Changes

- **Drive view** with an Apache ECharts treemap and sunburst of a snapshot. Clicking a node drills down; the breadcrumb navigates back. Children load lazily, one level at a time, **capped at 20 per level plus an "Other" bucket** (a setting can lower the cap to 5).
- **Top-N largest folders** (recursive) and **largest files** tables: 20 rows by default with "Show more", virtualised (TanStack Table + Virtual), with sort, filter and drill-down.
- **Category and extension breakdown**, driven by the scan-time category from `filesystem-scan`. A single category colour map is shared by every view.
- A global **logical size vs. size on disk** toggle that applies to every chart, table and total.
- **Copy path** (client side) and **Open in Explorer**. Open in Explorer is a backend call that launches `explorer.exe /select,<path>` for paths resolved from a snapshot only.
- **Visual design of the analysis views**: designed loading, empty and error states; ECharts themed from the design tokens in light and dark; tabular numerals; exact byte count on hover; drill-down transitions of 200 ms or less that respect reduced motion. These build on the design system (tokens, Tailwind + shadcn/ui, bundled Geist, app shell) defined in `add-project-skeleton`, which this change does not redefine.
- **Home page drive cards** for local fixed internal drives: media badge, health, fill bar, low-space flag (below 10% **or** below 20 GiB, from `drive-inventory`), last-scan summary, and a "Potential savings" slot that `cleanup-rules` fills in.
- Read-only API under `/api/v1/snapshots/{id}/…` for tree levels, top-N and breakdowns. TS types are generated with ts-rs.

## Capabilities

### New Capabilities
- `usage-visualization`: Read-only API and UI for exploring a scan snapshot: treemap/sunburst drill-down, Top-N tables, type breakdown, size-mode toggle, open-in-Explorer and copy-path, home-page drive cards, and the visual design of these analysis views.

### Modified Capabilities
None

## Impact

- `crates/diskdash` (api: snapshot view routes, reveal route), `crates/diskdash-store` (tree/top-N/breakdown queries and indexes in the snapshot DB schema).
- `frontend/`: drive view, home page, chart wrappers themed from tokens, category colour map, and size formatter (on the skeleton's shared components).
- Safety rules touched: **1** (the reveal endpoint is POST + CSRF), **2** (ECharts is bundled, no CDN), **3** (read-only; launching Explorer is an allowed side effect).

## Depends on

`add-project-skeleton` (design system, app shell), `add-drive-inventory`, `add-filesystem-scan`.
