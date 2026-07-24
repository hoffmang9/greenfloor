use tempfile::tempdir;

use super::target::{failed, submitted};
use super::*;
use crate::offer::types::{OfferCancelFields, OfferExecutionMode, StoredOfferCancelMetadata};
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

#[test]
fn empty_market_id_normalizes_to_unknown() {
    let target = CancelOfferTarget::Tracked {
        offer_id: "offer-1".to_string(),
        market_id: "   ".to_string(),
    };
    assert_eq!(target.normalized_market_id(), "unknown");
}

#[test]
fn outcome_accessors_cover_submitted_and_failed() {
    let target = CancelOfferTarget::Tracked {
        offer_id: "offer-1".to_string(),
        market_id: "m1".to_string(),
    };
    let ok = submitted(&target, "m1", "op-ok");
    assert!(ok.is_success());
    assert_eq!(ok.offer_id(), "offer-1");
    assert_eq!(ok.market_id(), "m1");
    assert_eq!(ok.operation_id(), "op-ok");
    assert!(ok.error().is_none());

    let err = failed(&target, "m1", "op-bad", "boom");
    assert!(!err.is_success());
    assert_eq!(err.offer_id(), "offer-1");
    assert_eq!(err.market_id(), "m1");
    assert_eq!(err.operation_id(), "op-bad");
    assert_eq!(err.error(), Some("boom"));
}

fn sufficient_metadata() -> StoredOfferCancelMetadata {
    StoredOfferCancelMetadata {
        fields: OfferCancelFields::from_direct_build("11".repeat(32), "22".repeat(32)),
        execution_mode: Some(OfferExecutionMode::Direct),
    }
}

#[test]
fn cancel_targets_need_dexie_skips_local_file() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let targets = [CancelOfferTarget::LocalFile {
        offer_id: "local".to_string(),
        market_id: "m1".to_string(),
        offer_text: "offer1qqq".to_string(),
    }];
    assert!(!cancel_targets_need_dexie_fallback(&store, &targets).expect("probe"));
}

#[test]
fn cancel_targets_need_dexie_when_tracked_metadata_incomplete() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    store
        .upsert_offer_state("offer-open", "m1", "open", Some(0))
        .expect("seed");
    let targets = [CancelOfferTarget::Tracked {
        offer_id: "offer-open".to_string(),
        market_id: "m1".to_string(),
    }];
    assert!(cancel_targets_need_dexie_fallback(&store, &targets).expect("probe"));
}

#[test]
fn cancel_targets_skip_dexie_when_tracked_metadata_sufficient() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let offer_id = "ab".repeat(32);
    let meta = sufficient_metadata();
    store
        .upsert_offer_state_with_metadata_at(
            &offer_id,
            "m1",
            "open",
            Some(0),
            &chrono::Utc::now().to_rfc3339(),
            crate::storage::OfferCancelWrite {
                fields: Some(&meta.fields),
                execution_mode: meta.execution_mode,
                ..Default::default()
            },
        )
        .expect("seed");
    let targets = [CancelOfferTarget::Tracked {
        offer_id: offer_id.clone(),
        market_id: "m1".to_string(),
    }];
    assert!(!cancel_targets_need_dexie_fallback(&store, &targets).expect("probe"));
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

#[test]
fn prepare_cancel_persist_ephemeral_skips_store() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let target = CancelOfferTarget::LocalFile {
        offer_id: "local".to_string(),
        market_id: "m1".to_string(),
        offer_text: "offer1qqq".to_string(),
    };
    let policy = prepare_cancel_persist(&store, &target, "m1", &"cd".repeat(32), false)
        .expect("infra")
        .expect("ephemeral");
    assert!(matches!(policy, CancelPersistPolicy::Ephemeral));
}

#[test]
fn prepare_cancel_persist_tracked_success_and_invalid_tx_soft_fail() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let offer_id = "ab".repeat(32);
    store
        .upsert_offer_state(&offer_id, "m1", "open", None)
        .expect("seed");
    let target = CancelOfferTarget::Tracked {
        offer_id: offer_id.clone(),
        market_id: "m1".to_string(),
    };
    let policy = prepare_cancel_persist(&store, &target, "m1", &"cd".repeat(32), true)
        .expect("infra")
        .expect("prepared");
    assert!(matches!(policy, CancelPersistPolicy::Tracked { .. }));
    assert_eq!(
        store
            .offer_state_for_id(&offer_id)
            .expect("state")
            .as_deref(),
        Some("cancel_submitted")
    );

    let soft = prepare_cancel_persist(&store, &target, "m1", "not-a-tx-id", true)
        .expect("infra")
        .expect_err("invalid tx");
    assert!(!soft.is_success());
    assert!(soft
        .error()
        .is_some_and(|e| e.contains("cancel_submitted prepare failed")));
}

#[tokio::test]
async fn tracked_cancel_with_sufficient_metadata_still_fails_before_broadcast_without_kms() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let offer_id = "ab".repeat(32);
    let meta = sufficient_metadata();
    store
        .upsert_offer_state_with_metadata_at(
            &offer_id,
            "m1",
            "open",
            Some(0),
            &chrono::Utc::now().to_rfc3339(),
            crate::storage::OfferCancelWrite {
                fields: Some(&meta.fields),
                execution_mode: meta.execution_mode,
                ..Default::default()
            },
        )
        .expect("seed");
    let target = CancelOfferTarget::Tracked {
        offer_id: offer_id.clone(),
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
    // Build reached metadata path then failed at vault/KMS — never prepared cancel_submitted.
    assert_eq!(
        store
            .offer_state_for_id(&offer_id)
            .expect("lookup")
            .as_deref(),
        Some("open")
    );
}
