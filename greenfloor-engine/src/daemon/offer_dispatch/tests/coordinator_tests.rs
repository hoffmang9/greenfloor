use std::collections::BTreeMap;

use tempfile::tempdir;

use super::super::coordinator::{OfferReservationCoordinator, ReservationAcquireResult};
use crate::storage::{CycleWriteStore, OfferReservationRejectReason};

#[test]
fn coordinator_concurrent_acquires_only_one_succeeds_for_full_capacity() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = CycleWriteStore::open(&db_path).expect("coordinator");
    let coordinator = OfferReservationCoordinator::new(store, Some(300));
    let market_id = "m1";
    let wallet_id = "wallet-1";
    let requested = BTreeMap::from([("asset-a".to_string(), 100_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 100_i64)]);

    let first = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("first acquire");
    assert!(matches!(first, ReservationAcquireResult::Acquired { .. }));

    let second = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("second acquire");
    let ReservationAcquireResult::Rejected { reason } = second else {
        panic!("expected contention rejection");
    };
    assert!(reason.is_insufficient_capacity());
}

#[test]
fn coordinator_release_frees_capacity_for_next_acquire() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = CycleWriteStore::open(&db_path).expect("coordinator");
    let coordinator = OfferReservationCoordinator::new(store, Some(300));
    let market_id = "m1";
    let wallet_id = "wallet-1";
    let requested = BTreeMap::from([("asset-a".to_string(), 50_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 50_i64)]);

    let acquired = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("acquire");
    let ReservationAcquireResult::Acquired { reservation_id } = acquired else {
        panic!("expected acquire success");
    };
    coordinator
        .release(&reservation_id, "released_success")
        .expect("release");

    let after_release = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("acquire after release");
    assert!(matches!(
        after_release,
        ReservationAcquireResult::Acquired { .. }
    ));
}

#[test]
fn coordinator_partial_acquire_rejects_when_requested_exceeds_available() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = CycleWriteStore::open(&db_path).expect("coordinator");
    let coordinator = OfferReservationCoordinator::new(store, Some(300));
    let requested = BTreeMap::from([("asset-a".to_string(), 80_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 50_i64)]);

    let result = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("acquire");
    let ReservationAcquireResult::Rejected { reason } = result else {
        panic!("expected rejection");
    };
    assert_eq!(
        reason,
        OfferReservationRejectReason::InsufficientCapacity {
            asset_id: "asset-a".to_string(),
            available: 50,
            reserved: 0,
            needed: 80,
        }
    );
}

#[test]
fn coordinator_second_acquire_blocked_until_release() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = CycleWriteStore::open(&db_path).expect("coordinator");
    let coordinator = OfferReservationCoordinator::new(store, Some(300));
    let requested = BTreeMap::from([("asset-a".to_string(), 40_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 40_i64)]);

    let first = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("first acquire");
    let ReservationAcquireResult::Acquired { reservation_id } = first else {
        panic!("expected first acquire success");
    };

    let blocked = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("blocked acquire");
    assert!(matches!(blocked, ReservationAcquireResult::Rejected { .. }));

    coordinator
        .release(&reservation_id, "released_success")
        .expect("release");

    let after_release = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("acquire after release");
    assert!(matches!(
        after_release,
        ReservationAcquireResult::Acquired { .. }
    ));
}

#[test]
fn coordinator_multi_asset_acquire_requires_all_assets() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = CycleWriteStore::open(&db_path).expect("coordinator");
    let coordinator = OfferReservationCoordinator::new(store, Some(300));
    let requested = BTreeMap::from([
        ("asset-a".to_string(), 10_i64),
        ("asset-b".to_string(), 10_i64),
    ]);
    let available = BTreeMap::from([
        ("asset-a".to_string(), 10_i64),
        ("asset-b".to_string(), 5_i64),
    ]);

    let result = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("acquire");
    assert!(matches!(result, ReservationAcquireResult::Rejected { .. }));
}
