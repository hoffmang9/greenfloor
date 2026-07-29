//! Explicit test overrides for coin-op execution (injected via `CoinOpExecContext`).
//!
//! Canonical pattern: see [`crate::test_support::injections`].

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::coin_ops::SpendableCoin;

#[derive(Debug, Clone)]
#[cfg(test)]
pub struct CoinOpTestOverrides {
    pub wallet_coins: Option<Vec<SpendableCoin>>,
    pub mixed_split_operation_id: Option<String>,
    /// First `execute_mixed_split` returns [`SignerError::MixedSplitSelectedCoinsNotSpendable`].
    pub mixed_split_stale_first: bool,
    mixed_split_calls: Arc<AtomicUsize>,
}

#[cfg(test)]
impl Default for CoinOpTestOverrides {
    fn default() -> Self {
        Self {
            wallet_coins: None,
            mixed_split_operation_id: None,
            mixed_split_stale_first: false,
            mixed_split_calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[cfg(test)]
impl CoinOpTestOverrides {
    #[must_use]
    pub fn new(
        wallet_coins: Option<Vec<SpendableCoin>>,
        mixed_split_operation_id: Option<String>,
    ) -> Self {
        Self {
            wallet_coins,
            mixed_split_operation_id,
            ..Default::default()
        }
    }

    #[must_use]
    pub fn with_mixed_split_stale_first(mut self) -> Self {
        self.mixed_split_stale_first = true;
        self
    }

    pub(crate) fn wallet_coins_override(&self) -> Option<&[SpendableCoin]> {
        self.wallet_coins.as_deref()
    }

    pub(crate) fn mixed_split_operation_id_override(&self) -> Option<&str> {
        self.mixed_split_operation_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn take_mixed_split_stale_first_failure(&self) -> bool {
        if !self.mixed_split_stale_first {
            return false;
        }
        self.mixed_split_calls.fetch_add(1, Ordering::SeqCst) == 0
    }
}
