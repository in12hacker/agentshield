//! One original monotonic review window; queueing never restarts it.
use std::time::Duration;
use tokio::time::Instant;
/// Invalid original source window.
#[derive(Debug, thiserror::Error)]
pub enum WindowError {
    /// No useful lifetime was supplied.
    #[error("review budget must be nonzero")]
    Empty,
    /// The deadline cannot be represented.
    #[error("review deadline out of range")]
    Range,
}
/// Immutable lifetime captured by the original request source.
#[derive(Debug, Clone, Copy)]
pub struct HoldWindow {
    started: Instant,
    deadline: Instant,
}
impl HoldWindow {
    /// Adopt an already captured source window without restarting its lifetime.
    pub fn from_source(started: Instant, deadline: Instant) -> Result<Self, WindowError> {
        if deadline <= started {
            return Err(WindowError::Empty);
        }
        Ok(Self { started, deadline })
    }
    /// Capture the original source time once, before admission or matching.
    pub fn begin(budget: Duration) -> Result<Self, WindowError> {
        if budget.is_zero() {
            return Err(WindowError::Empty);
        }
        let started = Instant::now();
        let deadline = started.checked_add(budget).ok_or(WindowError::Range)?;
        Ok(Self { started, deadline })
    }
    /// Original capture time, unchanged across stages.
    pub fn started_at(self) -> Instant {
        self.started
    }
    /// Original deadline, unchanged by queueing.
    pub fn deadline(self) -> Instant {
        self.deadline
    }
}
