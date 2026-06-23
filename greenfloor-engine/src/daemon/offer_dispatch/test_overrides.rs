//! Test-only injection mappers for offer dispatch.
//!
//! Unit tests here cover the branch table. Integration wiring is exercised in
//! `tests/harness_tests.rs`, which asserts dispatch outcomes only.
//!
//! Canonical pattern: see [`crate::test_support::injections`].

use crate::daemon::dispatch_test_controls::{
    DaemonDispatchTestInjections, ManagedPostTestMode, ParallelDispatchTestMode,
};
use crate::error::{SignerError, SignerResult};

use super::managed_post::{flush_managed_post_persist_for_test, ManagedPostContext};
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
    post_ctx: &ManagedPostContext,
    injections: &DaemonDispatchTestInjections,
) -> Option<SignerResult<bool>> {
    match injections.managed_post? {
        ManagedPostTestMode::Success => Some(Ok(true)),
        ManagedPostTestMode::Failure => Some(Ok(false)),
        ManagedPostTestMode::ExerciseSharedPersistFlush => {
            Some(flush_managed_post_persist_for_test(post_ctx).map(|()| true))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ManagerProgramConfig;
    use crate::daemon::offer_dispatch::managed_post::ManagedPostContext;
    use crate::storage::SqliteStore;

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
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open_shared(&dir.path().join("state.db")).expect("open");
        let post_ctx = ManagedPostContext {
            program: ManagerProgramConfig::default(),
            paths: crate::daemon::cycle_paths::DaemonCyclePaths::new(
                dir.path().join("program.yaml"),
                dir.path().join("markets.yaml"),
                None,
            ),
            write_store: store,
            dispatch_injections: DaemonDispatchTestInjections::default(),
        };

        let success =
            DaemonDispatchTestInjections::default().managed_post(ManagedPostTestMode::Success);
        assert!(managed_post_result(&post_ctx, &success)
            .expect("configured")
            .expect("posted"));

        let failure =
            DaemonDispatchTestInjections::default().managed_post(ManagedPostTestMode::Failure);
        assert!(!managed_post_result(&post_ctx, &failure)
            .expect("configured")
            .expect("not posted"));

        crate::storage::reset_sqlite_open_calls_for_test();
        let flush = DaemonDispatchTestInjections::default()
            .managed_post(ManagedPostTestMode::ExerciseSharedPersistFlush);
        assert!(managed_post_result(&post_ctx, &flush)
            .expect("configured")
            .expect("flushed"));
        assert_eq!(crate::storage::sqlite_open_calls_for_test(), 0);
    }
}
