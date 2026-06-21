use std::collections::BTreeMap;

use tempfile::tempdir;

use super::super::coordinator::OfferReservationCoordinator;

#[test]
fn coordinator_concurrent_acquires_only_one_succeeds_for_full_capacity() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300)).expect("coordinator");
    let market_id = "m1";
    let wallet_id = "wallet-1";
    let requested = BTreeMap::from([("asset-a".to_string(), 100_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 100_i64)]);

    let first = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("first acquire");
    assert!(first.ok);
    assert!(first.reservation_id.is_some());

    let second = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("second acquire");
    assert!(!second.ok);
    let error = second.error.expect("contention error");
    assert!(error.contains("reservation_insufficient"));
}

#[test]
fn coordinator_release_frees_capacity_for_next_acquire() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300)).expect("coordinator");
    let market_id = "m1";
    let wallet_id = "wallet-1";
    let requested = BTreeMap::from([("asset-a".to_string(), 50_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 50_i64)]);

    let acquired = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("acquire");
    let reservation_id = acquired.reservation_id.expect("reservation id");
    coordinator
        .release(&reservation_id, "released_success")
        .expect("release");

    let after_release = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("acquire after release");
    assert!(after_release.ok);
}

#[test]
fn coordinator_partial_acquire_rejects_when_requested_exceeds_available() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300)).expect("coordinator");
    let requested = BTreeMap::from([("asset-a".to_string(), 80_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 50_i64)]);

    let result = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("acquire");
    assert!(!result.ok);
    assert!(result
        .error
        .expect("error")
        .contains("reservation_insufficient"));
}

#[test]
fn coordinator_second_acquire_blocked_until_release() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300)).expect("coordinator");
    let requested = BTreeMap::from([("asset-a".to_string(), 40_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 40_i64)]);

    let first = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("first acquire");
    assert!(first.ok);
    let reservation_id = first.reservation_id.expect("reservation id");

    let blocked = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("blocked acquire");
    assert!(!blocked.ok);

    coordinator
        .release(&reservation_id, "released_success")
        .expect("release");

    let after_release = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("acquire after release");
    assert!(after_release.ok);
}

#[test]
fn coordinator_multi_asset_acquire_requires_all_assets() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300)).expect("coordinator");
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
    assert!(!result.ok);
}
