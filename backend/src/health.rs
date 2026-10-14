//! Liveness signal for the BLE scanner, shared with the HTTP API.
//!
//! The scanner supervises itself and never returns, so from the outside a
//! scanner that is stuck retrying looks exactly like a healthy one. This
//! records what it last did so `GET /health` can tell them apart, and the
//! k3s liveness probe can restart a pod that has stopped collecting.

use std::sync::{Mutex, PoisonError};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Scanning,
    Retrying,
}

#[derive(Debug)]
struct Inner {
    last_event_at: Option<DateTime<Utc>>,
    last_reading_stored_at: Option<DateTime<Utc>>,
    session_state: SessionState,
    last_error: Option<String>,
    consecutive_failures: u32,
}

/// What the scanner last did, plus the policy that turns it into a verdict.
pub struct ScannerHealth {
    started_at: DateTime<Utc>,
    max_silence: Duration,
    inner: Mutex<Inner>,
}

/// Point-in-time view served by `GET /health`.
#[derive(Debug, Serialize)]
pub struct HealthSnapshot {
    pub healthy: bool,
    pub session_state: SessionState,
    pub last_event_at: Option<DateTime<Utc>>,
    pub last_reading_stored_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
}

impl ScannerHealth {
    pub fn new(started_at: DateTime<Utc>, max_silence: Duration) -> Self {
        Self {
            started_at,
            max_silence,
            inner: Mutex::new(Inner {
                last_event_at: None,
                last_reading_stored_at: None,
                session_state: SessionState::Retrying,
                last_error: None,
                consecutive_failures: 0,
            }),
        }
    }

    /// Any BLE advertisement event, SwitchBot or not. A session that
    /// delivers events is working, so this also ends a failure streak.
    pub fn record_event(&self) {
        let mut inner = self.lock();
        inner.last_event_at = Some(Utc::now());
        inner.consecutive_failures = 0;
    }

    pub fn record_reading_stored(&self) {
        self.lock().last_reading_stored_at = Some(Utc::now());
    }

    pub fn record_scan_started(&self) {
        self.lock().session_state = SessionState::Scanning;
    }

    /// A scan session ended. `error` is `None` when the event stream simply
    /// ended; every end counts as a failure until events flow again.
    pub fn record_session_ended(&self, error: Option<String>) {
        let mut inner = self.lock();
        inner.session_state = SessionState::Retrying;
        inner.consecutive_failures = inner.consecutive_failures.saturating_add(1);
        if error.is_some() {
            inner.last_error = error;
        }
    }

    /// Healthy while the scanner has seen a BLE event within `max_silence`,
    /// or the process is itself younger than that (startup grace period).
    ///
    /// "Any BLE event" rather than "a stored reading": a busy home sees
    /// dozens of devices, so total silence means the stack is broken, while
    /// one meter out of range or with a flat battery does not.
    pub fn snapshot(&self, now: DateTime<Utc>) -> HealthSnapshot {
        let inner = self.lock();
        let recently_started = now - self.started_at < self.max_silence;
        let recent_event = inner
            .last_event_at
            .is_some_and(|at| now - at < self.max_silence);
        HealthSnapshot {
            healthy: recently_started || recent_event,
            session_state: inner.session_state,
            last_event_at: inner.last_event_at,
            last_reading_stored_at: inner.last_reading_stored_at,
            last_error: inner.last_error.clone(),
            consecutive_failures: inner.consecutive_failures,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // The state is plain data with no invariants spanning fields, so a
        // panic elsewhere while holding the lock must not take /health down.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_SILENCE: i64 = 600;

    fn health_started(ago: Duration) -> ScannerHealth {
        ScannerHealth::new(Utc::now() - ago, Duration::seconds(MAX_SILENCE))
    }

    #[test]
    fn a_freshly_started_process_is_healthy_before_any_event() {
        let health = health_started(Duration::seconds(5));

        assert!(health.snapshot(Utc::now()).healthy);
    }

    #[test]
    fn a_process_past_the_grace_period_with_no_event_is_unhealthy() {
        let health = health_started(Duration::seconds(MAX_SILENCE + 1));

        assert!(!health.snapshot(Utc::now()).healthy);
    }

    #[test]
    fn a_recent_event_keeps_an_old_process_healthy() {
        let health = health_started(Duration::hours(3));
        health.record_event();

        assert!(health.snapshot(Utc::now()).healthy);
    }

    #[test]
    fn an_event_older_than_the_silence_window_is_unhealthy() {
        let health = health_started(Duration::hours(3));
        health.record_event();

        let later = Utc::now() + Duration::seconds(MAX_SILENCE + 1);
        assert!(!health.snapshot(later).healthy);
    }

    #[test]
    fn ending_a_session_counts_a_failure_and_events_reset_the_streak() {
        let health = health_started(Duration::hours(3));
        health.record_scan_started();
        health.record_session_ended(Some("adapter not found".to_string()));
        health.record_session_ended(None);

        let snapshot = health.snapshot(Utc::now());
        assert_eq!(snapshot.session_state, SessionState::Retrying);
        assert_eq!(snapshot.consecutive_failures, 2);
        // A clean stream end must not erase the error that explains it.
        assert_eq!(snapshot.last_error.as_deref(), Some("adapter not found"));

        health.record_event();
        assert_eq!(health.snapshot(Utc::now()).consecutive_failures, 0);
    }
}
