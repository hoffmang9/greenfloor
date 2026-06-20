//! Apply request-carried offer-dispatch test overrides from [`DaemonCycleTestControls`].

use std::collections::BTreeMap;

use crate::daemon::run_once::{
    ManagedPostTestMode, OfferDispatchTestOverrides, ParallelDispatchTestMode,
};
use crate::error::{SignerError, SignerResult};

use super::OfferDispatchOutput;

pub(crate) fn parallel_dispatch_result(
    overrides: &OfferDispatchTestOverrides,
) -> Option<SignerResult<OfferDispatchOutput>> {
    match overrides.parallel_dispatch? {
        ParallelDispatchTestMode::Transient => Some(Err(SignerError::ReservationContention(
            "test override".to_string(),
        ))),
        ParallelDispatchTestMode::Fatal => Some(Err(SignerError::Other(
            "permanent_offer_build_failure: test override".to_string(),
        ))),
        ParallelDispatchTestMode::Success => Some(Ok(OfferDispatchOutput {
            executed_count: 1,
            newly_executed_sell_counts: BTreeMap::from([(1, 1)]),
        })),
    }
}

pub(crate) fn managed_post_result(
    overrides: &OfferDispatchTestOverrides,
) -> Option<SignerResult<bool>> {
    match overrides.managed_post? {
        ManagedPostTestMode::Success => Some(Ok(true)),
        ManagedPostTestMode::Failure => Some(Ok(false)),
    }
}
