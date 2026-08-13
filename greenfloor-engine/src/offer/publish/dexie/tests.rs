use mockito::Matcher;
use serde_json::json;

use super::{post_offer_phase_dexie, PostOfferPhaseDexieParams};
use crate::adapters::DexieClient;
use crate::error::{OfferError, SignerError};
use crate::offer::publish::{ExpectedPublishAssetFields, PublishAssetSide};

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

#[tokio::test]
async fn post_offer_phase_posts_and_verifies_visibility() {
    let mut server = mockito::Server::new_async().await;
    let offer_id = "offer-123";
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
    let expected = expected_fields();
    let result = post_offer_phase_dexie(PostOfferPhaseDexieParams {
        dexie: &dexie,
        offer_text: "offer1test",
        drop_only: true,
        claim_rewards: false,
        expected: &expected,
    })
    .await
    .expect("post");
    assert!(result.success());
    assert_eq!(result.offer_id(), Some(offer_id));
}

#[tokio::test]
async fn post_offer_phase_retries_http_400_invalid_offer() {
    let mut server = mockito::Server::new_async().await;
    let offer_id = "offer-400";
    let _post_invalid = server
        .mock("POST", "/v1/offers")
        .with_status(400)
        .with_body(r#"{"error_message":"Invalid Offer"}"#)
        .expect(1)
        .create_async()
        .await;
    let _post_ok = server
        .mock("POST", "/v1/offers")
        .with_status(200)
        .with_body(json!({"success": true, "id": offer_id}).to_string())
        .expect(1)
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
    let expected = expected_fields();
    let result = post_offer_phase_dexie(PostOfferPhaseDexieParams {
        dexie: &dexie,
        offer_text: "offer1test",
        drop_only: true,
        claim_rewards: false,
        expected: &expected,
    })
    .await
    .expect("post");
    assert!(result.success());
    assert_eq!(result.offer_id(), Some(offer_id));
}

#[tokio::test]
async fn post_offer_phase_fails_on_asset_mismatch() {
    let mut server = mockito::Server::new_async().await;
    let offer_id = "offer-456";
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
                    "offered": [{"id": "wrongcat"}],
                    "requested": [{"code": "xch"}],
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let dexie = DexieClient::new(server.url());
    let expected = expected_fields();
    let err = post_offer_phase_dexie(PostOfferPhaseDexieParams {
        dexie: &dexie,
        offer_text: "offer1test",
        drop_only: true,
        claim_rewards: false,
        expected: &expected,
    })
    .await
    .expect_err("asset mismatch");
    assert!(matches!(
        err,
        SignerError::Offer(OfferError::DexieOfferAssetMismatch(ref detail))
            if detail.starts_with("dexie_offer_offered_asset_missing:")
    ));
}

#[tokio::test]
async fn post_offer_phase_reposts_on_transient_visibility_404() {
    let mut server = mockito::Server::new_async().await;
    let offer_id = "offer-789";
    let _post = server
        .mock("POST", "/v1/offers")
        .with_status(200)
        .with_body(json!({"success": true, "id": offer_id}).to_string())
        .expect(2)
        .create_async()
        .await;
    let _get_404 = server
        .mock("GET", Matcher::Regex(r"/v1/offers/.*".to_string()))
        .with_status(404)
        .with_body("missing")
        .expect(4)
        .create_async()
        .await;
    let _get_ok = server
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
        .expect(1)
        .create_async()
        .await;

    let dexie = DexieClient::new(server.url());
    let expected = expected_fields();
    let result = post_offer_phase_dexie(PostOfferPhaseDexieParams {
        dexie: &dexie,
        offer_text: "offer1test",
        drop_only: true,
        claim_rewards: false,
        expected: &expected,
    })
    .await
    .expect("post");
    assert!(result.success());
    assert_eq!(result.offer_id(), Some(offer_id));
}

#[tokio::test]
async fn post_offer_phase_mismatch_does_not_repost() {
    let mut server = mockito::Server::new_async().await;
    let offer_id = "offer-local";
    let _post = server
        .mock("POST", "/v1/offers")
        .with_status(200)
        .with_body(json!({"success": true, "id": offer_id}).to_string())
        .expect(1)
        .create_async()
        .await;
    let _get = server
        .mock("GET", Matcher::Regex(r"/v1/offers/.*".to_string()))
        .with_status(200)
        .with_body(
            json!({
                "offer": {
                    "id": "other-id",
                    "offered": [{"id": "basecat"}],
                    "requested": [{"code": "xch"}],
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let dexie = DexieClient::new(server.url());
    let expected = expected_fields();
    let err = post_offer_phase_dexie(PostOfferPhaseDexieParams {
        dexie: &dexie,
        offer_text: "offer1test",
        drop_only: true,
        claim_rewards: false,
        expected: &expected,
    })
    .await
    .expect_err("mismatch");
    assert!(matches!(
        err,
        SignerError::Offer(OfferError::DexieOfferVisibilityMismatch)
    ));
}
