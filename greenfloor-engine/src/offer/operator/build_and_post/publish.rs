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
        resolved_base_asset_id: ctx.resolved_base_asset_id.clone(),
        resolved_quote_asset_id: ctx.resolved_quote_asset_id.clone(),
        created_extra: json!({}),
        cancel_fields,
        execution_mode,
    })
}

#[cfg(test)]
mod tests {
    use mockito::Matcher;
    use serde_json::json;

    use super::*;
    use crate::adapters::{DexieClient, SplashClient};
    use crate::offer::operator::build_and_post::context::sample_resolved_build_and_post_context;
    use crate::offer::publish::{ExpectedPublishAssetFields, PublishAssetSide};
    use crate::offer::types::{CreateOfferResult, OfferExecutionMode, PresplitCancelFields};

    fn expected_fields() -> ExpectedPublishAssetFields {
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
    fn offer_post_persist_record_uses_create_result_execution_mode() {
        let ctx = sample_resolved_build_and_post_context();
        let publish = PublishResult {
            success: true,
            offer_id: Some("offer-1".to_string()),
            body: json!({"success": true}),
        };
        let create = CreateOfferResult {
            offer: "offer1".to_string(),
            spend_bundle_hex: String::new(),
            selected_coin_ids: Vec::new(),
            offer_nonce: String::new(),
            execution_mode: OfferExecutionMode::PresplitNew,
            split_spend_bundle_hex: None,
            presplit_coin_id: Some("cc".repeat(64)),
            split_broadcast_status: None,
            presplit_cancel_fields: Some(PresplitCancelFields::from_presplit_build(
                "coin".to_string(),
                "puzzle".to_string(),
            )),
        };
        let record = offer_post_persist_record(&publish, "sell", "direct", &ctx, 10, Some(&create))
            .expect("record");
        assert_eq!(record.execution_mode, Some(OfferExecutionMode::PresplitNew));
        assert_eq!(record.cancel_fields.input_coin_id.as_deref(), Some("coin"));
    }

    #[tokio::test]
    async fn publish_offer_routes_to_dexie_local_server() {
        let mut server = mockito::Server::new_async().await;
        let offer_id = "offer-dexie";
        let _post = server
            .mock("POST", "/v1/offers")
            .with_status(200)
            .with_body(json!({"success": true, "id": offer_id}).to_string())
            .create_async()
            .await;
        let _get = server
            .mock("GET", Matcher::Regex(r"/v1/offers/.*".to_string()))
            .with_status(200)
            .with_body(
                json!({
                    "offer": {
                        "id": offer_id,
                        "offered": [{"id": "basecat"}],
                        "requested": [{"code": "xch"}],
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;

        let dexie = DexieClient::new(server.url());
        let result = publish_offer(
            "dexie",
            Some(&dexie),
            None,
            "offer1test",
            true,
            false,
            &expected_fields(),
        )
        .await
        .expect("publish");
        assert!(result.success);
        assert_eq!(result.offer_id.as_deref(), Some(offer_id));
    }

    #[tokio::test]
    async fn publish_offer_routes_to_splash_local_server() {
        let mut server = mockito::Server::new_async().await;
        let offer_id = "offer-splash";
        let _post = server
            .mock("POST", "/")
            .with_status(200)
            .with_body(json!({"success": true, "id": offer_id}).to_string())
            .create_async()
            .await;

        let splash = SplashClient::new(server.url());
        let result = publish_offer(
            "splash",
            None,
            Some(&splash),
            "offer1test",
            false,
            false,
            &expected_fields(),
        )
        .await
        .expect("publish");
        assert!(result.success);
        assert_eq!(result.offer_id.as_deref(), Some(offer_id));
    }

    #[tokio::test]
    async fn publish_offer_rejects_missing_adapter_and_unknown_venue() {
        let err = publish_offer(
            "dexie",
            None,
            None,
            "offer1",
            false,
            false,
            &expected_fields(),
        )
        .await
        .expect_err("missing dexie");
        assert!(err.to_string().contains("dexie adapter missing"));

        let err = publish_offer(
            "unknown",
            None,
            None,
            "offer1",
            false,
            false,
            &expected_fields(),
        )
        .await
        .expect_err("unknown venue");
        assert!(err.to_string().contains("unsupported publish venue"));
    }
}
