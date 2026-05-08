//! Tiny in-memory sliding-window rate limiter.
//!
//! Per-instance only — adequate for Phase 0/1 single-instance dev and
//! the single-VPS production topology of ADR-0004. Once we run
//! multiple API replicas the limits move to Redis (which is already
//! provisioned by `docker compose`).

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use uuid::Uuid;

/// A sliding-window counter per `Uuid` key.
pub struct RateLimiter {
    window: Duration,
    max_per_window: usize,
    state: Mutex<HashMap<Uuid, VecDeque<Instant>>>,
}

impl RateLimiter {
    /// Build a limiter that admits at most `max_per_window` calls
    /// within `window`.
    #[must_use]
    pub fn new(max_per_window: usize, window: Duration) -> Self {
        Self {
            window,
            max_per_window,
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Record a call for `key` and return whether it was admitted.
    /// Out-of-window samples are pruned on every call so the map
    /// cannot grow unbounded without sustained traffic.
    pub fn check(&self, key: Uuid) -> bool {
        self.check_at(key, Instant::now())
    }

    /// Time-injectable variant for unit tests.
    pub fn check_at(&self, key: Uuid, now: Instant) -> bool {
        let mut state = self.state.lock().expect("rate-limiter mutex");
        let entry = state.entry(key).or_default();
        let cutoff = now.checked_sub(self.window).unwrap_or(now);
        while entry.front().is_some_and(|t| *t < cutoff) {
            entry.pop_front();
        }
        if entry.len() >= self.max_per_window {
            return false;
        }
        entry.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_up_to_the_limit_then_rejects() {
        let rl = RateLimiter::new(3, Duration::from_secs(60));
        let key = Uuid::now_v7();
        let t0 = Instant::now();
        assert!(rl.check_at(key, t0));
        assert!(rl.check_at(key, t0));
        assert!(rl.check_at(key, t0));
        assert!(!rl.check_at(key, t0));
    }

    #[test]
    fn slides_window_forward() {
        let rl = RateLimiter::new(2, Duration::from_secs(60));
        let key = Uuid::now_v7();
        let t0 = Instant::now();
        assert!(rl.check_at(key, t0));
        assert!(rl.check_at(key, t0));
        assert!(!rl.check_at(key, t0));
        // 61 seconds later the earlier samples are out of window.
        let t1 = t0 + Duration::from_secs(61);
        assert!(rl.check_at(key, t1));
    }

    #[test]
    fn keys_are_independent() {
        let rl = RateLimiter::new(1, Duration::from_secs(60));
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let t = Instant::now();
        assert!(rl.check_at(a, t));
        assert!(!rl.check_at(a, t));
        assert!(rl.check_at(b, t));
    }
}
