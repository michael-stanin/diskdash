## ADDED Requirements

### Requirement: Cold data on SSD suggestions
The system SHALL identify, on local fixed internal volumes whose media type is SATA SSD or NVMe SSD, the outermost folders (and standalone files) whose newest descendant time is older than a configurable age (default 12 months) and whose allocated size is at or above a minimum (default 1 GiB). It SHALL suggest moving each to an internal HDD volume. Time SHALL be last-modified unless the volume's time basis is last-access.

#### Scenario: Old video archive on NVMe
- **WHEN** `C:\Users\me\Videos\2019` (80 GiB, untouched for 3 years) is on an NVMe drive and an internal HDD has 500 GiB free
- **THEN** a suggestion to move it to the HDD is shown with reason "not modified for 3 years" and manual move instructions

#### Scenario: No internal HDD present
- **WHEN** the machine has no internal HDD volume (a USB HDD is connected but out of scope)
- **THEN** no cold-data suggestions are produced, and the page explains that no internal HDD tier was detected

### Requirement: Hot data on HDD suggestions
The system SHALL identify folders on internal HDD volumes that are frequently modified (at least a configurable share of bytes or files modified within the last N days, default 30), and detected app/game libraries (Steam `libraryfolders.vdf` and app manifests, Epic manifests) with recent use. It SHALL suggest moving them to an internal SSD volume. Manifest files MUST be read-only.

#### Scenario: Recently played Steam game on HDD
- **WHEN** a Steam app manifest on D: (internal HDD) shows the game was last played 2 days ago
- **THEN** a suggestion to move it to an SSD is shown, labelled "app library", with instructions to use Steam's "Move install folder"

#### Scenario: Unreadable manifest
- **WHEN** a Steam manifest cannot be parsed
- **THEN** it is skipped, the error is logged, and other suggestions are still produced

### Requirement: Target selection with headroom
The system SHALL assign each suggestion a target among local fixed internal volumes, on a different physical disk of the opposite tier. After all planned moves, the target's projected free space SHALL stay at or above both the percentage threshold (default 10%) and the absolute floor (default 20 GiB), plus a configurable headroom. Suggestions without a valid target SHALL be listed with reason `no_target_with_space`.

#### Scenario: Target would fall below the absolute floor
- **WHEN** moving a folder would leave the only internal HDD with 15 GiB free
- **THEN** the suggestion has no target and shows `no_target_with_space`

### Requirement: Projected free space
The system SHALL show, for each drive, the current free space and the projected free space after the suggestions the user has marked as planned. Projections SHALL update immediately as suggestions are toggled.

#### Scenario: Plan two suggestions
- **WHEN** the user marks two moves totalling 120 GiB from C: to D: as planned
- **THEN** C: projected free rises by 120 GiB and D: projected free falls by 120 GiB

### Requirement: Manual move instructions only
Each suggestion SHALL include manual move instructions rendered as copyable text: the launcher's own move feature for app/game libraries, a reinstall recommendation for registered app install folders, and otherwise a robocopy command line followed by verify-then-delete-via-Explorer steps. The system MUST NOT move, copy, link, delete or modify any file, and MUST NOT execute any instruction command. Suggestions SHALL be addable to a cleanup plan as `manual-move` items.

#### Scenario: Copy robocopy command
- **WHEN** the user clicks "Copy" on a plain data folder suggestion
- **THEN** the robocopy command with quoted source and destination is copied to the clipboard and nothing is executed

#### Scenario: Add to plan
- **WHEN** the user clicks "Add to plan" on a suggestion
- **THEN** a `manual-move` plan item with source, suggested destination and instructions is created, and no file is touched

### Requirement: Protected, excluded and app folders handled
Suggestions SHALL exclude protected paths and user exclusions, and SHALL flag folders that are install locations of registered applications as "app install – reinstall or use the app's own move feature".

#### Scenario: Program Files folder
- **WHEN** a cold folder lies under `C:\Program Files`
- **THEN** it is not suggested

### Requirement: Tiering view design and states
The Tiering page SHALL use the shared design-system components. Projected free space SHALL be shown as before/after fill bars per drive using the same low-space colouring as the home cards. The page SHALL show a skeleton while loading, a designed empty state when there are no suggestions (with the reason, for example "no internal HDD tier" or "nothing cold enough"), and an error state with retry.

#### Scenario: Nothing to suggest
- **WHEN** no folder qualifies as cold-on-SSD or hot-on-HDD
- **THEN** the page shows "No tiering suggestions" with the thresholds used and a link to adjust them in settings

#### Scenario: Analysis error
- **WHEN** the suggestion request fails
- **THEN** the page shows the error state with Retry, and the drive fill bars still render from inventory data

