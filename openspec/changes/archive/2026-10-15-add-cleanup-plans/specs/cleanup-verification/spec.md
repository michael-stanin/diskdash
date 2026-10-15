## ADDED Requirements

### Requirement: Verify plan items after a manual rescan
After a completed manual rescan whose root covers plan items, the system SHALL compare each covered item with the new snapshot by normalised path. It SHALL classify the item as removed, still present, partially removed, grown, or moved (for manual-move items whose destination now exists), and SHALL record the bytes freed against the item's baseline.

#### Scenario: Item removed by the user
- **WHEN** a planned 3 GiB folder is absent in the rescan
- **THEN** the item is marked "removed" with 3 GiB freed

#### Scenario: Item still present
- **WHEN** an item marked done still exists with the same size in the rescan
- **THEN** it is flagged "still present – 0 B freed"

#### Scenario: Rescan does not cover the item
- **WHEN** the rescan root is `D:\` and the item is on `C:`
- **THEN** the item stays "unverified"

### Requirement: Plan-level freed summary
The system SHALL show per-plan totals of verified freed bytes alongside the volume free-space change between the baseline and rescan snapshots, labelled as including unrelated activity.

#### Scenario: Summary after rescan
- **WHEN** verification completes for a plan with 10 items on C:
- **THEN** the plan shows the total freed by planned items and the C: free-space change as separate figures

### Requirement: Verification is read-only
Verification SHALL use snapshot data only, and MUST NOT access the filesystem beyond the user-initiated rescan itself.

#### Scenario: No live stat
- **WHEN** verification runs
- **THEN** no file or directory is opened or queried by the verification code (asserted in tests)
