use std::collections::BTreeMap;

use chrono::{Duration, Utc};
use greenfloor_engine::storage::{
    OfferReservationAcquireOutcome, OfferReservationLeaseRequest, OfferReservationRejectReason,
};

use crate::common::{acquire_test_reservation_lease, open_store};

#[test]
fn offer_reservation_lease_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut amounts = BTreeMap::default();
    amounts.insert("xch".to_string(), 1000);
    amounts.insert("cat-1".to_string(), 2500);
    acquire_test_reservation_lease(&store, "res-1", "vault-1", &amounts, 120);
    let rows = store
        .list_offer_reservation_leases(Some("res-1"))
        .expect("list");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.iter()
            .map(|row| row.asset_id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        ["cat-1", "xch"].into_iter().collect()
    );
    let reserved = store
        .get_offer_reserved_amounts_by_asset("vault-1")
        .expect("reserved");
    assert_eq!(reserved.get("xch"), Some(&1000));
    assert_eq!(reserved.get("cat-1"), Some(&2500));
}

#[test]
fn offer_reservation_release_clears_reserved_amount() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut amounts = BTreeMap::default();
    amounts.insert("xch".to_string(), 700);
    acquire_test_reservation_lease(&store, "res-2", "vault-1", &amounts, 120);
    assert_eq!(
        store
            .release_offer_reservation_lease("res-2", "released_success")
            .expect("release"),
        1
    );
    let reserved = store
        .get_offer_reserved_amounts_by_asset("vault-1")
        .expect("reserved");
    assert_eq!(reserved.get("xch").copied().unwrap_or(0), 0);
    let rows = store
        .list_offer_reservation_leases(Some("res-2"))
        .expect("rows");
    assert_eq!(rows[0].status, "released_success");
    assert!(rows[0].released_at.is_some());
}

#[test]
fn offer_reservation_expiry_marks_active_rows_expired() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut amounts = BTreeMap::default();
    amounts.insert("xch".to_string(), 120);
    acquire_test_reservation_lease(&store, "res-3", "vault-1", &amounts, 1);
    assert_eq!(
        store
            .expire_offer_reservation_leases(Some(Utc::now() + Duration::hours(1)))
            .expect("expire"),
        1
    );
    let rows = store
        .list_offer_reservation_leases(Some("res-3"))
        .expect("rows");
    assert_eq!(rows[0].status, "expired");
    assert!(rows[0].released_at.is_some());
}

#[test]
fn try_acquire_offer_reservation_lease_rejects_insufficient_capacity() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut requested = BTreeMap::default();
    requested.insert("asset".to_string(), 100);
    let mut available = BTreeMap::default();
    available.insert("asset".to_string(), 50);
    let outcome = store
        .try_acquire_offer_reservation_lease(&OfferReservationLeaseRequest {
            reservation_id: "res-4",
            market_id: "m1",
            wallet_id: "vault-1",
            requested_amounts: &requested,
            available_amounts: &available,
            lease_seconds: 120,
            now: None,
        })
        .expect("try acquire");
    let OfferReservationAcquireOutcome::Rejected(reason) = outcome else {
        panic!("expected rejection, got {outcome:?}");
    };
    assert_eq!(
        reason,
        OfferReservationRejectReason::InsufficientCapacity {
            asset_id: "asset".to_string(),
            available: 50,
            reserved: 0,
            needed: 100,
        }
    );
    assert!(store
        .list_offer_reservation_leases(Some("res-4"))
        .expect("rows")
        .is_empty());
}

#[test]
fn try_acquire_offer_reservation_lease_persists_rows_on_success() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut requested = BTreeMap::default();
    requested.insert("asset".to_string(), 100);
    requested.insert("xch".to_string(), 20);
    let mut available = BTreeMap::default();
    available.insert("asset".to_string(), 150);
    available.insert("xch".to_string(), 40);
    assert!(matches!(
        store
            .try_acquire_offer_reservation_lease(&OfferReservationLeaseRequest {
                reservation_id: "res-5",
                market_id: "m1",
                wallet_id: "vault-1",
                requested_amounts: &requested,
                available_amounts: &available,
                lease_seconds: 120,
                now: None,
            })
            .expect("try acquire"),
        OfferReservationAcquireOutcome::Acquired
    ));
    assert_eq!(
        store
            .list_offer_reservation_leases(Some("res-5"))
            .expect("rows")
            .len(),
        2
    );
    let reserved = store
        .get_offer_reserved_amounts_by_asset("vault-1")
        .expect("reserved");
    assert_eq!(reserved.get("asset"), Some(&100));
    assert_eq!(reserved.get("xch"), Some(&20));
}

#[test]
fn prune_offer_reservation_leases_removes_old_inactive_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut amounts = BTreeMap::default();
    amounts.insert("asset".to_string(), 10);
    acquire_test_reservation_lease(&store, "res-6", "vault-1", &amounts, 120);
    store
        .release_offer_reservation_lease("res-6", "released_success")
        .expect("release");
    assert_eq!(
        store
            .prune_offer_reservation_leases(Utc::now() + Duration::hours(1))
            .expect("prune"),
        1
    );
    assert!(store
        .list_offer_reservation_leases(Some("res-6"))
        .expect("rows")
        .is_empty());
}
