//! Test-only overrides for signer denomination bootstrap execution.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde_json::Value;

#[derive(Debug, Default)]
pub struct SignerDenominationTestOverrides {
    vault_mixed_split_stubs: Mutex<Vec<Value>>,
    vault_stub_index: AtomicUsize,
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
}

pub(crate) fn vault_mixed_split_stub_response(
    test_overrides: Option<&SignerDenominationTestOverrides>,
) -> Option<Value> {
    test_overrides.and_then(SignerDenominationTestOverrides::take_vault_mixed_split_stub)
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
