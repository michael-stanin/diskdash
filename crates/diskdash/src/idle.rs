//! Stop the server when the browser page has been closed (`--exit-when-idle`).
//!
//! The page keeps one event stream open to the server. When the last page is
//! closed that stream ends, so "no clients for a while" means "nobody is looking".
//! The server never stops while a scan or another job is running.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use crate::events::EventBus;
use crate::jobs::JobRunner;

/// How long to wait for the first page to connect after start-up before giving up.
pub const FIRST_CLIENT_WAIT: Duration = Duration::from_secs(120);
/// Default quiet time after the last page closed. Long enough to survive a reload or navigation.
pub const DEFAULT_IDLE: Duration = Duration::from_secs(30);

pub struct IdleWatcher {
    idle: Duration,
    first_client_wait: Duration,
    started: Instant,
    seen_client: bool,
    idle_since: Option<Instant>,
}

impl IdleWatcher {
    pub fn new(now: Instant, idle: Duration, first_client_wait: Duration) -> Self {
        Self { idle, first_client_wait, started: now, seen_client: false, idle_since: None }
    }

    /// Feed the current state; true means "stop now".
    pub fn tick(&mut self, now: Instant, clients: usize, busy: bool) -> bool {
        if clients > 0 {
            self.seen_client = true;
        }
        if clients > 0 || busy {
            self.idle_since = None;
            return false;
        }
        if !self.seen_client {
            // Nobody ever opened the page (the browser may be slow, or it was never opened).
            return now.duration_since(self.started) >= self.first_client_wait;
        }
        let since = *self.idle_since.get_or_insert(now);
        now.duration_since(since) >= self.idle
    }
}

/// Poll every two seconds and notify `stop` when the server should exit.
pub fn spawn(events: Arc<EventBus>, jobs: JobRunner, idle: Duration, stop: Arc<Notify>) {
    let first_wait = FIRST_CLIENT_WAIT.max(idle);
    tokio::spawn(async move {
        let mut w = IdleWatcher::new(Instant::now(), idle, first_wait);
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if w.tick(Instant::now(), events.subscriber_count(), jobs.active_jobs() > 0) {
                stop.notify_one();
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: Duration = Duration::from_secs(30);
    const FIRST: Duration = Duration::from_secs(120);

    fn at(t0: Instant, s: u64) -> Instant {
        t0 + Duration::from_secs(s)
    }

    #[test]
    fn stops_only_after_the_page_has_been_closed_for_the_whole_grace_period() {
        let t0 = Instant::now();
        let mut w = IdleWatcher::new(t0, IDLE, FIRST);
        assert!(!w.tick(at(t0, 1), 1, false), "a page is open");
        assert!(!w.tick(at(t0, 60), 1, false));
        assert!(!w.tick(at(t0, 62), 0, false), "just closed: the clock starts");
        assert!(!w.tick(at(t0, 91), 0, false), "29 s later");
        assert!(w.tick(at(t0, 92), 0, false), "30 s with nobody there");
    }

    #[test]
    fn a_reload_or_a_second_tab_resets_the_clock() {
        let t0 = Instant::now();
        let mut w = IdleWatcher::new(t0, IDLE, FIRST);
        w.tick(at(t0, 1), 1, false);
        assert!(!w.tick(at(t0, 10), 0, false));
        assert!(!w.tick(at(t0, 25), 1, false), "the page came back (a reload)");
        assert!(!w.tick(at(t0, 40), 0, false));
        assert!(!w.tick(at(t0, 69), 0, false), "counted from the second close");
        assert!(w.tick(at(t0, 70), 0, false));
    }

    #[test]
    fn it_never_stops_while_a_job_is_running() {
        let t0 = Instant::now();
        let mut w = IdleWatcher::new(t0, IDLE, FIRST);
        w.tick(at(t0, 1), 1, false);
        for s in [10, 100, 1000, 5000] {
            assert!(!w.tick(at(t0, s), 0, true), "a scan is running at {s} s");
        }
        // When the job ends the grace period starts from then, not from when the page closed.
        assert!(!w.tick(at(t0, 5001), 0, false));
        assert!(!w.tick(at(t0, 5030), 0, false));
        assert!(w.tick(at(t0, 5031), 0, false));
    }

    #[test]
    fn when_no_page_ever_connects_it_waits_longer_before_giving_up() {
        let t0 = Instant::now();
        let mut w = IdleWatcher::new(t0, IDLE, FIRST);
        assert!(!w.tick(at(t0, 30), 0, false), "the browser may still be starting");
        assert!(!w.tick(at(t0, 119), 0, false));
        assert!(w.tick(at(t0, 120), 0, false));
        // But a running job keeps it alive even then.
        let mut w = IdleWatcher::new(t0, IDLE, FIRST);
        assert!(!w.tick(at(t0, 500), 0, true));
    }
}
