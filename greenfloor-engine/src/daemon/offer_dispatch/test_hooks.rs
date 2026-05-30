#[cfg(test)]
use std::sync::{Mutex, MutexGuard};

#[cfg(test)]
static TEST_HOOKS_SERIAL: Mutex<()> = Mutex::new(());

#[cfg(test)]
static PARALLEL_DISPATCH_OVERRIDE: Mutex<Option<&'static str>> = Mutex::new(None);

#[cfg(test)]
static MANAGED_POST_OVERRIDE: Mutex<Option<&'static str>> = Mutex::new(None);

/// Serializes tests that mutate global dispatch/post overrides (cargo runs tests in parallel).
#[cfg(test)]
pub struct TestHooksScope {
    _guard: MutexGuard<'static, ()>,
}

#[cfg(test)]
impl TestHooksScope {
    pub fn begin() -> Self {
        Self {
            _guard: TEST_HOOKS_SERIAL
                .lock()
                .expect("test hooks scope lock poisoned"),
        }
    }
}

#[cfg(test)]
impl Drop for TestHooksScope {
    fn drop(&mut self) {
        if let Ok(mut parallel) = PARALLEL_DISPATCH_OVERRIDE.lock() {
            *parallel = None;
        }
        if let Ok(mut managed) = MANAGED_POST_OVERRIDE.lock() {
            *managed = None;
        }
    }
}

#[cfg(test)]
pub fn set_parallel_dispatch_override(mode: Option<&'static str>) {
    *PARALLEL_DISPATCH_OVERRIDE
        .lock()
        .expect("parallel override lock") = mode;
}

#[cfg(test)]
pub fn set_managed_post_override(mode: Option<&'static str>) {
    *MANAGED_POST_OVERRIDE
        .lock()
        .expect("managed post override lock") = mode;
}

#[cfg(test)]
pub fn parallel_dispatch_test_override(
) -> Option<crate::error::SignerResult<super::OfferDispatchOutput>> {
    use std::collections::BTreeMap;

    use crate::error::SignerError;

    match *PARALLEL_DISPATCH_OVERRIDE
        .lock()
        .expect("parallel override lock")
    {
        None => None,
        Some("transient") => Some(Err(SignerError::ReservationContention(
            "test override".to_string(),
        ))),
        Some("fatal") => Some(Err(SignerError::Other(
            "permanent_offer_build_failure: test override".to_string(),
        ))),
        Some("success") => Some(Ok(super::OfferDispatchOutput {
            executed_count: 1,
            newly_executed_sell_counts: BTreeMap::from([(1, 1)]),
        })),
        Some(other) => panic!("unknown parallel dispatch test override: {other}"),
    }
}

#[cfg(test)]
pub fn managed_post_test_override() -> Option<crate::error::SignerResult<bool>> {
    match *MANAGED_POST_OVERRIDE.lock().expect("managed post override lock") {
        None => None,
        Some("success") => Some(Ok(true)),
        Some("failure") => Some(Ok(false)),
        Some(other) => panic!("unknown managed post test override: {other}"),
    }
}
