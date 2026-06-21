//! Test-only injection mappers for offer dispatch.
//!
//! Unit tests here cover the branch table. Integration wiring is exercised in
//! `tests/harness_tests.rs`, which asserts dispatch outcomes only.

use crate::daemon::dispatch_test_controls::{
    DaemonDispatchTestInjections, ManagedPostTestMode, ParallelDispatchTestMode,
};
use crate::error::{SignerError, SignerResult};

use super::OfferDispatchOutput;

pub(crate) fn parallel_dispatch_result(
    injections: &DaemonDispatchTestInjections,
) -> Option<SignerResult<OfferDispatchOutput>> {
    match injections.parallel? {
        ParallelDispatchTestMode::Transient => Some(Err(SignerError::ReservationContention(
            "test override".to_string(),
        ))),
        ParallelDispatchTestMode::Fatal => Some(Err(SignerError::Other(
            "permanent_offer_build_failure: test override".to_string(),
        ))),
        ParallelDispatchTestMode::Success => Some(Ok(OfferDispatchOutput {
            executed_count: 1,
            newly_executed_sell_counts: std::collections::BTreeMap::from([(1, 1)]),
        })),
    }
}

pub(crate) fn managed_post_result(
    injections: &DaemonDispatchTestInjections,
) -> Option<SignerResult<bool>> {
    match injections.managed_post? {
        ManagedPostTestMode::Success => Some(Ok(true)),
        ManagedPostTestMode::Failure => Some(Ok(false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_dispatch_injection_modes() {
        let transient =
            DaemonDispatchTestInjections::default().parallel(ParallelDispatchTestMode::Transient);
        assert!(matches!(
            parallel_dispatch_result(&transient).expect("configured"),
            Err(SignerError::ReservationContention(_))
        ));

        let fatal =
            DaemonDispatchTestInjections::default().parallel(ParallelDispatchTestMode::Fatal);
        let fatal_err = parallel_dispatch_result(&fatal)
            .expect("configured")
            .expect_err("fatal");
        assert!(fatal_err
            .to_string()
            .contains("permanent_offer_build_failure"));

        let success =
            DaemonDispatchTestInjections::default().parallel(ParallelDispatchTestMode::Success);
        let output = parallel_dispatch_result(&success)
            .expect("configured")
            .expect("success");
        assert_eq!(output.executed_count, 1);
    }

    #[test]
    fn managed_post_injection_modes() {
        let success =
            DaemonDispatchTestInjections::default().managed_post(ManagedPostTestMode::Success);
        assert!(managed_post_result(&success)
            .expect("configured")
            .expect("posted"));

        let failure =
            DaemonDispatchTestInjections::default().managed_post(ManagedPostTestMode::Failure);
        assert!(!managed_post_result(&failure)
            .expect("configured")
            .expect("not posted"));
    }
}
