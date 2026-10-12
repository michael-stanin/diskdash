## MODIFIED Requirements

### Requirement: Persistent settings
The system SHALL store user settings in the database and expose them via `GET /api/v1/settings` and `PUT /api/v1/settings`. It SHALL validate every value against a typed schema with bounds. Defaults SHALL be: low-space threshold 10% of capacity, low-space absolute floor 20 GiB, stale threshold 12 months, HDD scan concurrency 1 per physical disk, SSD scan concurrency 4, chart children per level 20 (allowed range 5–20), and hard-link detection on the system volume only. No setting SHALL exist that enables file-modifying behaviour, network drives or removable drives.

#### Scenario: First run defaults
- **WHEN** the application starts with an empty database
- **THEN** `GET /api/v1/settings` returns the documented defaults

#### Scenario: Invalid value rejected
- **WHEN** the user submits a low-space threshold of 150%
- **THEN** the server responds 422 with a field-level error and stores nothing

#### Scenario: Chart cap above 20 rejected
- **WHEN** the user submits 30 children per level
- **THEN** the server responds 422 with "must be between 5 and 20"
