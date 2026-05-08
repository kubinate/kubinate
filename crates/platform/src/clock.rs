//! Injectable clock abstraction for deterministic tests.

use async_trait::async_trait;
use time::OffsetDateTime;

/// A source of the current time. Production code uses [`SystemClock`];
/// tests inject a mock.
#[async_trait]
pub trait Clock: Send + Sync {
    /// Current UTC instant.
    fn now(&self) -> OffsetDateTime;
}

/// Wall-clock implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

#[async_trait]
impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}
