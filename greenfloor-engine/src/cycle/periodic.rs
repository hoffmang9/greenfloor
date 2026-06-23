use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Shared monotonic clock for daemon periodic tasks.
#[must_use]
pub fn monotonic_seconds() -> f64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_secs_f64()
}

#[must_use]
pub fn is_periodic_due(now_monotonic: f64, next_deadline: f64) -> bool {
    next_deadline <= now_monotonic
}

#[must_use]
pub fn next_periodic_deadline(now_monotonic: f64, interval_seconds: u64) -> f64 {
    now_monotonic + crate::offer::pricing::u64_to_f64(interval_seconds.max(1))
}

/// Runs `task` at most once per `interval_seconds` of monotonic time.
pub struct PeriodicGate {
    next_deadline: Mutex<Option<f64>>,
}

impl PeriodicGate {
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_deadline: Mutex::new(None),
        }
    }

    /// Runs `task` when due. Advances the next deadline only when `task` returns `true`.
    pub fn run_if_due(&self, interval_seconds: u64, task: impl FnOnce() -> bool) {
        let now = monotonic_seconds();
        let Ok(mut next_deadline) = self.next_deadline.lock() else {
            return;
        };
        let deadline = next_deadline.unwrap_or(0.0);
        if !is_periodic_due(now, deadline) {
            return;
        }
        if task() {
            *next_deadline = Some(next_periodic_deadline(now, interval_seconds));
        }
    }

    pub fn seed_next_deadline(&self, interval_seconds: u64) {
        let now = monotonic_seconds();
        if let Ok(mut next_deadline) = self.next_deadline.lock() {
            *next_deadline = Some(next_periodic_deadline(now, interval_seconds));
        }
    }
}

impl Default for PeriodicGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periodic_gate_runs_once_until_interval_elapses() {
        let gate = PeriodicGate::new();
        let mut runs = 0_u8;
        gate.run_if_due(3600, || {
            runs += 1;
            true
        });
        gate.run_if_due(3600, || {
            runs += 1;
            true
        });
        assert_eq!(runs, 1);
    }

    #[test]
    fn periodic_gate_does_not_advance_deadline_when_task_returns_false() {
        let gate = PeriodicGate::new();
        let mut runs = 0_u8;
        gate.run_if_due(3600, || false);
        gate.run_if_due(3600, || {
            runs += 1;
            true
        });
        assert_eq!(runs, 1);
    }
}
