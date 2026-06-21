//! Request-carried daemon offer-dispatch test injections.

use std::collections::BTreeMap;

use crate::cycle::SpendableAssetProfile;

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

/// Test-only offer-dispatch injections carried on `DaemonCycleTestControls`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DaemonDispatchTestInjections {
    pub(crate) parallel: Option<ParallelDispatchTestMode>,
    pub(crate) managed_post: Option<ManagedPostTestMode>,
    pub(crate) spendable_profiles: Option<BTreeMap<String, SpendableAssetProfile>>,
}

impl DaemonDispatchTestInjections {
    #[must_use]
    pub fn parallel(mut self, mode: ParallelDispatchTestMode) -> Self {
        self.parallel = Some(mode);
        self
    }

    #[must_use]
    pub fn managed_post(mut self, mode: ManagedPostTestMode) -> Self {
        self.managed_post = Some(mode);
        self
    }

    #[must_use]
    pub fn spendable_profiles(mut self, profiles: BTreeMap<String, SpendableAssetProfile>) -> Self {
        self.spendable_profiles = Some(profiles);
        self
    }
}
