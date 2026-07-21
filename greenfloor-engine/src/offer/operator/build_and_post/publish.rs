use serde_json::{json, Value};

use crate::adapters::{dexie_offer_view_url, DexieClient, SplashClient};
use crate::coinset::push_offer_text;
use crate::error::{SignerError, SignerResult};
use crate::offer::publish::{
    post_offer_phase_dexie, ExpectedPublishAssetFields, PostOfferPhaseDexieParams,
};
use crate::offer::types::{CreateOfferResult, OfferExecutionMode};
use crate::storage::OfferPostPersistRecord;

use super::context::ResolvedBuildAndPostContext;
use super::types::PublishResult;

#[derive(Debug, Clone, Copy)]
pub(super) struct CoinsetPublishEndpoint<'a> {
    pub network: &'a str,
    pub base_url: Option<&'a str>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn publish_offer(
    publish_venue: &str,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
    coinset: Option<CoinsetPublishEndpoint<'_>>,
    offer_text: &str,
    drop_only: bool,
    claim_rewards: bool,
    expected: &ExpectedPublishAssetFields,
) -> SignerResult<PublishResult> {
    match crate::config::Venue::parse(publish_venue)? {
        crate::config::Venue::Coinset => {
            let endpoint = coinset.ok_or_else(|| {
                SignerError::Other("coinset endpoint missing for coinset publish".to_string())
            })?;
            let payload = push_offer_text(endpoint.network, endpoint.base_url, offer_text).await?;
            Ok(PublishResult::from_coinset_push_offer(payload))
        }
        crate::config::Venue::Dexie => {
            let dexie = dexie.ok_or_else(|| {
                SignerError::Other("dexie adapter missing for dexie publish".to_string())
            })?;
            Ok(PublishResult::from_dexie_response(
                post_offer_phase_dexie(PostOfferPhaseDexieParams {
                    dexie,
                    offer_text,
                    drop_only,
                    claim_rewards,
                    expected,
                })
                .await?,
            ))
        }
        crate::config::Venue::Splash => {
            let splash = splash.ok_or_else(|| {
                SignerError::Other("splash adapter missing for splash publish".to_string())
            })?;
            Ok(PublishResult::from_splash_response(
                splash.post_offer(offer_text).await?,
            ))
        }
    }
}

pub(super) fn finalize_publish_payload(
    publish: PublishResult,
    execution_mode: &str,
    timing_ms: Value,
    dexie_base_url: Option<&str>,
) -> Value {
    let mut payload = publish.body;
    if let Value::Object(obj) = &mut payload {
        obj.insert("execution_mode".to_string(), json!(execution_mode));
        obj.insert("timing_ms".to_string(), timing_ms);
        if publish.success {
            if let (Some(base_url), Some(offer_id)) = (dexie_base_url, publish.offer_id.as_deref())
            {
                obj.insert(
                    "offer_view_url".to_string(),
                    Value::String(dexie_offer_view_url(base_url, offer_id)),
                );
            }
        }
    }
    payload
}

