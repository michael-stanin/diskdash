## ADDED Requirements

### Requirement: Jobs run outside the request path
Long-running operations SHALL be submitted as jobs via the job runner and executed on dedicated worker threads, never on the async request executor. The submitting API call SHALL return a job id within 500 ms, without waiting for the work to finish.

#### Scenario: Submitting a job
- **WHEN** a client submits a job of kind `noop-sleep` (a test job) with a duration of 5 s
- **THEN** the API responds with a job id in under 500 ms and the job status is `queued` or `running`

#### Scenario: Panic is contained
- **WHEN** job code panics mid-job
- **THEN** the panic is caught at the job boundary, the job is marked `failed` with the panic message, and the server keeps serving requests

#### Scenario: Concurrency limit
- **WHEN** more jobs are submitted than the configured worker limit
- **THEN** the extra jobs stay `queued` and start in submission order as workers free up

### Requirement: Job lifecycle and status
Every job SHALL move through the states `queued → running → (succeeded | failed | cancelled)`, persisted in the `jobs` table. `GET /api/v1/jobs/{id}` SHALL return the current state, progress (`done`, `total` or null, `unit`), a human-readable phase, and an ETA in seconds when it can be estimated.

#### Scenario: Progress reported
- **WHEN** a running job reports 40 of 100 units done
- **THEN** `GET /api/v1/jobs/{id}` returns `done: 40, total: 100` and a non-null ETA

#### Scenario: Unknown total
- **WHEN** a job cannot estimate its total
- **THEN** `total` and `eta_seconds` are null, and the UI shows an indeterminate progress bar

#### Scenario: Restart marks orphaned jobs
- **WHEN** the server restarts while a job was `running`
- **THEN** that job is marked `interrupted` on startup, and it may be resumed if its kind supports resume

### Requirement: Cancellation
Clients SHALL be able to cancel a queued or running job via `POST /api/v1/jobs/{id}/cancel`. The worker MUST check for cancellation at least once per second of work and stop cleanly, leaving the job in state `cancelled` and its partial results either discarded or marked partial as the job kind defines.

#### Scenario: Cancel a running job
- **WHEN** the user cancels a running job
- **THEN** within 2 seconds the job state is `cancelled`, and no further progress events are emitted for it

### Requirement: Event stream
The server SHALL expose `GET /api/v1/events` as a Server-Sent Events stream delivering job lifecycle and progress events. Progress events SHALL be throttled to at most 4 per second per job. Each event SHALL carry a monotonically increasing id, and a reconnecting client sending `Last-Event-ID` SHALL receive the events it missed, up to a retention window of 1000 events.

#### Scenario: Live progress
- **WHEN** a job is running and a client is subscribed
- **THEN** the client receives `job.progress` events no more than 4 times per second, and a final `job.finished` event

#### Scenario: Reconnect without loss
- **WHEN** the client disconnects for 3 seconds and reconnects with `Last-Event-ID`
- **THEN** it receives the events emitted during the gap, in order

### Requirement: Path-scoped job locks
The job runner SHALL support exclusive locks on filesystem paths. A job that declares a path lock MUST NOT start while another job holds a lock on the same path, an ancestor, or a descendant. Instead it SHALL stay `queued` with the reason "waiting for <job>". Later phases rely on this so that overlapping scans, and duplicate hashing during a scan of the same path, never run at the same time.

#### Scenario: Overlapping paths serialize
- **WHEN** job A holds a lock on `D:\Data` and job B requests a lock on `D:\Data\Photos`
- **THEN** job B stays queued until job A finishes

#### Scenario: Disjoint paths run concurrently
- **WHEN** job A holds `D:\Data` and job B requests `E:\Media`
- **THEN** both run at the same time, subject to concurrency limits
