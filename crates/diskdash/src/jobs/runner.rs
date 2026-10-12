//! Bounded worker pool with a FIFO queue, path locks, cancellation, panic
//! containment, state persisted to `jobs`, and SSE events.

use std::collections::{HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Instant;

use diskdash_store::Db;
use rusqlite::{params, Connection, OptionalExtension};

use super::locks::{any_conflict, LockPath};
use super::{Job, JobContext, JobError, JobState, JobStatus, Progress, ProgressSink};
use crate::events::EventBus;
use crate::time::now_rfc3339;

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("job {0} not found")]
    NotFound(u64),
    #[error("job runner is shutting down")]
    ShuttingDown,
    #[error("cannot start worker thread: {0}")]
    Spawn(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Clone)]
pub struct JobRunner {
    shared: Arc<Shared>,
}

struct Shared {
    db: Db,
    writer: Mutex<Connection>,
    events: Arc<EventBus>,
    sched: Mutex<Sched>,
    cv: Condvar,
    live: Mutex<HashMap<u64, JobStatus>>,
}

#[derive(Default)]
struct Sched {
    queue: VecDeque<Queued>,
    running: HashMap<u64, Running>,
    shutdown: bool,
}

struct Queued {
    id: u64,
    job: Box<dyn Job>,
    locks: Vec<LockPath>,
    cancel: Arc<AtomicBool>,
}

struct Running {
    locks: Vec<LockPath>,
    cancel: Arc<AtomicBool>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic inside a job never holds these locks; recover rather than cascade.
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl JobRunner {
    /// Start `workers` threads. Jobs left `queued`/`running` by a previous
    /// process are marked `interrupted` first.
    pub fn start(db: Db, events: Arc<EventBus>, workers: usize) -> Result<Self, RunnerError> {
        let writer = db.connect()?;
        writer.execute(
            "UPDATE jobs SET status = 'interrupted', finished_at = ?1 WHERE status IN ('queued', 'running')",
            [now_rfc3339()],
        )?;
        let shared = Arc::new(Shared {
            db,
            writer: Mutex::new(writer),
            events,
            sched: Mutex::new(Sched::default()),
            cv: Condvar::new(),
            live: Mutex::new(HashMap::new()),
        });
        for n in 0..workers.max(1) {
            let s = shared.clone();
            thread::Builder::new().name(format!("job-worker-{n}")).spawn(move || worker_loop(s))?;
        }
        Ok(Self { shared })
    }

    /// Queue a job; returns immediately with its id.
    pub fn submit(&self, job: Box<dyn Job>) -> Result<u64, RunnerError> {
        if lock(&self.shared.sched).shutdown {
            return Err(RunnerError::ShuttingDown);
        }
        let now = now_rfc3339();
        let kind = job.kind();
        let id = {
            let w = lock(&self.shared.writer);
            w.execute(
                "INSERT INTO jobs(kind, params, status, created_at) VALUES (?1, ?2, 'queued', ?3)",
                params![kind, job.params().to_string(), now],
            )?;
            w.last_insert_rowid() as u64
        };
        let status = JobStatus {
            id,
            kind: kind.to_string(),
            status: JobState::Queued,
            phase: None,
            done: 0,
            total: None,
            unit: None,
            eta_seconds: None,
            error: None,
            waiting_for: None,
            created_at: now,
            started_at: None,
            finished_at: None,
        };
        lock(&self.shared.live).insert(id, status.clone());
        self.shared.events.publish("job.queued", &status);
        let locks = job.locks().iter().map(|p| LockPath::new(p)).collect();
        lock(&self.shared.sched).queue.push_back(Queued { id, job, locks, cancel: Arc::new(AtomicBool::new(false)) });
        self.shared.cv.notify_all();
        Ok(id)
    }

    /// Cancel a queued (immediately) or running (cooperatively) job.
    pub fn cancel(&self, id: u64) -> Result<JobStatus, RunnerError> {
        let removed = {
            let mut s = lock(&self.shared.sched);
            if let Some(r) = s.running.get(&id) {
                r.cancel.store(true, Ordering::Relaxed);
                None
            } else {
                s.queue.iter().position(|q| q.id == id).and_then(|i| s.queue.remove(i))
            }
        };
        if removed.is_some() {
            self.shared.finish(id, JobState::Cancelled, None);
            self.shared.cv.notify_all();
        }
        self.get(id)
    }

    pub fn get(&self, id: u64) -> Result<JobStatus, RunnerError> {
        if let Some(s) = lock(&self.shared.live).get(&id) {
            return Ok(s.clone());
        }
        let conn = self.shared.db.connect_read_only()?;
        load_status(&conn, id)?.ok_or(RunnerError::NotFound(id))
    }

    /// Most recent jobs first.
    pub fn recent(&self, limit: u32) -> Result<Vec<JobStatus>, RunnerError> {
        let conn = self.shared.db.connect_read_only()?;
        let mut stmt = conn.prepare(&format!("{SELECT_STATUS} ORDER BY id DESC LIMIT ?1"))?;
        let rows = stmt.query_map([limit], row_to_status)?;
        let mut out = Vec::new();
        for r in rows {
            let mut st = r?;
            if let Some(live) = lock(&self.shared.live).get(&st.id) {
                st = live.clone();
            }
            out.push(st);
        }
        Ok(out)
    }

    /// Stop accepting jobs, cancel running ones and wake the workers.
    pub fn shutdown(&self) {
        let mut s = lock(&self.shared.sched);
        s.shutdown = true;
        for r in s.running.values() {
            r.cancel.store(true, Ordering::Relaxed);
        }
        drop(s);
        self.shared.cv.notify_all();
    }
}

fn worker_loop(shared: Arc<Shared>) {
    loop {
        let Some(q) = shared.next_job() else { return };
        shared.run_one(q);
    }
}

impl Shared {
    /// Block until a job can start (FIFO, skipping jobs blocked by path locks).
    fn next_job(&self) -> Option<Queued> {
        let mut s = lock(&self.sched);
        loop {
            if s.shutdown {
                return None;
            }
            let mut pick = None;
            let mut waiting = Vec::new();
            for (i, q) in s.queue.iter().enumerate() {
                let blocker = s
                    .running
                    .iter()
                    .find(|(_, r)| any_conflict(&q.locks, &r.locks))
                    .map(|(id, _)| *id)
                    .or_else(|| s.queue.iter().take(i).find(|e| any_conflict(&q.locks, &e.locks)).map(|e| e.id));
                match blocker {
                    None => {
                        pick = Some(i);
                        break;
                    }
                    Some(b) => waiting.push((q.id, b)),
                }
            }
            for (id, blocker) in waiting {
                self.set_waiting(id, Some(blocker));
            }
            if let Some(i) = pick {
                let q = s.queue.remove(i)?;
                s.running.insert(q.id, Running { locks: q.locks.clone(), cancel: q.cancel.clone() });
                return Some(q);
            }
            s = self.cv.wait(s).unwrap_or_else(|p| p.into_inner());
        }
    }

