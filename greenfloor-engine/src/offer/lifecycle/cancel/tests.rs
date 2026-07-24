use tempfile::tempdir;

use super::*;
use crate::test_support::signer_config::test_signer_config;

#[test]
fn tracked_target_persists_state() {
    let target = CancelOfferTarget::Tracked {
        offer_id: "offer-1".to_string(),
        market_id: "m1".to_string(),
    };
    assert_eq!(target.market_id(), "m1");
    assert!(target.persists_state());
    assert!(target.offer_text().is_none());
}

#[test]
fn local_file_target_is_ephemeral() {
    let target = CancelOfferTarget::LocalFile {
        offer_id: "local-offer-1".to_string(),
        market_id: "m1".to_string(),
        offer_text: "offer1qqq".to_string(),
    };
    assert!(!target.persists_state());
    assert_eq!(target.offer_text(), Some("offer1qqq"));
}

fn fast_fail_signer() -> crate::config::SignerConfig {
    let mut signer = test_signer_config("http://coinset.test");
    signer.kms_runtime = crate::kms::KmsRuntime::test(crate::kms::KmsOverrides {
        public_key_compressed_hex: None,
        fast_fail: true,
    });
    signer
}

#[tokio::test]
async fn local_file_cancel_does_not_write_offer_state() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    store
        .upsert_offer_state("existing-offer", "m1", "open", Some(0))
        .expect("seed");
    let target = CancelOfferTarget::LocalFile {
        offer_id: "local-offer-test".to_string(),
        market_id: "m1".to_string(),
        offer_text: "offer1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq".to_string(),
    };
    let outcomes = cancel_offers_on_chain(
        &store,
        None,
        fast_fail_signer(),
        "mainnet",
        std::slice::from_ref(&target),
    )
    .await
    .expect("cancel batch");
    assert_eq!(outcomes.len(), 1);
    assert!(!outcomes[0].is_success());
    assert!(store
        .offer_state_for_id("local-offer-test")
        .expect("lookup")
        .is_none());
    assert_eq!(
        store
            .offer_state_for_id("existing-offer")
            .expect("lookup")
            .as_deref(),
        Some("open")
    );
}

#[tokio::test]
async fn tracked_cancel_failure_does_not_write_cancel_submitted() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    store
        .upsert_offer_state("offer-open", "m1", "open", Some(0))
        .expect("seed");
    let target = CancelOfferTarget::Tracked {
        offer_id: "offer-open".to_string(),
        market_id: "m1".to_string(),
    };
    let outcomes = cancel_offers_on_chain(
        &store,
        None,
        fast_fail_signer(),
        "mainnet",
        std::slice::from_ref(&target),
    )
    .await
    .expect("cancel batch");
    assert_eq!(outcomes.len(), 1);
    assert!(!outcomes[0].is_success());
    assert_eq!(
        store
            .offer_state_for_id("offer-open")
            .expect("lookup")
            .as_deref(),
        Some("open")
    );
}
