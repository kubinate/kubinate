//! Cluster-scoped event hub used by the SSE endpoint
//! (Sprint 3 ticket 10).
//!
//! Workflow progress is already mirrored into the
//! `provisioning_workflows` shadow table; the SSE flow needs the same
//! transitions surfaced as in-process pushes so a connected browser
//! can re-render in <1s instead of waiting for the next 2s poll.
//!
//! Shape:
//! - One `tokio::sync::broadcast` channel per active cluster id.
//! - `publish(cluster_id, event)` is a no-op when no one is
//!   subscribed — the workflow keeps running normally; the lookup
//!   stays cheap because the entry is dropped as soon as the last
//!   receiver disconnects.
//! - `subscribe(cluster_id)` is the API edge's hook; it returns a
//!   receiver whose `recv()` future resolves with the next event.
//!
//! The hub deliberately lives in the `workflows` crate so the runner
//! can publish without importing API types. The HTTP encoding lives
//! in `kubinate-api`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

/// Cluster lifecycle event surfaced to SSE subscribers.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClusterEvent {
    /// Workflow advanced to a new step (mirrors the shadow-table
    /// `current_step` write).
    Step {
        /// Human-readable step slug (e.g. `installing_k3s_server`).
        step: String,
    },
    /// Workflow reached a terminal state and is closing the stream.
    Terminal {
        /// Final cluster status (`ready` / `failed` / `destroyed`).
        status: String,
        /// Categorised error reason when `status == failed`. `None`
        /// for happy-path terminals.
        error_category: Option<String>,
    },
}

/// Per-cluster broadcaster. Cheap to clone; intended to be held in an
/// `Arc` and shared between the runner (publishes) and the API edge
/// (subscribes).
#[derive(Default)]
pub struct ClusterEventHub {
    // A `DashMap` would let us avoid the Mutex, but `broadcast::Sender`
    // is already `Sync` and the per-cluster traffic is low enough that
    // a Mutex around the map is not a hot path. Keeping `std::sync`
    // out of `tokio` lets the publisher path be sync (no `await`).
    inner: Mutex<HashMap<Uuid, broadcast::Sender<ClusterEvent>>>,
    /// Channel capacity — both the per-cluster history depth and the
    /// max lag a slow client can tolerate before it gets a `Lagged`
    /// error and is dropped. 64 covers the longest provision (~10
    /// transitions) plus burst on terminal.
    capacity: usize,
}

impl ClusterEventHub {
    /// Build a hub with a sensible default channel capacity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            capacity: 64,
        }
    }

    /// Publish an event for a cluster. Cheap when no one is
    /// subscribed: the only cost is a `HashMap::get`. Returns the
    /// number of receivers that observed the event (0 if none).
    ///
    /// # Panics
    /// Panics if the internal mutex is poisoned (only possible after a
    /// previous thread panicked while holding the lock).
    pub fn publish(&self, cluster_id: Uuid, event: ClusterEvent) -> usize {
        let mut guard = self.inner.lock().expect("hub mutex poisoned");
        let Some(sender) = guard.get(&cluster_id) else {
            return 0;
        };
        // `send` errors only when no receivers exist; clean up the
        // entry so the next publish doesn't keep paying for a dead
        // sender. Subscribers come and go on every page navigation,
        // so this matters in practice.
        if let Ok(n) = sender.send(event) {
            n
        } else {
            guard.remove(&cluster_id);
            0
        }
    }

    /// Subscribe to a cluster's event stream. The first subscriber
    /// for a cluster id allocates the channel; subsequent ones share
    /// it. The receiver only sees events published *after* this
    /// call — initial state is fetched separately by the SSE handler.
    ///
    /// # Panics
    /// Panics if the internal mutex is poisoned (only possible after a
    /// previous thread panicked while holding the lock).
    pub fn subscribe(&self, cluster_id: Uuid) -> broadcast::Receiver<ClusterEvent> {
        let mut guard = self.inner.lock().expect("hub mutex poisoned");
        let sender = guard
            .entry(cluster_id)
            .or_insert_with(|| broadcast::channel(self.capacity).0);
        sender.subscribe()
    }
}

/// Convenience constructor.
#[must_use]
pub fn shared_hub() -> Arc<ClusterEventHub> {
    Arc::new(ClusterEventHub::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscriber_receives_published_step() {
        let hub = ClusterEventHub::new();
        let id = Uuid::now_v7();
        let mut rx = hub.subscribe(id);

        let observers = hub.publish(
            id,
            ClusterEvent::Step {
                step: "installing_k3s_server".into(),
            },
        );
        assert_eq!(observers, 1);

        let event = rx.recv().await.expect("event");
        match event {
            ClusterEvent::Step { step } => assert_eq!(step, "installing_k3s_server"),
            ClusterEvent::Terminal { status, .. } => {
                panic!("unexpected Terminal event: {status}")
            }
        }
    }

    #[tokio::test]
    async fn publish_with_no_subscribers_is_a_noop() {
        let hub = ClusterEventHub::new();
        let id = Uuid::now_v7();
        let observers = hub.publish(id, ClusterEvent::Step { step: "x".into() });
        assert_eq!(observers, 0);
    }

    #[tokio::test]
    async fn distinct_clusters_do_not_cross_publish() {
        let hub = ClusterEventHub::new();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let mut rx_a = hub.subscribe(a);
        let mut rx_b = hub.subscribe(b);

        hub.publish(
            a,
            ClusterEvent::Step {
                step: "for-a".into(),
            },
        );
        let evt = rx_a.recv().await.unwrap();
        assert!(matches!(evt, ClusterEvent::Step { ref step } if step == "for-a"));

        // `b` should not have received anything; recv would block, so
        // use try_recv to assert.
        assert!(rx_b.try_recv().is_err());
    }

    #[tokio::test]
    async fn terminal_event_serializes_with_status_field() {
        let evt = ClusterEvent::Terminal {
            status: "failed".into(),
            error_category: Some("provider_error".into()),
        };
        let json = serde_json::to_value(&evt).unwrap();
        assert_eq!(json["type"], "terminal");
        assert_eq!(json["status"], "failed");
        assert_eq!(json["error_category"], "provider_error");
    }
}
