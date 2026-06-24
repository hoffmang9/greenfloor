//! Test-only overrides for signer denomination bootstrap execution.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde_json::Value;

#[derive(Debug, Default)]
pub struct SignerDenominationTestOverrides {
    vault_mixed_split_stubs: Mutex<Vec<Value>>,
    vault_stub_index: AtomicUsize,
    last_vault_output_amounts_mojos: Mutex<Option<Vec<u64>>>,
}

impl SignerDenominationTestOverrides {
    pub fn enqueue_vault_mixed_split_stub(&self, stub: Value) {
        self.vault_mixed_split_stubs
            .lock()
            .expect("vault stub lock")
            .push(stub);
    }

    pub(crate) fn take_vault_mixed_split_stub(&self) -> Option<Value> {
        let index = self.vault_stub_index.fetch_add(1, Ordering::SeqCst);
        self.vault_mixed_split_stubs
            .lock()
            .expect("vault stub lock")
            .get(index)
            .cloned()
    }

    pub(crate) fn record_vault_output_amounts_mojos(&self, amounts: &[u64]) {
        *self
            .last_vault_output_amounts_mojos
            .lock()
            .expect("vault output lock") = Some(amounts.to_vec());
    }

    #[must_use]
    pub(crate) fn take_vault_output_amounts_mojos(&self) -> Option<Vec<u64>> {
        self.last_vault_output_amounts_mojos
            .lock()
            .expect("vault output lock")
            .take()
    }
}

pub(crate) fn vault_mixed_split_stub_response(
    test_overrides: Option<&SignerDenominationTestOverrides>,
    output_amounts_mojos: &[u64],
) -> Option<Value> {
    test_overrides.map(|overrides| {
        overrides.record_vault_output_amounts_mojos(output_amounts_mojos);
        overrides
            .take_vault_mixed_split_stub()
            .unwrap_or_else(sample_vault_mixed_split_stub)
    })
}

pub(crate) fn sample_vault_mixed_split_stub() -> Value {
    serde_json::json!({
        "offered_total": 100,
        "target_total": 100,
        "change_amount": 0,
        "selected_coin_ids": [],
        "broadcast_status": "submitted",
        "spend_bundle_hex": "deadbeef",
    })
}
