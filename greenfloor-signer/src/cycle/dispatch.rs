use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedActionInput {
    pub size: i64,
    pub repeat: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pair: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_value: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_after_create: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_spread_bps: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SpendableAssetProfile {
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub max_single: i64,
    #[serde(default)]
    pub max_single_known: bool,
}

/// Expand each input row by its `repeat` count (preserves per-row metadata).
pub(crate) fn expand_inputs_by_repeat(actions: &[PlannedActionInput]) -> Vec<PlannedActionInput> {
    let mut expanded = Vec::new();
    for action in actions {
        let repeat = action.repeat.max(0);
        for _ in 0..repeat {
            expanded.push(action.clone());
        }
    }
    expanded
}

pub fn expiry_seconds_for_action(expiry_unit: &str, expiry_value: i64) -> Option<i64> {
    if expiry_value <= 0 {
        return None;
    }
    let unit = expiry_unit.trim().to_ascii_lowercase();
    let unit_seconds = match unit.as_str() {
        "second" | "seconds" => 1,
        "minute" | "minutes" => 60,
        "hour" | "hours" => 60 * 60,
        "day" | "days" => 24 * 60 * 60,
        _ => return None,
    };
    Some(expiry_value * unit_seconds)
}

pub fn reservation_request_for_managed_offer(
    side: &str,
    size_base_units: i64,
    base_asset_id: &str,
    quote_asset_id: &str,
    base_unit_mojo_multiplier: i64,
    quote_unit_mojo_multiplier: i64,
    quote_price: f64,
    fee_asset_id: &str,
    fee_amount_mojos: i64,
) -> BTreeMap<String, i64> {
    let base_asset_id = base_asset_id.trim();
    let quote_asset_id = quote_asset_id.trim();
    if base_asset_id.is_empty() || quote_asset_id.is_empty() {
        return BTreeMap::new();
    }

    let side = side.trim().to_ascii_lowercase();
    let base_amount = size_base_units * base_unit_mojo_multiplier;
    let quote_amount = (size_base_units as f64 * quote_price * quote_unit_mojo_multiplier as f64)
        .round() as i64;
    let (offer_asset_id, offer_amount) = if side == "buy" {
        (quote_asset_id, quote_amount)
    } else {
        (base_asset_id, base_amount)
    };
    if offer_amount <= 0 {
        return BTreeMap::new();
    }

    let mut request = BTreeMap::from([(offer_asset_id.to_string(), offer_amount)]);
    let fee_asset = fee_asset_id.trim();
    if !fee_asset.is_empty() && fee_amount_mojos > 0 {
        *request.entry(fee_asset.to_string()).or_insert(0) += fee_amount_mojos;
    }
    request
}

pub fn single_input_preferred_skip_reason(
    requested_amounts: &BTreeMap<String, i64>,
    spendable_profiles: &BTreeMap<String, SpendableAssetProfile>,
) -> Option<String> {
    let primary_request_candidates: Vec<(&String, i64)> = requested_amounts
        .iter()
        .filter_map(|(asset_id, amount)| {
            let amount = *amount;
            if asset_id.trim().is_empty() || amount <= 0 {
                None
            } else {
                Some((asset_id, amount))
            }
        })
        .collect();
    if primary_request_candidates.is_empty() {
        return None;
    }
    let (primary_asset_id, primary_needed) = primary_request_candidates
        .into_iter()
        .max_by_key(|(_, amount)| *amount)?;

    let profile = spendable_profiles
        .get(primary_asset_id)
        .cloned()
        .unwrap_or_default();
    if !profile.max_single_known {
        return None;
    }
    if profile.total >= primary_needed && profile.max_single < primary_needed {
        return Some(format!(
            "single_input_preferred_requires_combine:asset_id={primary_asset_id}:needed={primary_needed}:max_single={}:available={}",
            profile.max_single, profile.total
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_inputs_by_repeat_preserves_order() {
        let actions = vec![
            PlannedActionInput {
                size: 1,
                repeat: 2,
                side: None,
                pair: None,
                expiry_unit: None,
                expiry_value: None,
                cancel_after_create: None,
                reason: None,
                target_spread_bps: None,
            },
            PlannedActionInput {
                size: 10,
                repeat: 1,
                side: None,
                pair: None,
                expiry_unit: None,
                expiry_value: None,
                cancel_after_create: None,
                reason: None,
                target_spread_bps: None,
            },
        ];
        let expanded = expand_inputs_by_repeat(&actions);
        assert_eq!(expanded.len(), 3);
        assert_eq!(expanded[0].size, 1);
        assert_eq!(expanded[1].size, 1);
        assert_eq!(expanded[2].size, 10);
    }

    #[test]
    fn reservation_request_sell_side_uses_base_asset() {
        let request = reservation_request_for_managed_offer(
            "sell",
            10,
            "base_asset",
            "quote_asset",
            1000,
            1000,
            1.5,
            "xch_asset",
            0,
        );
        assert_eq!(request.get("base_asset"), Some(&10_000));
    }

    #[test]
    fn single_input_preferred_skip_when_no_large_enough_coin() {
        let requested = BTreeMap::from([("asset_a".to_string(), 5000)]);
        let profiles = BTreeMap::from([(
            "asset_a".to_string(),
            SpendableAssetProfile {
                total: 6000,
                max_single: 1000,
                max_single_known: true,
            },
        )]);
        let reason = single_input_preferred_skip_reason(&requested, &profiles);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("single_input_preferred_requires_combine"));
    }
}
