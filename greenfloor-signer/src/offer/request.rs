//! Deterministic signer ``create_offer`` leg math and request shaping (no IO).

use serde_json::Value;

use crate::error::{SignerError, SignerResult};
use crate::offer::build_context::mojo_multiplier_for_leg;

/// Normalized offer action side: ``buy`` or ``sell``.
pub fn normalize_offer_side(value: &str) -> &'static str {
    if value.trim().eq_ignore_ascii_case("buy") {
        "buy"
    } else {
        "sell"
    }
}

/// Quote-leg mojos for a base size at the given price and unit multiplier.
pub fn quote_mojos_for_base_size(
    size_base_units: i64,
    quote_price: f64,
    quote_unit_multiplier: i64,
) -> i64 {
    (size_base_units as f64 * quote_price * quote_unit_multiplier as f64).round() as i64
}

/// Asset id to split for bootstrap / presplit given action side.
pub fn signer_split_asset_id(
    action_side: &str,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
) -> String {
    if normalize_offer_side(action_side) == "buy" {
        resolved_quote_asset_id.trim().to_string()
    } else {
        resolved_base_asset_id.trim().to_string()
    }
}

/// Strip optional ``0x`` prefix and lowercase for signer request fields.
pub fn normalize_offer_asset_id(asset_id: &str) -> String {
    let trimmed = asset_id.trim().to_lowercase();
    trimmed.strip_prefix("0x").unwrap_or(&trimmed).to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerOfferLegAmounts {
    pub offer_asset_id: String,
    pub request_asset_id: String,
    pub offer_amount_mojos: u64,
    pub request_amount_mojos: u64,
}

/// Compute offer/request legs for vault CAT offer construction.
pub fn compute_signer_offer_leg_amounts(
    size_base_units: i64,
    quote_price: f64,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    action_side: &str,
    pricing: &Value,
) -> SignerResult<SignerOfferLegAmounts> {
    let side = normalize_offer_side(action_side);
    let base_mult = mojo_multiplier_for_leg(
        pricing,
        "base_unit_mojo_multiplier",
        resolved_base_asset_id,
    );
    let quote_mult = mojo_multiplier_for_leg(
        pricing,
        "quote_unit_mojo_multiplier",
        resolved_quote_asset_id,
    );
    let offer_amount = size_base_units.saturating_mul(base_mult);
    let request_amount = quote_mojos_for_base_size(size_base_units, quote_price, quote_mult);
    if request_amount <= 0 {
        return Err(SignerError::Other(
            "request_amount must be positive".to_string(),
        ));
    }

    if side == "buy" {
        Ok(SignerOfferLegAmounts {
            offer_asset_id: resolved_quote_asset_id.trim().to_string(),
            request_asset_id: resolved_base_asset_id.trim().to_string(),
            offer_amount_mojos: u64::try_from(request_amount).map_err(|_| {
                SignerError::Other("offer_amount_mojos overflow".to_string())
            })?,
            request_amount_mojos: u64::try_from(offer_amount).map_err(|_| {
                SignerError::Other("request_amount_mojos overflow".to_string())
            })?,
        })
    } else {
        Ok(SignerOfferLegAmounts {
            offer_asset_id: resolved_base_asset_id.trim().to_string(),
            request_asset_id: resolved_quote_asset_id.trim().to_string(),
            offer_amount_mojos: u64::try_from(offer_amount).map_err(|_| {
                SignerError::Other("offer_amount_mojos overflow".to_string())
            })?,
            request_amount_mojos: u64::try_from(request_amount).map_err(|_| {
                SignerError::Other("request_amount_mojos overflow".to_string())
            })?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BASE_ASSET: &str = "457275a8b9926058d8c9c2ebae52ac5938a4034345cafef6e87f4c7c24b749d8";
    const QUOTE_XCH: &str = "xch";

    fn pricing(base_mult: i64, quote_mult: i64) -> Value {
        json!({
            "base_unit_mojo_multiplier": base_mult,
            "quote_unit_mojo_multiplier": quote_mult,
        })
    }

    #[test]
    fn normalize_offer_side_maps_buy_and_default_sell() {
        assert_eq!(normalize_offer_side("buy"), "buy");
        assert_eq!(normalize_offer_side("BUY"), "buy");
        assert_eq!(normalize_offer_side("sell"), "sell");
        assert_eq!(normalize_offer_side(""), "sell");
    }

    #[test]
    fn quote_mojos_matches_python_rounding() {
        assert_eq!(quote_mojos_for_base_size(1, 1.0, 1_000_000_000_000), 1_000_000_000_000);
        assert_eq!(quote_mojos_for_base_size(1, 5.0, 1_000), 5_000);
    }

    #[test]
    fn sell_side_leg_amounts_match_direct_fixture() {
        let leg = compute_signer_offer_leg_amounts(
            1,
            1.0,
            BASE_ASSET,
            QUOTE_XCH,
            "sell",
            &pricing(1_000, 1_000_000_000_000),
        )
        .expect("leg amounts");
        assert_eq!(leg.offer_asset_id, BASE_ASSET);
        assert_eq!(leg.request_asset_id, QUOTE_XCH);
        assert_eq!(leg.offer_amount_mojos, 1_000);
        assert_eq!(leg.request_amount_mojos, 1_000_000_000_000);
    }

    #[test]
    fn buy_side_swaps_legs_and_amounts() {
        let quote_cat = "664799fc173e0d9d4d024c42e411d26f275eeb1095dad980ccd11df09c8bb6fb";
        let leg = compute_signer_offer_leg_amounts(
            1,
            5.0,
            BASE_ASSET,
            quote_cat,
            "buy",
            &pricing(1_000, 1_000),
        )
        .expect("leg amounts");
        assert_eq!(leg.offer_asset_id, quote_cat);
        assert_eq!(leg.request_asset_id, BASE_ASSET);
        assert_eq!(leg.offer_amount_mojos, 5_000);
        assert_eq!(leg.request_amount_mojos, 1_000);
    }

    #[test]
    fn rejects_non_positive_request_amount() {
        let err = compute_signer_offer_leg_amounts(
            1,
            0.0,
            BASE_ASSET,
            QUOTE_XCH,
            "sell",
            &pricing(1_000, 1_000),
        )
        .unwrap_err();
        assert!(err.to_string().contains("request_amount must be positive"));
    }

    #[test]
    fn normalize_offer_asset_id_strips_prefix() {
        assert_eq!(
            normalize_offer_asset_id("0xAbCd"),
            "abcd"
        );
    }
}
