use mockito::Matcher;
use serde_json::json;

use super::{poll_dexie_offer_visibility_once, post_offer_phase_dexie, PostOfferPhaseDexieParams};
use crate::adapters::DexieClient;
use crate::offer::publish::ExpectedPublishAssetFieldsRef;

fn expected_fields<'a>() -> ExpectedPublishAssetFieldsRef<'a> {
    ExpectedPublishAssetFieldsRef {
        expected_offered_asset_id: "basecat",
        expected_offered_symbol: "A1",
        expected_requested_asset_id: "xch",
        expected_requested_symbol: "xch",
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
    let result = post_offer_phase_dexie(PostOfferPhaseDexieParams {
        dexie: &dexie,
        offer_text: "offer1test",
        drop_only: true,
        claim_rewards: false,
        expected: expected_fields(),
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
    let result = post_offer_phase_dexie(PostOfferPhaseDexieParams {
        dexie: &dexie,
        offer_text: "offer1test",
        drop_only: true,
        claim_rewards: false,
        expected: expected_fields(),
    })
    .await
    .expect("post");
    assert!(!result.success());
    assert!(result
        .error_text()
        .starts_with("dexie_offer_offered_asset_missing:"));
    assert_eq!(result.offer_id(), Some(offer_id));
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
    let result = post_offer_phase_dexie(PostOfferPhaseDexieParams {
        dexie: &dexie,
        offer_text: "offer1test",
        drop_only: true,
        claim_rewards: false,
        expected: expected_fields(),
    })
    .await
    .expect("post");
    assert!(result.success());
    assert_eq!(result.offer_id(), Some(offer_id));
}

#[tokio::test]
async fn poll_visibility_once_retries_on_http_error_payload() {
    let mut server = mockito::Server::new_async().await;
    let offer_id = "offer-404";
    let _get = server
        .mock("GET", Matcher::Regex(r"/v1/offers/.*".to_string()))
        .with_status(404)
        .with_body("missing")
        .create_async()
        .await;

    let dexie = DexieClient::new(server.url());
    let poll = poll_dexie_offer_visibility_once(&dexie, offer_id, expected_fields()).await;
    match poll {
        super::OfferVisibilityPoll::Retry(error) => {
            assert!(error.contains("dexie_http_error:404"));
        }
        other => panic!("expected retry, got {other:?}"),
    }
}

#[test]
fn dexie_publish_failure_overwrites_success_and_error() {
    use crate::adapters::DexieResponse;

    let failed = super::dexie_publish_failure(
        DexieResponse::from_value(json!({"success": true, "id": "offer-1"})),
        "dexie_offer_offered_asset_missing:expected_asset=cat:expected_symbol=cat",
    );
    assert!(!failed.success());
    assert_eq!(
        failed.error_text(),
        "dexie_offer_offered_asset_missing:expected_asset=cat:expected_symbol=cat"
    );
    assert_eq!(failed.offer_id(), Some("offer-1"));
}
