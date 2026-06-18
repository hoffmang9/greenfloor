//! Deterministic signer ``create_offer`` leg math and request shaping (no IO).

use serde_json::Value;

use crate::error::{SignerError, SignerResult};
use crate::num_conv::quote_mojos_for_base_size;
use crate::offer::build_context::mojo_multiplier_for_leg;

/// Normalized offer action side: ``buy`` or ``sell``.
pub fn normalize_offer_side(value: &str) -> &'static str {
    if value.trim().eq_ignore_ascii_case("buy") {
        "buy"
    } else {
        "sell"
    }
}

/// Asset id to split for bootstrap / presplit given action side.
pub fn signer_split_asset_id(
    action_side: &str,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
) -> String {
    if normalize_offer_side(action_side) == "buy" {
        normalize_offer_asset_id(resolved_quote_asset_id)
    } else {
        normalize_offer_asset_id(resolved_base_asset_id)
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

fn base_and_quote_leg_mojos(
    size_base_units: i64,
    quote_price: f64,
    base_mult: i64,
    quote_mult: i64,
) -> SignerResult<(u64, u64)> {
    if size_base_units <= 0 {
        return Err(SignerError::InvalidSizeBaseUnits);
    }
    let base_offer = size_base_units
        .checked_mul(base_mult)
        .ok_or(SignerError::InvalidOfferAmount)?;
    if base_offer <= 0 {
        return Err(SignerError::InvalidOfferAmount);
    }
    let request_amount = quote_mojos_for_base_size(size_base_units, quote_price, quote_mult)?;
    if request_amount <= 0 {
        return Err(SignerError::InvalidOfferRequestAmount);
    }
    let offer_u = u64::try_from(base_offer).map_err(|_| SignerError::InvalidOfferAmount)?;
    let request_u =
        u64::try_from(request_amount).map_err(|_| SignerError::InvalidOfferRequestAmount)?;
    Ok((offer_u, request_u))
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
    let base_mult =
        mojo_multiplier_for_leg(pricing, "base_unit_mojo_multiplier", resolved_base_asset_id);
    let quote_mult = mojo_multiplier_for_leg(
        pricing,
        "quote_unit_mojo_multiplier",
        resolved_quote_asset_id,
    );
    let (base_offer_mojos, quote_request_mojos) =
        base_and_quote_leg_mojos(size_base_units, quote_price, base_mult, quote_mult)?;

    let (offer_asset_id, request_asset_id, offer_amount_mojos, request_amount_mojos) =
        if side == "buy" {
            (
                normalize_offer_asset_id(resolved_quote_asset_id),
                normalize_offer_asset_id(resolved_base_asset_id),
                quote_request_mojos,
                base_offer_mojos,
            )
        } else {
            (
                normalize_offer_asset_id(resolved_base_asset_id),
                normalize_offer_asset_id(resolved_quote_asset_id),
                base_offer_mojos,
                quote_request_mojos,
            )
        };

    Ok(SignerOfferLegAmounts {
        offer_asset_id,
        request_asset_id,
        offer_amount_mojos,
        request_amount_mojos,
    })
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
        assert_eq!(
            quote_mojos_for_base_size(1, 1.0, 1_000_000_000_000).expect("quote mojos"),
            1_000_000_000_000
        );
        assert_eq!(
            quote_mojos_for_base_size(1, 5.0, 1_000).expect("quote mojos"),
            5_000
        );
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
        assert!(matches!(err, SignerError::InvalidOfferRequestAmount));
    }

    #[test]
    fn rejects_non_positive_size_base_units() {
        let err = compute_signer_offer_leg_amounts(
            0,
            1.0,
            BASE_ASSET,
            QUOTE_XCH,
            "sell",
            &pricing(1_000, 1_000),
        )
        .unwrap_err();
        assert!(matches!(err, SignerError::InvalidSizeBaseUnits));
    }

    #[test]
    fn rejects_zero_base_multiplier_offer_amount() {
        let err = compute_signer_offer_leg_amounts(
            1,
            1.0,
            BASE_ASSET,
            QUOTE_XCH,
            "sell",
            &pricing(0, 1_000),
        )
        .unwrap_err();
        assert!(matches!(err, SignerError::InvalidOfferAmount));
    }

    #[test]
    fn normalize_offer_asset_id_strips_prefix() {
        assert_eq!(normalize_offer_asset_id("0xAbCd"), "abcd");
    }

    #[test]
    fn signer_split_asset_id_normalizes_selected_asset() {
        assert_eq!(
            signer_split_asset_id("sell", &format!("0x{BASE_ASSET}"), QUOTE_XCH),
            BASE_ASSET
        );
        let quote_cat = "664799fc173e0d9d4d024c42e411d26f275eeb1095dad980ccd11df09c8bb6fb";
        assert_eq!(
            signer_split_asset_id("buy", BASE_ASSET, &format!("0x{quote_cat}")),
            quote_cat
        );
    }

    #[test]
    fn compute_normalizes_offer_and_request_asset_ids() {
        let leg = compute_signer_offer_leg_amounts(
            1,
            1.0,
            &format!("0x{BASE_ASSET}"),
            QUOTE_XCH,
            "sell",
            &pricing(1_000, 1_000),
        )
        .expect("leg amounts");
        assert_eq!(leg.offer_asset_id, BASE_ASSET);
        assert_eq!(leg.request_asset_id, QUOTE_XCH);
    }
}
