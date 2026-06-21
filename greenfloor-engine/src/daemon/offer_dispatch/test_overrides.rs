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
