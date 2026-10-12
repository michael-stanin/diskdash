# ui-foundation Specification

## Purpose
TBD - created by archiving change add-project-skeleton. Update Purpose after archive.
## Requirements
### Requirement: Design tokens and themes
The frontend SHALL define all colours, spacing, radii, shadows and typography as design tokens (CSS variables consumed by Tailwind), with a light and a dark theme derived from the same token names. Components MUST NOT hard-code colour values. The user SHALL be able to choose light, dark or system; the choice SHALL persist in browser storage and SHALL be applied before first paint. Chart themes (ECharts) SHALL read the same tokens, so charts match the surrounding UI in both themes.

#### Scenario: Theme persists without flash
- **WHEN** the user selects the dark theme and reloads the page
- **THEN** the page renders dark from the first frame, with no light flash

#### Scenario: System theme follows OS
- **WHEN** the theme is "system" and Windows switches from light to dark
- **THEN** the UI and all charts switch to dark without a reload

#### Scenario: No hard-coded colours
- **WHEN** CI runs the lint step
- **THEN** a lint rule rejects hex/rgb colour literals in component files outside the token definitions

### Requirement: Typography and number formatting
The UI SHALL use Geist for text and Geist Mono for paths, both bundled locally (no web font requests), with a defined type scale. All numeric columns and size figures SHALL use tabular numerals. Sizes SHALL be shown in binary units (KiB, MiB, GiB, TiB) with at most three significant figures, and the exact byte count SHALL appear on hover or focus. Dates SHALL be shown as relative times ("3 months ago") with the absolute date on hover.

#### Scenario: Size display
- **WHEN** a size of 123456789012 bytes is rendered
- **THEN** it reads "115 GiB", and hovering shows "123,456,789,012 bytes"

#### Scenario: Font is local
- **WHEN** the app loads with the network adapter disabled
- **THEN** text renders in Geist, and no font request leaves the machine

### Requirement: App shell
The UI SHALL have a persistent app shell with:
- a collapsible left sidebar with icon + label navigation (Home, Drives, Recommendations, Plans, Settings; later phases add entries);
- a header showing the page title, a breadcrumb where applicable, the scan activity indicator, the reduced-mode badge, the connection status and the theme toggle;
- an in-app alert area for warnings such as low space.

The shell SHALL remain usable at 1280×720 without horizontal scrolling.

#### Scenario: Navigation
- **WHEN** the user clicks "Drives" in the sidebar
- **THEN** the Drives page opens, the item is highlighted, and the browser URL updates so back/forward work

#### Scenario: Small window
- **WHEN** the window is 1280×720
- **THEN** no horizontal scrollbar appears, and the sidebar can be collapsed to icons

#### Scenario: Backend stops
- **WHEN** the backend process stops while the UI is open
- **THEN** within 10 seconds the connection status shows "Disconnected", and data views show a retry prompt instead of stale data presented as live

### Requirement: Shared components
The frontend SHALL provide shared, documented components that every feature uses:
- a stat card (label, value, delta, icon);
- a size display;
- a fill bar with normal/caution/warning states;
- a media badge (HDD / SATA SSD / NVMe);
- a risk badge (Safe / Review / Expert);
- a virtualised data table with sorting, filtering, a column chooser and row actions (Open in Explorer, Copy path);
- a skeleton loader;
- an empty state (illustration/icon, message, primary action);
- an error state (message, details toggle, retry);
- a toast for in-app confirmations such as "Path copied".

A component gallery page SHALL be available in dev mode at `/dev/components`, showing each component in both themes.

#### Scenario: Gallery renders both themes
- **WHEN** the Playwright smoke test opens `/dev/components` in light and in dark
- **THEN** every component renders without console errors, and the screenshots are stored as CI artifacts

### Requirement: Loading, empty and error states
Every view that loads data SHALL show a skeleton that matches the final layout while loading, a designed empty state with a next step when there is no data (for example "No scan yet — Scan D:"), and an error state with a retry action when the request fails. Spinners alone MUST NOT be used for page-level loading.

#### Scenario: First run
- **WHEN** the app starts with no snapshots
- **THEN** Home shows the drive cards and an empty state inviting the user to run a first scan, not blank panels

#### Scenario: Failed request
- **WHEN** a data request returns 500
- **THEN** the view shows the error state with "Retry", and clicking it re-issues the request

### Requirement: Accessibility and motion
Interactive elements SHALL be keyboard reachable with visible focus rings. Text and essential graphics SHALL meet WCAG AA contrast in both themes. Animations SHALL last at most 200 ms and SHALL be disabled when `prefers-reduced-motion` is set. Charts SHALL provide a table alternative for their data.

#### Scenario: Keyboard navigation
- **WHEN** the user presses Tab from page load
- **THEN** focus moves through the sidebar, header controls and page content in order, with a visible focus ring

#### Scenario: Reduced motion
- **WHEN** the OS has reduced motion enabled
- **THEN** page and chart transitions render without animation

#### Scenario: Contrast check
- **WHEN** the Playwright smoke test runs axe on the shell in both themes
- **THEN** no contrast violations are reported

