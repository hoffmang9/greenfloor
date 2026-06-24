use serde_json::{json, Value};

use crate::adapters::{dexie_offer_view_url, DexieClient, SplashClient};
use crate::error::{SignerError, SignerResult};
use crate::offer::publish::{
    post_offer_phase_dexie, ExpectedPublishAssetFields, PostOfferPhaseDexieParams,
};
use crate::offer::types::{CreateOfferResult, OfferExecutionMode};
use crate::storage::OfferPostPersistRecord;

use super::context::ResolvedBuildAndPostContext;
use super::types::PublishResult;

pub(super) async fn publish_offer(
    publish_venue: &str,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
    offer_text: &str,
    drop_only: bool,
    claim_rewards: bool,
    expected: &ExpectedPublishAssetFields,
) -> SignerResult<PublishResult> {
    match publish_venue {
        "dexie" => {
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
        "splash" => {
            let splash = splash.ok_or_else(|| {
                SignerError::Other("splash adapter missing for splash publish".to_string())
            })?;
            Ok(PublishResult::from_splash_response(
                splash.post_offer(offer_text).await?,
            ))
        }
        other => Err(SignerError::Other(format!(
            "unsupported publish venue: {other}"
        ))),
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
        .and_then(|result| result.presplit_cancel_fields.clone())
        .unwrap_or_default();
    let execution_mode = create_result
        .map(|result| result.execution_mode)
        .or_else(|| OfferExecutionMode::parse_db(execution_mode));
    Some(OfferPostPersistRecord {
        offer_id,
        market_id: ctx.market.market_id.clone(),
        side: side.to_string(),
        size_base_units,
        publish_venue: ctx.publish_venue.clone(),
        resolved_base_asset_id: ctx.offer_assets.base_asset_id.clone(),
        resolved_quote_asset_id: ctx.offer_assets.quote_asset_id.clone(),
        created_extra: json!({}),
        cancel_fields,
        execution_mode,
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

    #[tokio::test]
    async fn publish_offer_rejects_missing_adapters_and_unknown_venue() {
        let expected = sample_expected_fields();
        let err = publish_offer("dexie", None, None, "offer1", false, false, &expected)
            .await
            .expect_err("missing dexie");
        assert!(err.to_string().contains("dexie adapter missing"));

        let err = publish_offer("splash", None, None, "offer1", false, false, &expected)
            .await
            .expect_err("missing splash");
        assert!(err.to_string().contains("splash adapter missing"));

        let err = publish_offer("unknown", None, None, "offer1", false, false, &expected)
            .await
            .expect_err("unknown venue");
        assert!(err.to_string().contains("unsupported publish venue"));
    }
}
