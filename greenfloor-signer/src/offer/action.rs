//! Unified offer build for market actions (signer vault KMS and local BLS paths).

use std::time::{SystemTime, UNIX_EPOCH};

use chia_bls::SecretKey;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bls::{build_bls_offer_spend_bundle, BlsOfferRequest};
use crate::coinset::{self, is_xch_like_asset, normalize_asset_id, resolve_offer_asset_ids, MspCoinset};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::offer::build::build_vault_cat_offer;
use crate::offer::build_context::{resolve_offer_expiry_for_pricing, resolve_quote_price_for_pricing};
use crate::offer::codec::encode_offer_from_spend_bundle_bytes;
use crate::offer::request::{compute_signer_offer_leg_amounts, normalize_offer_asset_id, normalize_offer_side};
use crate::offer::types::{CreateOfferRequest, CreateOfferResult};

#[derive(Debug, Clone, Deserialize)]
pub struct BuildOfferForActionRequest {
    pub receive_address: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub size_base_units: u64,
    pub action_side: String,
    pub pricing: Value,
    #[serde(default)]
    pub quote_price: Option<f64>,
    #[serde(default = "default_true")]
    pub split_input_coins: bool,
    #[serde(default = "default_true")]
    pub broadcast_split: bool,
    #[serde(default)]
    pub offer_coin_ids: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildOfferForActionResult {
    pub offer_text: String,
    pub side: String,
    pub expires_at_unix: u64,
    pub offer_amount: u64,
    pub request_amount: u64,
    pub execution_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_result: Option<CreateOfferResult>,
}

pub fn expires_at_unix_from_pricing(pricing: &Value) -> u64 {
    let (unit, value) = resolve_offer_expiry_for_pricing(pricing);
    let secs = match unit {
        "hours" => value.saturating_mul(3600),
        "days" => value.saturating_mul(86400),
        _ => value.saturating_mul(60),
    };
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().saturating_add(secs as u64))
        .unwrap_or(secs as u64)
}

fn resolve_quote_price(request: &BuildOfferForActionRequest) -> SignerResult<f64> {
    if let Some(price) = request.quote_price {
        if price > 0.0 {
            return Ok(price);
        }
    }
    resolve_quote_price_for_pricing(&request.pricing)
}

async fn resolve_signer_assets(
    msp: &MspCoinset,
    base_asset: &str,
    quote_asset: &str,
) -> SignerResult<(String, String)> {
    match (
        normalize_asset_id(base_asset),
        normalize_asset_id(quote_asset),
    ) {
        (Ok(resolved_base), Ok(resolved_quote)) => {
            if resolved_base == resolved_quote
                && !is_xch_like_asset(&resolved_base)
                && !is_xch_like_asset(&resolved_quote)
            {
                return Err(SignerError::Other(
                    "resolved_assets_collide_for_non_xch_pair".to_string(),
                ));
            }
            Ok((resolved_base, resolved_quote))
        }
        _ => resolve_offer_asset_ids(msp, base_asset, quote_asset).await,
    }
}

fn leg_amounts_for_request(
    request: &BuildOfferForActionRequest,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    quote_price: f64,
) -> SignerResult<crate::offer::request::SignerOfferLegAmounts> {
    let size = i64::try_from(request.size_base_units).map_err(|_| SignerError::InvalidSizeBaseUnits)?;
    compute_signer_offer_leg_amounts(
        size,
        quote_price,
        resolved_base_asset_id,
        resolved_quote_asset_id,
        &request.action_side,
        &request.pricing,
    )
}

fn create_offer_request_from_leg(
    request: &BuildOfferForActionRequest,
    leg: &crate::offer::request::SignerOfferLegAmounts,
    expires_at_unix: u64,
) -> SignerResult<CreateOfferRequest> {
    Ok(CreateOfferRequest {
        receive_address: request.receive_address.clone(),
        offer_asset_id: leg.offer_asset_id.clone(),
        offer_amount: leg.offer_amount_mojos,
        request_asset_id: leg.request_asset_id.clone(),
        request_amount: leg.request_amount_mojos,
        offer_coin_ids: coinset::parse_coin_ids(&request.offer_coin_ids)?,
        presplit_coin_ids: Vec::new(),
        split_input_coins: request.split_input_coins,
        broadcast_split: request.broadcast_split,
        expires_at: Some(expires_at_unix),
    })
}

pub async fn build_signer_offer_for_action(
    config: SignerConfig,
    request: BuildOfferForActionRequest,
) -> SignerResult<BuildOfferForActionResult> {
    let msp = MspCoinset::for_network(&config.network, Some(&config.coinset_msp_base_url))?;
    let (resolved_base, resolved_quote) =
        resolve_signer_assets(&msp, &request.base_asset, &request.quote_asset).await?;
    let quote_price = resolve_quote_price(&request)?;
    let leg = leg_amounts_for_request(&request, &resolved_base, &resolved_quote, quote_price)?;
    let expires_at_unix = expires_at_unix_from_pricing(&request.pricing);
    let side = normalize_offer_side(&request.action_side).to_string();
    let create_request = create_offer_request_from_leg(&request, &leg, expires_at_unix)?;
    let create_result = build_vault_cat_offer(config, create_request).await?;

    Ok(BuildOfferForActionResult {
        offer_text: create_result.offer.clone(),
        side,
        expires_at_unix,
        offer_amount: leg.offer_amount_mojos,
        request_amount: leg.request_amount_mojos,
        execution_mode: create_result.execution_mode.to_string(),
        create_result: Some(create_result),
    })
}

pub async fn build_bls_offer_for_action(
    network: &str,
    master_sk: &SecretKey,
    request: BuildOfferForActionRequest,
) -> SignerResult<BuildOfferForActionResult> {
    let resolved_base = normalize_offer_asset_id(&request.base_asset);
    let resolved_quote = normalize_offer_asset_id(&request.quote_asset);
    let quote_price = resolve_quote_price(&request)?;
    let leg = leg_amounts_for_request(&request, &resolved_base, &resolved_quote, quote_price)?;
    let expires_at_unix = expires_at_unix_from_pricing(&request.pricing);
    let side = normalize_offer_side(&request.action_side).to_string();

    let bls_request = BlsOfferRequest {
        receive_address: request.receive_address.clone(),
        offer_asset_id: leg.offer_asset_id.clone(),
        offer_amount: leg.offer_amount_mojos,
        request_asset_id: leg.request_asset_id.clone(),
        request_amount: leg.request_amount_mojos,
        offer_coin_ids: request.offer_coin_ids.clone(),
        expires_at: Some(expires_at_unix),
    };
    let built = build_bls_offer_spend_bundle(network, master_sk, bls_request).await?;
    let raw_hex = built
        .spend_bundle_hex
        .strip_prefix("0x")
        .unwrap_or(built.spend_bundle_hex.as_str());
    let spend_bytes = hex::decode(raw_hex)
        .map_err(|err| SignerError::Other(format!("invalid spend_bundle_hex: {err}")))?;
    let offer_text = encode_offer_from_spend_bundle_bytes(&spend_bytes)?;

    Ok(BuildOfferForActionResult {
        offer_text,
        side,
        expires_at_unix,
        offer_amount: leg.offer_amount_mojos,
        request_amount: leg.request_amount_mojos,
        execution_mode: "bls".to_string(),
        create_result: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn expires_at_from_minutes_pricing() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        let expires = expires_at_unix_from_pricing(&json!({"strategy_offer_expiry_minutes": 5}));
        assert!(expires >= before + 300);
        assert!(expires <= before + 301);
    }
}
