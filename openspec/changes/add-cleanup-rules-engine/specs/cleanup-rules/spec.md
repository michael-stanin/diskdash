## ADDED Requirements

### Requirement: Rule definition schema
Each cleanup rule SHALL be defined in YAML with the fields `id` (unique), `category`, `title`, `explanation` (plain language), `risk` (one of Safe, Review, Expert), `paths` and/or `globs`, optional `min_age_days`, optional `conditions`, `kind` (`manual-cleanup` for items the user can remove by hand, or `system-setting` for space controlled by a Windows setting or tool), and `instructions`. Every rule MUST include non-empty `instructions` describing the manual steps. Commands in instructions are displayed as text only and MUST NOT be executed by the system. Path entries SHALL support `%ENV%` variables and known-folder tokens, which are expanded for the interactive user.

#### Scenario: Valid rule loads
- **WHEN** a YAML file defines a rule with all required fields and `risk: Safe`
- **THEN** the rule is loaded and appears in the rule list with its expanded paths

#### Scenario: Rule without instructions
- **WHEN** a rule has `kind: system-setting` and no `instructions`
- **THEN** the rule is rejected with a validation error naming the file and rule id

### Requirement: Rule loading, override and error isolation
The system SHALL load built-in rules from the repository `rules/` directory and user rules from `%LOCALAPPDATA%\diskdash\rules\`. A user rule with the same `id` SHALL override the built-in one, and ids listed under `disabled` SHALL be skipped. Malformed YAML or schema violations MUST be reported (file, rule id or index, message) through the API, the UI and the log, and the invalid rule MUST be skipped without stopping the other rules from loading.

#### Scenario: Broken user rule file
- **WHEN** a user rule file contains invalid YAML
- **THEN** all other rules still load, and the Rules page lists the file with the parse error and line number

#### Scenario: User override
- **WHEN** a user rule has the same id as a built-in rule
- **THEN** only the user's version is evaluated and it is marked "overridden" in the rule list

### Requirement: Snapshot-based evaluation without file content access
Rule evaluation SHALL run as a background job against a completed snapshot and SHALL derive matches from snapshot entries only. The only additional filesystem access permitted is reading metadata of known system locations (for example `SHQueryRecycleBinW` per drive, and attributes and size of hiberfil.sys, pagefile.sys and swapfile.sys). Evaluation MUST NOT open any user file for reading or writing, and MUST NOT open cloud placeholder files.

#### Scenario: Evaluate after a scan
- **WHEN** a scan of drive C: completes
- **THEN** a rule-evaluation job runs for that snapshot, reports progress over SSE, and stores its matches linked to the snapshot

#### Scenario: Metadata lookup denied
- **WHEN** reading the size of pagefile.sys fails with access denied
- **THEN** the candidate is shown with size "unknown", the error is logged, and the evaluation continues

### Requirement: Conditional matching
The rule engine SHALL support the conditions `sibling_exists`, `child_exists`, `min_size`, `min_age_days` and `extracted_sibling`, all evaluated from snapshot data. Age SHALL use last-modified time unless the stale-data analysis has marked last-access time as reliable for that volume.

#### Scenario: node_modules without package.json
- **WHEN** a `node_modules` directory has no sibling `package.json` in the snapshot
- **THEN** the developer-artifacts rule does not match it

#### Scenario: Virtual environment detection
- **WHEN** a directory named `.venv` contains `pyvenv.cfg`
- **THEN** it matches the Python venv rule, and a `.venv` directory without `pyvenv.cfg` does not

### Requirement: Built-in rule set
The system SHALL ship built-in rules covering at least: Windows temp and caches (`%TEMP%`, `C:\Windows\Temp`, `SoftwareDistribution\Download`, Delivery Optimization cache, WER reports, crash and memory dumps, thumbnail and icon caches, Prefetch at Expert risk); Windows upgrade leftovers (`Windows.old`, `$WINDOWS.~BT`, `$WINDOWS.~WS`); Recycle Bin per drive; Chrome, Edge, Firefox and Brave caches; developer artifacts (`node_modules` with sibling `package.json`, `.venv`/`venv` with `pyvenv.cfg`, `__pycache__`, `target/` with sibling `Cargo.toml`, `bin`/`obj` beside a project file, `.gradle`, `~/.m2`, npm/pnpm/yarn/pip/NuGet/Cargo caches, Android SDK system images, Visual Studio and JetBrains caches); app caches (Steam shader cache and downloading folder, Teams, Slack, Discord, Spotify, Adobe media cache); Downloads triage (installers, ISOs, archives already extracted next to themselves); and `*.log`, `*.etl`, `*.tmp`, `~$*` files older than a configurable N days (default 30).

#### Scenario: Extracted archive in Downloads
- **WHEN** Downloads contains `tool.zip` (100 MiB) and a sibling directory `tool` totalling at least 50 MiB
- **THEN** `tool.zip` is listed under Downloads triage with risk Review and an explanation that it appears already extracted

#### Scenario: Old log files only
- **WHEN** a folder contains `a.log` modified 5 days ago and `b.log` modified 90 days ago, with N = 30
- **THEN** only `b.log` matches the old-logs rule

### Requirement: System files reported with instructions only
Rules for hiberfil.sys, pagefile.sys sizing, System Restore and shadow copies, WinSxS and Docker/WSL `.vhdx` files SHALL be of kind `system-setting`. They SHALL show the measured or estimated size and step-by-step instructions (for example `powercfg /h off` or `powercfg /h /type reduced`, `DISM /Online /Cleanup-Image /StartComponentCleanup`, `vssadmin list shadowstorage`, `Optimize-VHD` or `diskpart compact vdisk`). The system MUST NOT execute these commands. It MUST NOT run admin-only diagnostics such as `DISM /AnalyzeComponentStore`, and SHALL only suggest them.

#### Scenario: Hibernation file present
- **WHEN** `C:\hiberfil.sys` exists with 12 GiB allocated
- **THEN** a system-setting candidate with 12 GiB and copyable powercfg instructions is shown, and the app runs nothing

#### Scenario: Shadow storage in reduced mode
- **WHEN** the read-only elevated helper is not running and shadow storage size cannot be queried
- **THEN** the shadow copies candidate shows "size unavailable without admin" along with instructions to check it manually

### Requirement: Potential savings by drive and risk
The system SHALL compute potential savings per drive, split by risk level (Safe, Review, Expert), using allocated size. It SHALL count each byte at most once when matches overlap, by attributing nested matches to the outermost matching ancestor. `system-setting` candidates SHALL be totalled separately from `manual-cleanup` ones. Home page drive cards SHALL show these totals.

#### Scenario: Overlapping rules
- **WHEN** `%TEMP%` (1 GiB) matches the temp rule and `%TEMP%\x.tmp` (100 MiB) also matches the old-tmp rule
- **THEN** the Safe total for that drive includes 1 GiB, not 1.1 GiB

### Requirement: Recommendations list
The system SHALL provide a Recommendations page that lists candidates across all drives, ranked by bytes freed weighted by risk (Safe before Review before Expert at similar size). Each item SHALL show rule title, explanation, risk, path or location, size and kind. A filter SHALL be available for drive, risk and category. Each item SHALL offer copy-instructions, copy-path and "Open in Explorer". The page MUST NOT offer any control that deletes, moves or otherwise changes files.

#### Scenario: Ranking
- **WHEN** a Safe candidate of 2 GiB and an Expert candidate of 3 GiB exist
- **THEN** the Safe candidate is ranked above the Expert one

### Requirement: Designed states and shared components for suggestion views
The Recommendations page, the drive-view cleanup tab and the Rules page SHALL be built from the project's shared design-system components. Risk SHALL be shown as consistent Safe/Review/Expert badges, and categories SHALL use the shared category colour map. Each view SHALL present a skeleton while loading, a designed empty state with a next action, and an error state with retry.

#### Scenario: No snapshot yet
- **WHEN** the user opens Recommendations before any scan has completed
- **THEN** an empty state explains that suggestions need a scan and offers "Scan a drive"

#### Scenario: Evaluation failed
- **WHEN** the rule-evaluation job for a snapshot failed
- **THEN** the cleanup tab shows an error state with the reason and a "Re-run rules" action, and the other drive-view tabs still work

