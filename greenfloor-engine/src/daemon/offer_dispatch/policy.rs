//! Parallel offer-dispatch policy for the daemon strategy phase.

use crate::config::ManagerProgramConfig;

#[must_use]
pub(super) fn parallel_managed_dispatch_enabled(program: &ManagerProgramConfig) -> bool {
    program.runtime_offer_parallelism_enabled && !program.runtime_dry_run
}

#[must_use]
pub(super) fn parallel_max_workers(submission_count: usize, configured_max: usize) -> usize {
    submission_count.min(configured_max.max(1))
}

#[must_use]
pub(super) fn reservation_release_status(is_executed: bool) -> &'static str {
    if is_executed {
        "released_success"
    } else {
        "released_failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ManagerProgramConfig;

    #[test]
    fn parallel_managed_dispatch_enabled_requires_parallelism_and_live_runtime() {
        let mut program = ManagerProgramConfig {
            runtime_market_slot_count: 1,
            runtime_offer_parallelism_enabled: true,
            runtime_offer_parallelism_max_workers: 2,
            tx_block_websocket_reconnect_interval_seconds: 1,
            tx_block_fallback_poll_interval_seconds: 1,
            ..Default::default()
        };
        assert!(parallel_managed_dispatch_enabled(&program));
        program.runtime_offer_parallelism_enabled = false;
        assert!(!parallel_managed_dispatch_enabled(&program));
        program.runtime_offer_parallelism_enabled = true;
        program.runtime_dry_run = true;
        assert!(!parallel_managed_dispatch_enabled(&program));
    }

    #[test]
    fn parallel_max_workers_caps_at_submission_count() {
        assert_eq!(parallel_max_workers(3, 8), 3);
        assert_eq!(parallel_max_workers(0, 0), 0);
        assert_eq!(parallel_max_workers(5, 0), 1);
    }
}
