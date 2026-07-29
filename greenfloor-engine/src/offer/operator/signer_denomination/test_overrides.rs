//! Test-only vault submit stubs for signer denomination bootstrap.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::vault::MixedSplitResult;

#[derive(Debug, Default)]
pub struct SignerDenominationTestOverrides {
    vault_mixed_split_stubs: Mutex<Vec<MixedSplitResult>>,
    vault_stub_index: AtomicUsize,
    last_vault_output_amounts_mojos: Mutex<Option<Vec<u64>>>,
}

impl SignerDenominationTestOverrides {
    /// # Panics
    ///
    /// Panics if the stub mutex is poisoned.
    pub fn enqueue_vault_mixed_split_stub(&self, stub: MixedSplitResult) {
        self.vault_mixed_split_stubs
            .lock()
            .expect("vault stub lock")
            .push(stub);
    }

    pub fn enqueue_sample_vault_mixed_split_stub(&self) {
        self.enqueue_vault_mixed_split_stub(sample_vault_mixed_split_result());
    }

    pub(crate) fn take_vault_mixed_split_stub(
        &self,
        output_amounts_mojos: &[u64],
    ) -> Option<MixedSplitResult> {
        let stubs = self
            .vault_mixed_split_stubs
            .lock()
            .expect("vault stub lock");
        if stubs.is_empty() {
            return None;
        }
        *self
            .last_vault_output_amounts_mojos
            .lock()
            .expect("vault output lock") = Some(output_amounts_mojos.to_vec());
        let index = self.vault_stub_index.fetch_add(1, Ordering::SeqCst);
        Some(
            stubs
                .get(index)
                .cloned()
                .unwrap_or_else(sample_vault_mixed_split_result),
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

#[must_use]
pub fn sample_vault_mixed_split_result() -> MixedSplitResult {
    MixedSplitResult {
        spend_bundle_hex: "deadbeef".to_string(),
        broadcast_status: Some("submitted".to_string()),
        selected_coin_ids: Vec::new(),
        offered_total: 100,
        target_total: 100,
        change_amount: 0,
    }
}
