use super::*;
use crate::error::SignerError;
use tempfile::tempdir;

fn tracked_target() -> CancelOfferTarget {
    CancelOfferTarget::Tracked {
        offer_id: "ab".repeat(32),
        market_id: "m1".to_string(),
    }
}

fn ephemeral_target() -> CancelOfferTarget {
    CancelOfferTarget::LocalFile {
        offer_id: "local-1".to_string(),
        market_id: "m1".to_string(),
        offer_text: "offer1qqq".to_string(),
    }
}

fn ok_broadcast(operation_id: &str) -> BroadcastSpendBundleResult {
    BroadcastSpendBundleResult {
        status: "success".to_string(),
        operation_id: operation_id.to_string(),
    }
}

#[test]
fn ephemeral_success_submits_without_store_writes() {
    let target = ephemeral_target();
    let out = finalize_cancel_broadcast(
        &target,
        "m1",
        "unused",
        CancelPersistPolicy::Ephemeral,
        Ok(ok_broadcast(&"cd".repeat(32))),
    );
    assert!(out.is_success());
    assert_eq!(out.operation_id(), "cd".repeat(32));
}

#[test]
fn ephemeral_broadcast_failure_returns_error() {
    let target = ephemeral_target();
    let out = finalize_cancel_broadcast(
        &target,
        "m1",
        "op-1",
        CancelPersistPolicy::Ephemeral,
        Err(SignerError::Other("push_tx failed".to_string())),
    );
    assert!(!out.is_success());
    assert_eq!(out.operation_id(), "op-1");
    assert!(out.error().is_some_and(|e| e.contains("push_tx failed")));
}

#[test]
fn tracked_success_observes_mempool_tx() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let target = tracked_target();
    let offer_id = target.offer_id().to_string();
    let cancel_tx = "cd".repeat(32);
    store
        .upsert_offer_state(&offer_id, "m1", "open", None)
        .expect("seed");
    store
        .prepare_offer_cancel_submitted(&offer_id, "m1", &cancel_tx, None)
        .expect("prepare");
    let out = finalize_cancel_broadcast(
        &target,
        "m1",
        &cancel_tx,
        CancelPersistPolicy::Tracked {
            store: &store,
            prior_state: Some("open".to_string()),
        },
        Ok(ok_broadcast(&cancel_tx)),
    );
    assert!(out.is_success());
    assert!(store
        .get_tx_signal_state(std::slice::from_ref(&cancel_tx))
        .expect("tx")
        .get(&cancel_tx)
        .is_some_and(|row| row.mempool_observed_at.is_some()));
}

#[test]
fn tracked_broadcast_failure_rolls_back_prior_state() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let target = tracked_target();
    let offer_id = target.offer_id().to_string();
    let cancel_tx = "cd".repeat(32);
    store
        .upsert_offer_state(&offer_id, "m1", "open", None)
        .expect("seed");
    store
        .prepare_offer_cancel_submitted(&offer_id, "m1", &cancel_tx, None)
        .expect("prepare");
    let out = finalize_cancel_broadcast(
        &target,
        "m1",
        &cancel_tx,
        CancelPersistPolicy::Tracked {
            store: &store,
            prior_state: Some("open".to_string()),
        },
        Err(SignerError::Other("broadcast denied".to_string())),
    );
    assert!(!out.is_success());
    assert_eq!(
        store
            .offer_state_for_id(&offer_id)
            .expect("state")
            .as_deref(),
        Some("open")
    );
    assert!(out.error().is_some_and(|e| e.contains("broadcast denied")));
}

#[test]
fn tracked_broadcast_failure_defaults_prior_state_to_open() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let target = tracked_target();
    let offer_id = target.offer_id().to_string();
    let cancel_tx = "ef".repeat(32);
    store
        .upsert_offer_state(&offer_id, "m1", "pending", None)
        .expect("seed");
    store
        .prepare_offer_cancel_submitted(&offer_id, "m1", &cancel_tx, None)
        .expect("prepare");
    let out = finalize_cancel_broadcast(
        &target,
        "m1",
        &cancel_tx,
        CancelPersistPolicy::Tracked {
            store: &store,
            prior_state: None,
        },
        Err(SignerError::Other("denied".to_string())),
    );
    assert!(!out.is_success());
    assert_eq!(
        store
            .offer_state_for_id(&offer_id)
            .expect("state")
            .as_deref(),
        Some("open")
    );
}
