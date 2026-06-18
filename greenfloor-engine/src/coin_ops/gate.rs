use serde::Serialize;

use crate::metrics::collection_len_to_i64;

use super::wallet_coin::is_spendable_wallet_coin;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoinSplitGateResult {
    pub asset_id: String,
    pub size_base_units: i64,
    pub required_min_count: i64,
    pub current_count: i64,
    pub larger_reserve_coin_count: i64,
    pub extra_denom_coin_count: i64,
    pub reserve_ready: bool,
    pub ready: bool,
}

/// Denomination readiness gate for split-until-ready loops (mirrors Python runtime policy).
pub fn evaluate_coin_split_gate(
    asset_scoped_coins: &[serde_json::Value],
    resolved_asset_id: &str,
    size_base_units: i64,
    required_count: i64,
) -> CoinSplitGateResult {
    let spendable: Vec<i64> = asset_scoped_coins
        .iter()
        .filter(|coin| is_spendable_wallet_coin(coin))
        .filter_map(|coin| coin.get("amount").and_then(serde_json::Value::as_i64))
        .collect();
    let size = size_base_units;
    let required = required_count;
    let denom_coins: Vec<i64> = spendable
        .iter()
        .copied()
        .filter(|amount| *amount == size)
        .collect();
    let larger_reserve_count =
        collection_len_to_i64(spendable.iter().filter(|amount| **amount > size).count());
    let current_count = collection_len_to_i64(denom_coins.len());
    let extra_denom_count = (current_count - required).max(0);
    let reserve_ready = larger_reserve_count >= 1 || extra_denom_count >= 1;
    let ready = current_count >= required && reserve_ready;
    CoinSplitGateResult {
        asset_id: resolved_asset_id.to_string(),
        size_base_units: size,
        required_min_count: required,
        current_count,
        larger_reserve_coin_count: larger_reserve_count,
        extra_denom_coin_count: extra_denom_count,
        reserve_ready,
        ready,
    }
}

/// Stop predicate for coin-op until-ready iteration loops.
pub fn coin_op_should_stop(
    until_ready: bool,
    final_readiness_ready: Option<bool>,
    has_explicit_coin_ids: bool,
    iteration: i64,
    max_iterations: i64,
) -> (bool, &'static str) {
    if !until_ready || final_readiness_ready.unwrap_or(false) {
        let stop_reason = if until_ready && final_readiness_ready.is_some() {
            "ready"
        } else {
            "single_pass"
        };
        return (true, stop_reason);
    }
    if has_explicit_coin_ids {
        return (true, "requires_new_coin_selection");
    }
    if iteration == max_iterations {
        return (true, "max_iterations_reached");
    }
    (false, "")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoinCombineGateResult {
    pub asset_id: String,
    pub size_base_units: i64,
    pub max_allowed_count: i64,
    pub current_count: i64,
    pub ready: bool,
}

/// Denomination readiness for combine-until-ready loops (excess denomination coin cap).
pub fn evaluate_coin_combine_gate(
    asset_scoped_coins: &[serde_json::Value],
    asset_id: &str,
    size_base_units: i64,
    max_allowed_count: i64,
) -> CoinCombineGateResult {
    let current_count = collection_len_to_i64(
        asset_scoped_coins
            .iter()
            .filter(|coin| is_spendable_wallet_coin(coin))
            .filter_map(|coin| coin.get("amount").and_then(serde_json::Value::as_i64))
            .filter(|amount| *amount == size_base_units)
            .count(),
    );
    let ready = current_count <= max_allowed_count;
    CoinCombineGateResult {
        asset_id: asset_id.to_string(),
        size_base_units,
        max_allowed_count,
        current_count,
        ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn split_gate_ready_when_target_and_reserve_met() {
        let coins = vec![
            json!({"amount": 100, "state": "CONFIRMED"}),
            json!({"amount": 100, "state": "CONFIRMED"}),
            json!({"amount": 200, "state": "CONFIRMED"}),
        ];
        let gate = evaluate_coin_split_gate(&coins, "cat", 100, 2);
        assert!(gate.ready);
        assert!(gate.reserve_ready);
        assert_eq!(gate.current_count, 2);
    }

    #[test]
    fn coin_op_should_stop_max_iterations() {
        let (stop, reason) = coin_op_should_stop(true, Some(false), false, 3, 3);
        assert!(stop);
        assert_eq!(reason, "max_iterations_reached");
    }

    #[test]
    fn coin_op_should_stop_explicit_coin_ids_when_not_ready() {
        let (stop, reason) = coin_op_should_stop(true, Some(false), true, 1, 3);
        assert!(stop);
        assert_eq!(reason, "requires_new_coin_selection");
    }

    #[test]
    fn combine_gate_ready_when_at_or_below_cap() {
        let coins = vec![
            json!({"amount": 100, "state": "CONFIRMED"}),
            json!({"amount": 100, "state": "CONFIRMED"}),
        ];
        let gate = evaluate_coin_combine_gate(&coins, "cat", 100, 2);
        assert!(gate.ready);
        assert_eq!(gate.current_count, 2);
    }

    #[test]
    fn combine_gate_not_ready_when_above_cap() {
        let coins: Vec<serde_json::Value> = (0..4)
            .map(|_| json!({"amount": 100, "state": "CONFIRMED"}))
            .collect();
        let gate = evaluate_coin_combine_gate(&coins, "cat", 100, 2);
        assert!(!gate.ready);
        assert_eq!(gate.current_count, 4);
    }
}
