use serde_json::Value;

/// Whether a wallet coin state string is spendable for coin-op selection.
#[must_use]
pub fn is_spendable_coin_state(state: &str) -> bool {
    const NON_SPENDABLE: &[&str] = &[
        "PENDING",
        "MEMPOOL",
        "SPENT",
        "SPENDING",
        "LOCKED",
        "RESERVED",
        "UNCONFIRMED",
    ];
    const SPENDABLE: &[&str] = &["CONFIRMED", "UNSPENT", "SPENDABLE", "AVAILABLE", "SETTLED"];
    let state = state.trim().to_ascii_uppercase();
    if state.is_empty() {
        return false;
    }
    if NON_SPENDABLE.contains(&state.as_str()) {
        return false;
    }
    SPENDABLE.contains(&state.as_str())
}

/// Whether a Cloud Wallet / signer coin row is spendable for coin-op selection.
pub fn is_spendable_wallet_coin(coin: &Value) -> bool {
    if coin.get("isLocked").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    is_spendable_coin_state(coin.get("state").and_then(Value::as_str).unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn locked_or_pending_not_spendable() {
        assert!(!is_spendable_wallet_coin(&json!({"state": "PENDING"})));
        assert!(!is_spendable_wallet_coin(
            &json!({"isLocked": true, "state": "CONFIRMED"})
        ));
    }

    #[test]
    fn confirmed_is_spendable() {
        assert!(is_spendable_wallet_coin(&json!({"state": "CONFIRMED"})));
    }
}
