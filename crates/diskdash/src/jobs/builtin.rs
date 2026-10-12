//! Built-in job kinds used for testing the runner end to end.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{Job, JobContext, JobError};

/// Sleeps for `duration_ms`, reporting progress in milliseconds. Dev/test only.
pub struct NoopSleep {
    pub duration_ms: u64,
    pub lock: Option<PathBuf>,
}

impl Job for NoopSleep {
    fn kind(&self) -> &'static str {
        "noop-sleep"
    }

    fn params(&self) -> serde_json::Value {
        serde_json::json!({ "duration_ms": self.duration_ms, "lock": self.lock })
    }

    fn locks(&self) -> Vec<PathBuf> {
        self.lock.iter().cloned().collect()
    }

    fn run(&mut self, ctx: &JobContext) -> Result<(), JobError> {
        let start = Instant::now();
        let total = Duration::from_millis(self.duration_ms);
        loop {
            ctx.check_cancelled()?;
            let elapsed = start.elapsed().min(total);
            ctx.progress(elapsed.as_millis() as u64, Some(self.duration_ms), "ms", Some("sleeping"));
            if elapsed >= total {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20).min(total - elapsed));
        }
    }
}

/// Always panics. Used to prove panics are contained.
pub struct PanicJob;

impl Job for PanicJob {
    fn kind(&self) -> &'static str {
        "noop-panic"
    }

    fn run(&mut self, _ctx: &JobContext) -> Result<(), JobError> {
        panic!("boom");
    }
}
