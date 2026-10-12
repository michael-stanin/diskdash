## ADDED Requirements

### Requirement: Build a plan from suggestions
The system SHALL let the user create named cleanup plans and add items to them from any suggestion source (cleanup candidates, duplicate proposals, stale data results, tiering suggestions). Items SHALL be identified by snapshot entry id or stored suggestion id, never by a free-form path supplied by the client. Each item SHALL record path, kind, risk, source, and baseline logical and allocated size, modification time and file id at the time it was added.

#### Scenario: Add a cleanup candidate
- **WHEN** the user clicks "Add to plan" on a `node_modules` candidate of 1.2 GiB
- **THEN** the plan gains one item with that path, risk, source "cleanup-rules", and a baseline of 1.2 GiB

#### Scenario: Duplicate add
- **WHEN** the user adds an item whose path is already in the plan
- **THEN** no second item is created and the UI says it is already planned

#### Scenario: Unknown suggestion id
- **WHEN** an add request references a suggestion id that does not exist
- **THEN** the API responds 404 and the plan is unchanged

### Requirement: Plan overview with totals and manual instructions
A plan SHALL show its items with path, size, risk, kind and status, and SHALL show the total item count and total logical and allocated bytes (overall and per drive). Each item SHALL carry manual instructions for the user, rendered as text from per-kind templates, with copy-to-clipboard and "Open in Explorer" buttons.

#### Scenario: System setting item
- **WHEN** a plan contains the hiberfil.sys suggestion
- **THEN** the item shows the instruction text with `powercfg /h /type reduced` and `powercfg /h off` as copyable text and a note to run it in an elevated terminal

### Requirement: The app never acts on plan items
The system MUST NOT delete, recycle, move, rename, compress, archive or link any user file, and MUST NOT execute any command shown in instructions. The only permitted side effects are launching Explorer to show a path, copying text to the clipboard, and writing its own database rows and exported files.

#### Scenario: No action endpoint
- **WHEN** the API surface is enumerated in a test
- **THEN** no endpoint accepts a request that performs a file operation or runs an instruction command

### Requirement: Track item status
The user SHALL be able to mark each item as todo, done or skipped, and add a note. Totals SHALL show bytes by status.

#### Scenario: Mark done
- **WHEN** the user marks a 2 GiB item as done
- **THEN** the "done" total increases by 2 GiB and the item awaits verification

### Requirement: Export paths and instructions only
The system SHALL export a plan as CSV or TXT containing status, risk, kind, path, sizes, instructions and notes. Exports MUST NOT be executable scripts: no `.ps1`, `.bat` or `.cmd` output, and no script headers.

#### Scenario: CSV export with special characters
- **WHEN** a plan item path contains a comma and quotes
- **THEN** the CSV field is correctly escaped and the file opens with the right columns

#### Scenario: Export location not writable
- **WHEN** writing the export to the chosen folder fails with access denied
- **THEN** the error is shown and no partial file remains

### Requirement: Plans view design and states
The Plans page SHALL use the shared design-system components (checklist rows, risk badges, category colours, size formatting with exact bytes on hover). It SHALL show a skeleton while loading, a designed empty state when no plans exist, and an error state with retry.

#### Scenario: No plans yet
- **WHEN** the user opens Plans with no plans created
- **THEN** an empty state explains plans and links to Recommendations to add suggestions

#### Scenario: Plan fails to load
- **WHEN** loading a plan fails with a server error
- **THEN** the page shows the error state with Retry, and other plans remain selectable

