//! Background jobs: long work runs on worker threads, never on the async
//! request path. Jobs report throttled progress, can be cancelled, take path
//! locks, and a panic inside a job fails only that job.

pub mod builtin;
pub mod locks;
pub mod runner;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use runner::JobRunner;

/// Minimum interval between progress events for one job (≤ 4 per second).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Succeeded => "succeeded",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
            JobState::Interrupted => "interrupted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => JobState::Queued,
            "running" => JobState::Running,
            "succeeded" => JobState::Succeeded,
            "failed" => JobState::Failed,
            "cancelled" => JobState::Cancelled,
            "interrupted" => JobState::Interrupted,
            _ => return None,
        })
    }

    pub fn is_finished(self) -> bool {
        !matches!(self, JobState::Queued | JobState::Running)
    }
}

/// Public view of a job (API + events).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct JobStatus {
    #[ts(type = "number")]
    pub id: u64,
    pub kind: String,
    pub status: JobState,
    pub phase: Option<String>,
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number | null")]
    pub total: Option<u64>,
    pub unit: Option<String>,
    #[ts(type = "number | null")]
    pub eta_seconds: Option<u64>,
    pub error: Option<String>,
    /// Set while queued behind a job holding an overlapping path lock.
    #[ts(type = "number | null")]
    pub waiting_for: Option<u64>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

/// A unit of background work.
pub trait Job: Send + 'static {
    fn kind(&self) -> &'static str;
    fn params(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    /// Paths this job needs exclusively (see [`locks`]).
    fn locks(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    fn run(&mut self, ctx: &JobContext) -> Result<(), JobError>;
}

/// Handed to a running job.
pub struct JobContext {
    pub(crate) id: u64,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) reporter: Arc<dyn ProgressSink>,
    pub(crate) started: Instant,
    pub(crate) last_emit: Mutex<Option<Instant>>,
}

pub(crate) trait ProgressSink: Send + Sync {
    fn progress(&self, id: u64, p: Progress);
}

#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub done: u64,
    pub total: Option<u64>,
    pub unit: Option<String>,
    pub phase: Option<String>,
    pub eta_seconds: Option<u64>,
}

impl JobContext {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// `Err(Cancelled)` once cancellation was requested; use with `?`.
    pub fn check_cancelled(&self) -> Result<(), JobError> {
        if self.is_cancelled() {
            Err(JobError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Report progress. Calls are cheap; events are throttled to ≤ 4/s per job
    /// (the final `done == total` update always goes out).
    pub fn progress(&self, done: u64, total: Option<u64>, unit: &str, phase: Option<&str>) {
        let now = Instant::now();
        let complete = total.is_some_and(|t| done >= t);
        {
            let Ok(mut last) = self.last_emit.lock() else { return };
            if let Some(prev) = *last {
                if !complete && now.duration_since(prev) < PROGRESS_INTERVAL {
                    return;
                }
            }
            *last = Some(now);
        }
        let eta_seconds = eta(self.started.elapsed(), done, total);
        self.reporter.progress(
            self.id,
            Progress { done, total, unit: Some(unit.to_string()), phase: phase.map(String::from), eta_seconds },
        );
    }
}

/// Linear ETA from the rate so far; `None` without a total or any progress.
pub fn eta(elapsed: Duration, done: u64, total: Option<u64>) -> Option<u64> {
    let total = total?;
    if done == 0 || done > total {
        return None;
    }
    let remaining = (total - done) as f64;
    Some((elapsed.as_secs_f64() * remaining / done as f64).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Count(Mutex<Vec<Progress>>);
    impl ProgressSink for Count {
        fn progress(&self, _id: u64, p: Progress) {
            self.0.lock().unwrap().push(p);
        }
    }

    fn ctx(sink: Arc<Count>) -> JobContext {
        JobContext {
            id: 1,
            cancel: Arc::new(AtomicBool::new(false)),
            reporter: sink,
            started: Instant::now(),
            last_emit: Mutex::new(None),
        }
    }

    #[test]
    fn progress_is_throttled() {
        let sink = Arc::new(Count::default());
        let c = ctx(sink.clone());
        let start = Instant::now();
        let mut i = 0;
        while start.elapsed() < Duration::from_millis(600) {
            c.progress(i, Some(1_000_000_000), "items", None);
            i += 1;
        }
        let n = sink.0.lock().unwrap().len();
        assert!(i > 1000, "loop should call progress many times");
        assert!((2..=4).contains(&n), "expected ≤ 4 events/s, got {n} in 0.6 s");
    }

    #[test]
    fn final_progress_always_emitted() {
        let sink = Arc::new(Count::default());
        let c = ctx(sink.clone());
        c.progress(1, Some(10), "x", None);
        c.progress(10, Some(10), "x", None);
        assert_eq!(sink.0.lock().unwrap().last().unwrap().done, 10);
    }

    #[test]
    fn eta_math() {
        assert_eq!(eta(Duration::from_secs(10), 40, Some(100)), Some(15));
        assert_eq!(eta(Duration::from_secs(10), 0, Some(100)), None);
        assert_eq!(eta(Duration::from_secs(10), 5, None), None);
        assert_eq!(eta(Duration::from_secs(10), 100, Some(100)), Some(0));
    }
}