    fn set_waiting(&self, id: u64, blocker: Option<u64>) {
        let changed = {
            let mut live = lock(&self.live);
            match live.get_mut(&id) {
                Some(st) if st.waiting_for != blocker => {
                    st.waiting_for = blocker;
                    st.phase = blocker.map(|b| format!("waiting for job {b}"));
                    Some(st.clone())
                }
                _ => None,
            }
        };
        if let Some(st) = changed {
            self.events.publish("job.progress", &st);
        }
    }

    fn run_one(self: &Arc<Self>, mut q: Queued) {
        let started_at = now_rfc3339();
        let status = {
            let mut live = lock(&self.live);
            live.get_mut(&q.id).map(|st| {
                st.status = JobState::Running;
                st.started_at = Some(started_at.clone());
                st.waiting_for = None;
                st.phase = None;
                st.clone()
            })
        };
        self.persist(|w| {
            w.execute(
                "UPDATE jobs SET status = 'running', started_at = ?2, phase = NULL WHERE id = ?1",
                params![q.id, started_at],
            )
        });
        if let Some(st) = status {
            self.events.publish("job.started", &st);
        }

        let ctx = JobContext {
            id: q.id,
            cancel: q.cancel.clone(),
            reporter: self.clone(),
            started: Instant::now(),
            last_emit: Mutex::new(None),
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| q.job.run(&ctx)));
        let (state, error) = match outcome {
            Ok(Ok(())) => (JobState::Succeeded, None),
            Ok(Err(JobError::Cancelled)) => (JobState::Cancelled, None),
            Ok(Err(JobError::Failed(msg))) => (JobState::Failed, Some(msg)),
            Err(panic) => {
                let msg = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "job panicked".to_string());
                tracing::error!(job = q.id, kind = q.job.kind(), panic = %msg, "job panicked");
                (JobState::Failed, Some(format!("panic: {msg}")))
            }
        };
        lock(&self.sched).running.remove(&q.id);
        self.finish(q.id, state, error);
        self.cv.notify_all();
    }

    fn finish(&self, id: u64, state: JobState, error: Option<String>) {
        let finished_at = now_rfc3339();
        let status = lock(&self.live).remove(&id).map(|mut st| {
            st.status = state;
            st.error = error.clone();
            st.finished_at = Some(finished_at.clone());
            st.eta_seconds = None;
            st.waiting_for = None;
            st
        });
        self.persist(|w| {
            w.execute(
                "UPDATE jobs SET status = ?2, error = ?3, finished_at = ?4, eta_seconds = NULL WHERE id = ?1",
                params![id, state.as_str(), error, finished_at],
            )
        });
        if let Some(st) = status {
            self.persist(|w| {
                let seq: i64 =
                    w.query_row("SELECT coalesce(max(seq), 0) + 1 FROM job_events WHERE job_id = ?1", [id], |r| {
                        r.get(0)
                    })?;
                w.execute(
                    "INSERT INTO job_events(job_id, seq, ts, type, payload) VALUES (?1, ?2, ?3, 'job.finished', ?4)",
                    params![id, seq, st.finished_at, serde_json::to_string(&st).unwrap_or_default()],
                )
            });
            self.events.publish("job.finished", &st);
        }
    }

    fn persist(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<usize>) {
        if let Err(e) = f(&lock(&self.writer)) {
            tracing::error!(error = %e, "failed to persist job state");
        }
    }
}

