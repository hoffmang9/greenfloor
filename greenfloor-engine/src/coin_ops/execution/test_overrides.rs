//! Explicit test overrides for coin-op execution (injected via `CoinOpExecContext`).
//!
//! Canonical pattern: see [`crate::test_support::injections`].

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::coin_ops::SpendableCoin;
use crate::vault::MixedSplitResult;

#[derive(Debug, Clone)]
#[cfg(test)]
pub struct CoinOpTestOverrides {
    pub wallet_coins: Option<Vec<SpendableCoin>>,
    pub mixed_split_operation_id: Option<String>,
    /// First `execute_mixed_split` returns [`SignerError::MixedSplitSelectedCoinsNotSpendable`].
    pub mixed_split_stale_first: bool,
    mixed_split_calls: Arc<AtomicUsize>,
    mixed_split_result_stubs: Arc<Mutex<Vec<MixedSplitResult>>>,
    mixed_split_stub_index: Arc<AtomicUsize>,
    last_vault_output_amounts_mojos: Arc<Mutex<Option<Vec<u64>>>>,
}

#[cfg(test)]
impl Default for CoinOpTestOverrides {
    fn default() -> Self {
        Self {
            wallet_coins: None,
            mixed_split_operation_id: None,
            mixed_split_stale_first: false,
            mixed_split_calls: Arc::new(AtomicUsize::new(0)),
            mixed_split_result_stubs: Arc::new(Mutex::new(Vec::new())),
            mixed_split_stub_index: Arc::new(AtomicUsize::new(0)),
            last_vault_output_amounts_mojos: Arc::new(Mutex::new(None)),
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

    /// # Panics
    ///
    /// Panics if the stub mutex is poisoned.
    pub fn enqueue_mixed_split_result(&self, result: MixedSplitResult) {
        self.mixed_split_result_stubs
            .lock()
            .expect("mixed split stub lock")
            .push(result);
    }

    pub fn enqueue_sample_mixed_split_result(&self) {
        self.enqueue_mixed_split_result(sample_mixed_split_result());
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

    /// When stubs were enqueued, return the next stub and record output amounts.
    ///
    /// Returns `None` when no stubs were enqueued (live vault path).
    pub(crate) fn take_mixed_split_result_stub(
        &self,
        output_amounts_mojos: &[u64],
    ) -> Option<MixedSplitResult> {
        let stubs = self
            .mixed_split_result_stubs
            .lock()
            .expect("mixed split stub lock");
        if stubs.is_empty() {
            return None;
        }
        *self
            .last_vault_output_amounts_mojos
            .lock()
            .expect("vault output lock") = Some(output_amounts_mojos.to_vec());
        let index = self.mixed_split_stub_index.fetch_add(1, Ordering::SeqCst);
        Some(
            stubs
                .get(index)
                .cloned()
                .unwrap_or_else(sample_mixed_split_result),
        )
    }

    /// # Panics
    ///
    /// Panics if the vault-output mutex is poisoned.
    #[must_use]
    pub fn take_vault_output_amounts_mojos(&self) -> Option<Vec<u64>> {
        self.last_vault_output_amounts_mojos
            .lock()
            .expect("vault output lock")
            .take()
    }
}

#[cfg(test)]
#[must_use]
pub fn sample_mixed_split_result() -> MixedSplitResult {
    MixedSplitResult {
        spend_bundle_hex: "deadbeef".to_string(),
        broadcast_status: Some("submitted".to_string()),
        selected_coin_ids: Vec::new(),
        offered_total: 100,
        target_total: 100,
        change_amount: 0,
    }
}
