//! Per-project broadcast fan-out for the WS Session edge (POL-S4 / MD-10).
//!
//! Each project owns one `tokio::sync::broadcast::Sender<BroadcastEvent>`.
//! Open Sessions on that project subscribe at upgrade time; their `Receiver`
//! lives for the duration of the socket loop and is dropped when the socket
//! closes (the broadcast crate's natural unsubscribe).
//!
//! `tokio::sync::broadcast` is intentional: it does not replay history to
//! late subscribers (a Session opened *after* an event was emitted will not
//! see that event), which is exactly the ISC-012 contract — Sessions are
//! not durable, the elicitation log is.
//!
//! The Sender stays in the registry across Session open/close cycles. A
//! channel with zero current subscribers is harmless — `send()` returns
//! `Err(SendError)` which the caller ignores. We keep the entry rather than
//! garbage-collecting so the project's Sender identity stays stable; the
//! cost is one `broadcast::Sender` per project, which is negligible at v1
//! scale (one session per project, ISC-015).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

const CHANNEL_CAPACITY: usize = 64;

/// What a Workflow-side writer hands to the Session edge for broadcast.
///
/// Names are deliberately *internal* — the WS forwarder maps these to the
/// wire shape (an existing `ServerMessage::CommandAccepted` variant for the
/// first leg, raw JSON mirroring the State Update record for the second).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BroadcastEvent {
    /// POL-S4 stage 1: command validated and queued. Goes to every current
    /// subscriber; at v1 ISC-015 guarantees that's exactly the issuing
    /// Session.
    CommandAccepted {
        request_id: String,
        command: String,
        at: String,
    },
    /// POL-S4 stage 2: a State Update has committed to the log. Mirror of
    /// the on-disk record's `event` field (e.g. `slice.transitioned`,
    /// `cohort.dispatched`, `workflow.escalated`, `slice.blocked`).
    StateUpdated {
        request_id: String,
        slice_id: Option<String>,
        event: String,
        at: String,
    },
}

/// Project-id → broadcast Sender. Cheap to clone (Arc inside).
#[derive(Clone, Default)]
pub struct ProjectBroadcaster {
    inner: Arc<Mutex<HashMap<String, broadcast::Sender<BroadcastEvent>>>>,
}

impl ProjectBroadcaster {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe a Session to a project's event stream. Idempotent w.r.t.
    /// the underlying Sender — the first subscriber for a project mints the
    /// channel; every later subscriber attaches to the same Sender.
    pub fn subscribe(&self, project_id: &str) -> broadcast::Receiver<BroadcastEvent> {
        let mut g = self.lock();
        g.entry(project_id.to_owned())
            .or_insert_with(|| broadcast::channel::<BroadcastEvent>(CHANNEL_CAPACITY).0)
            .subscribe()
    }

    /// Emit to every current subscriber on `project_id`. Returns the number
    /// of receivers the event reached, or 0 if no Session is listening (a
    /// CLI-only writer firing a command before any GUI is up — not an
    /// error, the State Update is still durable on disk).
    pub fn send(&self, project_id: &str, event: BroadcastEvent) -> usize {
        let g = self.lock();
        match g.get(project_id) {
            Some(tx) => tx.send(event).unwrap_or(0),
            None => 0,
        }
    }

    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<String, broadcast::Sender<BroadcastEvent>>> {
        match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }
}

#[cfg(test)]
mod self_tests {
    use super::*;

    #[tokio::test]
    async fn subscribe_then_send_round_trips() {
        let b = ProjectBroadcaster::new();
        let mut rx = b.subscribe("p");
        assert_eq!(
            b.send(
                "p",
                BroadcastEvent::CommandAccepted {
                    request_id: "r".into(),
                    command: "dispatch_next".into(),
                    at: "t".into(),
                }
            ),
            1
        );
        let got = rx.recv().await.expect("recv");
        match got {
            BroadcastEvent::CommandAccepted { command, .. } => assert_eq!(command, "dispatch_next"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[tokio::test]
    async fn no_subscriber_send_is_noop() {
        let b = ProjectBroadcaster::new();
        assert_eq!(
            b.send(
                "p",
                BroadcastEvent::StateUpdated {
                    request_id: "r".into(),
                    slice_id: None,
                    event: "slice.transitioned".into(),
                    at: "t".into(),
                }
            ),
            0
        );
    }

    #[tokio::test]
    async fn late_subscriber_does_not_see_prior_events() {
        // ISC-012: Sessions are not durable; no replay.
        let b = ProjectBroadcaster::new();
        let _early = b.subscribe("p"); // mint the channel
        let _ = b.send(
            "p",
            BroadcastEvent::CommandAccepted {
                request_id: "r1".into(),
                command: "dispatch_next".into(),
                at: "t1".into(),
            },
        );
        let mut late = b.subscribe("p");
        // Send a *second* event; late should see only this one.
        let _ = b.send(
            "p",
            BroadcastEvent::CommandAccepted {
                request_id: "r2".into(),
                command: "dispatch_next".into(),
                at: "t2".into(),
            },
        );
        let got = late.recv().await.expect("recv");
        match got {
            BroadcastEvent::CommandAccepted { request_id, .. } => assert_eq!(request_id, "r2"),
            other => panic!("late subscriber saw {other:?}"),
        }
        // And nothing more is pending.
        match tokio::time::timeout(std::time::Duration::from_millis(20), late.recv()).await {
            Err(_) => {} // expected — timeout, no replay
            Ok(other) => panic!("late subscriber got extra event: {other:?}"),
        }
    }
}