impl ProgressSink for Shared {
    fn progress(&self, id: u64, p: Progress) {
        let st = {
            let mut live = lock(&self.live);
            let Some(st) = live.get_mut(&id) else { return };
            st.done = p.done;
            st.total = p.total;
            st.unit = p.unit.clone();
            st.phase = p.phase.clone();
            st.eta_seconds = p.eta_seconds;
            st.clone()
        };
        self.persist(|w| {
            w.execute(
                "UPDATE jobs SET done = ?2, total = ?3, unit = ?4, phase = ?5, eta_seconds = ?6 WHERE id = ?1",
                params![id, p.done as i64, p.total.map(|t| t as i64), p.unit, p.phase, p.eta_seconds.map(|e| e as i64)],
            )
        });
        self.events.publish("job.progress", &st);
    }
}

const SELECT_STATUS: &str = "SELECT id, kind, status, phase, done, total, unit, eta_seconds, error, created_at, started_at, finished_at FROM jobs";

fn row_to_status(r: &rusqlite::Row<'_>) -> rusqlite::Result<JobStatus> {
    let status: String = r.get(2)?;
    Ok(JobStatus {
        id: r.get::<_, i64>(0)? as u64,
        kind: r.get(1)?,
        status: JobState::parse(&status).unwrap_or(JobState::Failed),
        phase: r.get(3)?,
        done: r.get::<_, i64>(4)? as u64,
        total: r.get::<_, Option<i64>>(5)?.map(|t| t as u64),
        unit: r.get(6)?,
        eta_seconds: r.get::<_, Option<i64>>(7)?.map(|t| t as u64),
        error: r.get(8)?,
        waiting_for: None,
        created_at: r.get(9)?,
        started_at: r.get(10)?,
        finished_at: r.get(11)?,
    })
}

