//! Circuit breaker state machine for tools and providers (task #41).
//!
//! A `CircuitBreaker` tracks consecutive failures and trips to the `Open`
//! state when the threshold is exceeded, preventing further calls until the
//! reset window elapses and the breaker enters `HalfOpen` for one probe.
//!
//! # States
//!
//! ```text
//!   Closed  --[failures >= threshold]-->  Open
//!   Open    --[reset_secs elapsed]----->  HalfOpen
//!   HalfOpen--[success]--------------->  Closed
//!   HalfOpen--[failure]--------------->  Open
//! ```

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// CircuitState — internal + serializable snapshot
// ---------------------------------------------------------------------------

/// Internal state that holds an `Instant` (not serializable).
enum CircuitState {
    Closed,
    Open { opened_at: Instant },
    HalfOpen,
}

impl CircuitState {
    fn snapshot(&self) -> CircuitStateSnapshot {
        match self {
            Self::Closed => CircuitStateSnapshot::Closed,
            Self::Open { .. } => CircuitStateSnapshot::Open,
            Self::HalfOpen => CircuitStateSnapshot::HalfOpen,
        }
    }
}

/// Serializable snapshot of circuit state — safe to log, emit as an event,
/// or return to callers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitStateSnapshot {
    Closed,
    Open,
    HalfOpen,
}

impl std::fmt::Display for CircuitStateSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "closed"),
            Self::Open => write!(f, "open"),
            Self::HalfOpen => write!(f, "half_open"),
        }
    }
}

// ---------------------------------------------------------------------------
// CircuitBreaker
// ---------------------------------------------------------------------------

/// A shared, thread-safe circuit breaker.
///
/// Clone is cheap (internally backed by `Arc`s).
pub struct CircuitBreaker {
    name: String,
    state: Arc<Mutex<CircuitState>>,
    failure_threshold: u32,
    reset_duration: Duration,
    consecutive_failures: Arc<Mutex<u32>>,
}

impl CircuitBreaker {
    /// Create a new breaker in the `Closed` state.
    ///
    /// - `failure_threshold`: consecutive failures before opening.
    /// - `reset_secs`: seconds to wait in `Open` before moving to `HalfOpen`.
    pub fn new(name: impl Into<String>, failure_threshold: u32, reset_secs: u64) -> Self {
        Self {
            name: name.into(),
            state: Arc::new(Mutex::new(CircuitState::Closed)),
            failure_threshold,
            reset_duration: Duration::from_secs(reset_secs),
            consecutive_failures: Arc::new(Mutex::new(0)),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns `true` when calls SHOULD be rejected (breaker is `Open` and
    /// the reset window has not elapsed yet).
    ///
    /// Automatically transitions `Open → HalfOpen` when the reset window
    /// has elapsed, so the next probe can be attempted.
    pub fn is_open(&self) -> bool {
        let mut state = self.state.lock().expect("circuit_breaker lock poisoned");
        if let CircuitState::Open { opened_at } = &*state {
            if opened_at.elapsed() >= self.reset_duration {
                *state = CircuitState::HalfOpen;
                tracing::info!(breaker = %self.name, "circuit breaker → half_open");
                return false;
            }
            return true;
        }
        false
    }

    /// Snapshot the current state without side effects.
    pub fn state_snapshot(&self) -> CircuitStateSnapshot {
        self.state
            .lock()
            .expect("circuit_breaker lock poisoned")
            .snapshot()
    }

    /// Record a successful call.
    ///
    /// - Resets the failure counter.
    /// - Transitions `HalfOpen → Closed`.
    pub fn record_success(&self) {
        let mut state = self.state.lock().expect("circuit_breaker lock poisoned");
        let mut failures = self
            .consecutive_failures
            .lock()
            .expect("circuit_breaker lock poisoned");
        *failures = 0;
        if matches!(*state, CircuitState::HalfOpen) {
            *state = CircuitState::Closed;
            tracing::info!(breaker = %self.name, "circuit breaker → closed (recovered)");
        }
    }

    /// Record a failed call. Returns the new state snapshot.
    ///
    /// - Increments the failure counter when `Closed`.
    /// - Opens the breaker when the threshold is reached.
    /// - Immediately re-opens from `HalfOpen` on a probe failure.
    pub fn record_failure(&self) -> CircuitStateSnapshot {
        let mut state = self.state.lock().expect("circuit_breaker lock poisoned");
        let mut failures = self
            .consecutive_failures
            .lock()
            .expect("circuit_breaker lock poisoned");

        match &*state {
            CircuitState::HalfOpen => {
                *state = CircuitState::Open {
                    opened_at: Instant::now(),
                };
                tracing::warn!(breaker = %self.name, "circuit breaker → open (probe failed)");
            }
            CircuitState::Closed => {
                *failures += 1;
                if *failures >= self.failure_threshold {
                    *state = CircuitState::Open {
                        opened_at: Instant::now(),
                    };
                    *failures = 0;
                    tracing::warn!(
                        breaker    = %self.name,
                        threshold  = self.failure_threshold,
                        "circuit breaker → open (threshold reached)"
                    );
                }
            }
            CircuitState::Open { .. } => {} // already open
        }

        state.snapshot()
    }
}

impl Clone for CircuitBreaker {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            state: Arc::clone(&self.state),
            failure_threshold: self.failure_threshold,
            reset_duration: self.reset_duration,
            consecutive_failures: Arc::clone(&self.consecutive_failures),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_starts_closed() {
        let cb = CircuitBreaker::new("test", 3, 60);
        assert!(!cb.is_open());
        assert_eq!(cb.state_snapshot(), CircuitStateSnapshot::Closed);
    }

    #[test]
    fn test_opens_at_threshold() {
        let cb = CircuitBreaker::new("test", 2, 60);
        cb.record_failure();
        assert_eq!(cb.state_snapshot(), CircuitStateSnapshot::Closed);
        cb.record_failure();
        assert_eq!(cb.state_snapshot(), CircuitStateSnapshot::Open);
        assert!(cb.is_open());
    }

    #[test]
    fn test_success_resets_failures() {
        let cb = CircuitBreaker::new("test", 3, 60);
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        cb.record_failure();
        // After reset, 1 failure should not open (threshold is 3)
        assert_eq!(cb.state_snapshot(), CircuitStateSnapshot::Closed);
    }

    #[test]
    fn test_half_open_to_closed_on_success() {
        let cb = CircuitBreaker::new("test", 1, 0); // 0s reset — instant
        cb.record_failure(); // → Open
                             // is_open() triggers Open→HalfOpen because reset_duration=0
        assert!(!cb.is_open()); // should be HalfOpen now, returning false
        assert_eq!(cb.state_snapshot(), CircuitStateSnapshot::HalfOpen);
        cb.record_success(); // HalfOpen → Closed
        assert_eq!(cb.state_snapshot(), CircuitStateSnapshot::Closed);
    }

    #[test]
    fn test_half_open_to_open_on_failure() {
        let cb = CircuitBreaker::new("test", 1, 0);
        cb.record_failure(); // → Open
        cb.is_open(); // triggers → HalfOpen
        cb.record_failure(); // HalfOpen → Open
        assert_eq!(cb.state_snapshot(), CircuitStateSnapshot::Open);
    }
}