pub(super) fn offer_post_persist_record(
    publish: &PublishResult,
    side: &str,
    execution_mode: &str,
    ctx: &ResolvedBuildAndPostContext,
    size_base_units: u64,
    create_result: Option<&CreateOfferResult>,
) -> Option<OfferPostPersistRecord> {
    if !publish.success {
        return None;
    }
    let offer_id = publish.offer_id.clone()?;
    let cancel_fields = create_result
        .map(|result| result.cancel_fields.clone())
        .unwrap_or_default();
    let execution_mode = create_result
        .map(|result| result.execution_mode)
        .or_else(|| OfferExecutionMode::parse_db(execution_mode));
    let mut watched_coin_ids = create_result
        .map(|result| result.selected_coin_ids.clone())
        .unwrap_or_default();
    if let Some(presplit) = create_result.and_then(|result| result.presplit_coin_id.clone()) {
        watched_coin_ids.push(presplit);
    }
    if let Some(input) = cancel_fields.input_coin_id.clone() {
        watched_coin_ids.push(input);
    }
    watched_coin_ids.sort();
    watched_coin_ids.dedup();
    let mut watched_p2s = Vec::new();
    // On-chain maker puzzle hash only (not fixed_delegated CONDITIONS hash).
    if let Some(p2) = cancel_fields.maker_puzzle_hash.clone() {
        watched_p2s.push(p2);
    }
    // Do not seed shared market inventory receive/CAT outer p2s into per-offer
    // watches: those hashes are common to every open offer on the market and would
    // promote all of them on any deposit/spend. Inventory freshness uses
    // InventoryP2Index separately; lifecycle watch hits need maker-specific keys.
    watched_p2s.sort();
    watched_p2s.dedup();
    Some(OfferPostPersistRecord {
        offer_id,
        market_id: ctx.gated.market_row.market_id.clone(),
        side: side.to_string(),
        size_base_units,
        publish_venue: ctx.publish_venue.clone(),
        resolved_base_asset_id: ctx.offer_assets.base_asset_id.clone(),
        resolved_quote_asset_id: ctx.offer_assets.quote_asset_id.clone(),
        created_extra: json!({}),
        cancel_fields,
        execution_mode,
        watched_coin_ids,
        watched_p2s,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::offer::publish::{ExpectedPublishAssetFields, PublishAssetSide};

    fn sample_expected_fields() -> ExpectedPublishAssetFields {
        ExpectedPublishAssetFields {
            offered: PublishAssetSide {
                asset_id: "basecat".to_string(),
                symbol: "A1".to_string(),
            },
            requested: PublishAssetSide {
                asset_id: "xch".to_string(),
                symbol: "xch".to_string(),
            },
        }
    }

    #[test]
    fn finalize_publish_payload_adds_execution_mode_and_view_url() {
        let publish = PublishResult {
            success: true,
            offer_id: Some("offer-99".to_string()),
            body: json!({"success": true, "id": "offer-99"}),
        };
        let payload = finalize_publish_payload(
            publish,
            "direct",
            json!({"total_ms": 12}),
            Some("https://api.dexie.space"),
        );
        assert_eq!(payload.get("execution_mode"), Some(&json!("direct")));
        assert_eq!(payload.get("timing_ms"), Some(&json!({"total_ms": 12})));
        assert_eq!(
            payload
                .get("offer_view_url")
                .and_then(|value| value.as_str()),
            Some("https://dexie.space/offers/offer-99")
        );
    }

    #[test]
    fn from_coinset_push_offer_normalizes_hex_offer_id() {
        let expected = "ab".repeat(32);
        let offer_id = format!("0x{expected}");
        let publish = PublishResult::from_coinset_push_offer(json!({
            "success": true,
            "offer_id": offer_id,
            "splash_enabled": true,
            "splash_published": false,
            "splash_error": "relay_failed",
        }));
        assert!(publish.success);
        assert_eq!(publish.offer_id.as_deref(), Some(expected.as_str()));
        assert_eq!(
            publish.body.get("splash_error").and_then(Value::as_str),
            Some("relay_failed")
        );
    }

    #[test]
    fn from_coinset_push_offer_rejects_non_64_hex_offer_id() {
        let publish = PublishResult::from_coinset_push_offer(json!({
            "success": true,
            "offer_id": "not-a-trade-id",
        }));
        assert!(!publish.success);
        assert!(publish.offer_id.is_none());

        let short = PublishResult::from_coinset_push_offer(json!({
            "success": true,
            "offer_id": "abcd",
        }));
        assert!(!short.success);
        assert!(short.offer_id.is_none());
    }

    #[tokio::test]
    async fn publish_offer_rejects_missing_adapters_and_unknown_venue() {
        let expected = sample_expected_fields();
        let err = publish_offer(
            "coinset", None, None, None, "offer1", false, false, &expected,
        )
        .await
        .expect_err("missing coinset");
        assert!(err.to_string().contains("coinset endpoint missing"));

        let err = publish_offer("dexie", None, None, None, "offer1", false, false, &expected)
            .await
            .expect_err("missing dexie");
        assert!(err.to_string().contains("dexie adapter missing"));

        let err = publish_offer(
            "splash", None, None, None, "offer1", false, false, &expected,
        )
        .await
        .expect_err("missing splash");
        assert!(err.to_string().contains("splash adapter missing"));

        let err = publish_offer(
            "unknown", None, None, None, "offer1", false, false, &expected,
        )
        .await
        .expect_err("unknown venue");
        assert!(
            err.to_string().contains("offer venue must be"),
            "unexpected error: {err}"
        );
    }
}