fn load_status(conn: &Connection, id: u64) -> rusqlite::Result<Option<JobStatus>> {
    conn.query_row(&format!("{SELECT_STATUS} WHERE id = ?1"), [id], row_to_status).optional()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::builtin::{NoopSleep, PanicJob};
    use diskdash_store::{open_catalog, DataDir, OwnFiles};
    use std::path::PathBuf;
    use std::time::Duration;

    fn runner(workers: usize) -> (tempfile::TempDir, JobRunner, Arc<EventBus>) {
        let tmp = tempfile::tempdir().unwrap();
        let dd = DataDir::at(tmp.path().join("dd"));
        let own = OwnFiles::new(&dd).unwrap();
        let db = open_catalog(&dd, &own).unwrap();
        let bus = Arc::new(EventBus::new());
        let r = JobRunner::start(db, bus.clone(), workers).unwrap();
        (tmp, r, bus)
    }

    fn wait_for(r: &JobRunner, id: u64, pred: impl Fn(&JobStatus) -> bool) -> JobStatus {
        let start = Instant::now();
        loop {
            let st = r.get(id).unwrap();
            if pred(&st) {
                return st;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "timed out: {st:?}");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn sleep_job(ms: u64) -> Box<dyn Job> {
        Box::new(NoopSleep { duration_ms: ms, lock: None })
    }

    fn locked_job(ms: u64, path: &str) -> Box<dyn Job> {
        Box::new(NoopSleep { duration_ms: ms, lock: Some(PathBuf::from(path)) })
    }

    #[test]
    fn submit_returns_fast_and_job_succeeds() {
        let (_t, r, _b) = runner(2);
        let start = Instant::now();
        let id = r.submit(sleep_job(300)).unwrap();
        assert!(start.elapsed() < Duration::from_millis(500));
        let st = r.get(id).unwrap();
        assert!(matches!(st.status, JobState::Queued | JobState::Running));
        let st = wait_for(&r, id, |s| s.status.is_finished());
        assert_eq!(st.status, JobState::Succeeded);
        assert!(st.started_at.is_some() && st.finished_at.is_some());
    }

    #[test]
    fn panic_fails_only_that_job() {
        let (_t, r, _b) = runner(1);
        let bad = r.submit(Box::new(PanicJob)).unwrap();
        let st = wait_for(&r, bad, |s| s.status.is_finished());
        assert_eq!(st.status, JobState::Failed);
        assert!(st.error.unwrap().contains("boom"));
        // The single worker survived and runs the next job.
        let good = r.submit(sleep_job(10)).unwrap();
        assert_eq!(wait_for(&r, good, |s| s.status.is_finished()).status, JobState::Succeeded);
    }

    #[test]
    fn cancel_running_job_within_two_seconds() {
        let (_t, r, _b) = runner(1);
        let id = r.submit(sleep_job(60_000)).unwrap();
        wait_for(&r, id, |s| s.status == JobState::Running);
        let start = Instant::now();
        r.cancel(id).unwrap();
        let st = wait_for(&r, id, |s| s.status.is_finished());
        assert_eq!(st.status, JobState::Cancelled);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn cancel_queued_job_immediately() {
        let (_t, r, _b) = runner(1);
        let blocker = r.submit(sleep_job(60_000)).unwrap();
        let queued = r.submit(sleep_job(10)).unwrap();
        assert_eq!(r.cancel(queued).unwrap().status, JobState::Cancelled);
        r.cancel(blocker).unwrap();
    }

    #[test]
    fn extra_jobs_wait_for_a_free_worker_in_order() {
        let (_t, r, _b) = runner(1);
        let a = r.submit(sleep_job(200)).unwrap();
        let b = r.submit(sleep_job(10)).unwrap();
        wait_for(&r, a, |s| s.status == JobState::Running);
        assert_eq!(r.get(b).unwrap().status, JobState::Queued);
        let sa = wait_for(&r, a, |s| s.status.is_finished());
        let sb = wait_for(&r, b, |s| s.status.is_finished());
        assert!(sb.started_at.unwrap() >= sa.finished_at.unwrap());
    }

    #[test]
    fn overlapping_locks_serialise_and_disjoint_run_together() {
        let (_t, r, _b) = runner(3);
        let a = r.submit(locked_job(400, r"D:\Data")).unwrap();
        wait_for(&r, a, |s| s.status == JobState::Running);
        let b = r.submit(locked_job(10, r"D:\Data\Photos")).unwrap();
        let c = r.submit(locked_job(10, r"E:\Media")).unwrap();
        let sc = wait_for(&r, c, |s| s.status.is_finished());
        assert_eq!(sc.status, JobState::Succeeded);
        let sb = r.get(b).unwrap();
        assert_eq!(sb.status, JobState::Queued, "b must wait for a");
        assert_eq!(sb.waiting_for, Some(a));
        assert_eq!(wait_for(&r, b, |s| s.status.is_finished()).status, JobState::Succeeded);
    }

    #[test]
    fn progress_is_reported_and_persisted() {
        let (_t, r, bus) = runner(1);
        let (_, mut rx) = bus.subscribe(None);
        let id = r.submit(sleep_job(700)).unwrap();
        let st = wait_for(&r, id, |s| s.done > 0);
        assert_eq!(st.total, Some(700));
        assert_eq!(st.unit.as_deref(), Some("ms"));
        wait_for(&r, id, |s| s.status.is_finished());
        let mut kinds = Vec::new();
        while let Ok(e) = rx.try_recv() {
            kinds.push(e.kind);
        }
        assert_eq!(kinds.first().map(String::as_str), Some("job.queued"));
        assert_eq!(kinds.last().map(String::as_str), Some("job.finished"));
        let progress = kinds.iter().filter(|k| *k == "job.progress").count();
        assert!((1..=5).contains(&progress), "≤ 4/s over 0.7 s, got {progress}");
    }

    #[test]
    fn restart_marks_orphans_interrupted() {
        let tmp = tempfile::tempdir().unwrap();
        let dd = DataDir::at(tmp.path().join("dd"));
        let own = OwnFiles::new(&dd).unwrap();
        let db = open_catalog(&dd, &own).unwrap();
        db.connect()
            .unwrap()
            .execute("INSERT INTO jobs(kind, status, created_at) VALUES ('scan', 'running', 'x')", [])
            .unwrap();
        let r = JobRunner::start(db, Arc::new(EventBus::new()), 1).unwrap();
        assert_eq!(r.get(1).unwrap().status, JobState::Interrupted);
    }
}
