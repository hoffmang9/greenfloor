//! Request-carried test overrides for daemon offer dispatch (unit tests only).

use std::collections::BTreeMap;

use crate::error::{SignerError, SignerResult};

use super::OfferDispatchOutput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParallelDispatchTestMode {
    Transient,
    Fatal,
    Success,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedPostTestMode {
    Success,
    Failure,
}

#[derive(Debug, Clone, Default)]
pub struct OfferDispatchTestOverrides {
    parallel_dispatch: Option<ParallelDispatchTestMode>,
    managed_post: Option<ManagedPostTestMode>,
}

impl OfferDispatchTestOverrides {
    #[must_use]
    pub fn parallel_dispatch(mut self, mode: ParallelDispatchTestMode) -> Self {
        self.parallel_dispatch = Some(mode);
        self
    }

    #[must_use]
    pub fn managed_post(mut self, mode: ManagedPostTestMode) -> Self {
        self.managed_post = Some(mode);
        self
    }

    pub(crate) fn parallel_dispatch_result(&self) -> Option<SignerResult<OfferDispatchOutput>> {
        match self.parallel_dispatch? {
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

    pub(crate) fn managed_post_result(&self) -> Option<SignerResult<bool>> {
        match self.managed_post? {
            ManagedPostTestMode::Success => Some(Ok(true)),
            ManagedPostTestMode::Failure => Some(Ok(false)),
        }
    }
}
