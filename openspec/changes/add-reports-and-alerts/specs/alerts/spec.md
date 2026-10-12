## ADDED Requirements

### Requirement: Low free space alert
While the app is running, the system SHALL raise a low-space alert for a local fixed internal volume when its free space falls below the percentage threshold (default 10%) or below the absolute floor (default 20 GiB). The check SHALL run after every inventory refresh and every completed manual scan or quick rescan.

#### Scenario: Percentage threshold crossed
- **WHEN** a manual scan completes and C: is at 8% free with a 10% threshold
- **THEN** a low-space alert for C: is recorded, the header bell count increases, and a banner appears on the home page and C: drive view

#### Scenario: Absolute floor crossed on a small drive
- **WHEN** a 128 GiB drive has 18 GiB free (14%)
- **THEN** a low-space alert is raised because free space is below the 20 GiB floor

### Requirement: Fast growth alert
The system SHALL raise a growth alert when a folder's allocated size grows faster than a configurable rate (default 5 GiB/day), computed between the two most recent snapshots of the same root that are at least one day apart.

#### Scenario: Folder balloons
- **WHEN** `D:\VMs` grew by 30 GiB between scans taken 2 days apart
- **THEN** a growth alert names `D:\VMs` with a rate of 15 GiB/day and links to the history diff

#### Scenario: Insufficient history
- **WHEN** a root has only one snapshot
- **THEN** no growth alert is evaluated for it

### Requirement: In-app delivery only
Alerts SHALL be shown only inside the dashboard: a header alert indicator with a count, an Alerts page, and dismissible banners in the affected views, updated live over SSE. The system MUST NOT show OS notifications, and MUST NOT create Start-menu shortcuts, AppUserModelIDs, Task Scheduler entries, services, or background processes.

#### Scenario: Alert while a tab is open
- **WHEN** an inventory refresh raises a new alert while the dashboard is open
- **THEN** the bell count and banners update without a page reload, and no Windows notification appears

#### Scenario: App closed
- **WHEN** the dashboard server is not running
- **THEN** no alerts are evaluated, and on next start the Alerts page notes that alerts are only checked while the app runs

### Requirement: De-duplication, cooldown and dismiss
The system SHALL NOT re-raise an alert with the same key (volume or folder) until a cooldown (default 24 h) has elapsed, or the condition has cleared and recurred. A low-space condition SHALL clear only when free space rises above both limits plus a hysteresis margin. Dismissing a banner SHALL hide it until the alert is re-raised.

#### Scenario: Repeated scans while low
- **WHEN** three scans within 6 hours all find C: below the threshold
- **THEN** only one alert entry is created and the banner is not re-shown after dismissal

### Requirement: Alerts view design and states
The Alerts page and banners SHALL use the shared design-system alert, banner and badge components, with severity colours from the design tokens in light and dark. The page SHALL show a skeleton while loading, a designed empty state when there are no alerts, and an error state with retry.

#### Scenario: No alerts
- **WHEN** the user opens the Alerts page with no active or past alerts
- **THEN** it shows "All drives look healthy" with the thresholds in use and a link to change them

#### Scenario: Alerts fail to load
- **WHEN** the alerts request fails
- **THEN** the page shows the error state with Retry, and the header bell shows an unknown state rather than zero
