//! Server-sent events: one multiplexed stream for all jobs, with a replay
//! buffer so a reconnecting client (`Last-Event-ID`) misses nothing.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Mutex;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{self, Stream, StreamExt};
use serde::Serialize;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use ts_rs::TS;

pub const REPLAY_CAPACITY: usize = 1000;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ServerEvent {
    #[ts(type = "number")]
    pub id: u64,
    /// e.g. `job.queued`, `job.started`, `job.progress`, `job.finished`.
    #[serde(rename = "type")]
    pub kind: String,
    #[ts(type = "unknown")]
    pub data: serde_json::Value,
}

pub struct EventBus {
    tx: broadcast::Sender<ServerEvent>,
    state: Mutex<BusState>,
}

struct BusState {
    next_id: u64,
    replay: VecDeque<ServerEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx, state: Mutex::new(BusState { next_id: 1, replay: VecDeque::with_capacity(REPLAY_CAPACITY) }) }
    }

    /// Publish an event to all subscribers and the replay buffer.
    pub fn publish(&self, kind: &str, data: impl Serialize) {
        let data = serde_json::to_value(data).unwrap_or(serde_json::Value::Null);
        let Ok(mut st) = self.state.lock() else { return };
        let event = ServerEvent { id: st.next_id, kind: kind.to_string(), data };
        st.next_id += 1;
        if st.replay.len() == REPLAY_CAPACITY {
            st.replay.pop_front();
        }
        st.replay.push_back(event.clone());
        // Send while holding the lock so subscribe+replay cannot miss or duplicate.
        let _ = self.tx.send(event);
    }

    /// Events after `last_id` still in the buffer, plus a live receiver.
    pub fn subscribe(&self, last_id: Option<u64>) -> (Vec<ServerEvent>, broadcast::Receiver<ServerEvent>) {
        let st = match self.state.lock() {
            Ok(st) => st,
            Err(poisoned) => poisoned.into_inner(),
        };
        let rx = self.tx.subscribe();
        let missed = match last_id {
            Some(last) => st.replay.iter().filter(|e| e.id > last).cloned().collect(),
            None => Vec::new(),
        };
        (missed, rx)
    }

    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

fn to_sse(e: ServerEvent) -> Event {
    Event::default().id(e.id.to_string()).event(e.kind.clone()).data(serde_json::to_string(&e).unwrap_or_default())
}

/// Build the SSE response for one client.
pub fn sse_response(bus: &EventBus, last_id: Option<u64>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (missed, rx) = bus.subscribe(last_id);
    let replay = stream::iter(missed.into_iter().map(|e| Ok(to_sse(e))));
    // A lagging client skips what it missed; it can refetch state via the API.
    let live = BroadcastStream::new(rx).filter_map(|r| async move { r.ok().map(|e| Ok(to_sse(e))) });
    Sse::new(replay.chain(live)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_after_reconnect_returns_missed_events_in_order() {
        let bus = EventBus::new();
        for i in 0..5 {
            bus.publish("job.progress", i);
        }
        let (missed, _rx) = bus.subscribe(Some(2));
        let ids: Vec<u64> = missed.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![3, 4, 5]);
    }

    #[test]
    fn replay_buffer_is_bounded() {
        let bus = EventBus::new();
        for i in 0..(REPLAY_CAPACITY + 10) {
            bus.publish("x", i);
        }
        let (missed, _rx) = bus.subscribe(Some(0));
        assert_eq!(missed.len(), REPLAY_CAPACITY);
        assert_eq!(missed[0].id, 11);
    }

    #[tokio::test]
    async fn live_subscribers_receive_new_events() {
        let bus = EventBus::new();
        let (missed, mut rx) = bus.subscribe(None);
        assert!(missed.is_empty());
        bus.publish("job.started", serde_json::json!({"id": 1}));
        let e = rx.recv().await.unwrap();
        assert_eq!(e.kind, "job.started");
    }
}
