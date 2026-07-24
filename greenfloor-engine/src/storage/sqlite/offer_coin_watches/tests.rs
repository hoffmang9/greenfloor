use std::collections::HashMap;

use tempfile::tempdir;

use super::super::SqliteStore;
use super::WatchMatchKind;

#[test]
fn replace_and_list_offer_coin_watches() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let coin = "ab".repeat(32);
    let p2 = "cd".repeat(32);
    store
        .replace_offer_coin_watches(
            "offer1",
            "m1",
            std::slice::from_ref(&coin),
            std::slice::from_ref(&p2),
        )
        .expect("replace");
    let watched = store.list_watched_coin_ids_for_market("m1").expect("list");
    assert!(watched.contains(&coin));
    assert!(!watched.contains(&p2));
    let watched_p2s = store.list_watched_p2s_for_market("m1").expect("p2s");
    assert!(watched_p2s.contains(&p2));
    assert!(!watched_p2s.contains(&coin));
    let offers = store
        .list_offer_ids_for_watched_coin(&coin)
        .expect("offers");
    assert_eq!(offers, vec!["offer1".to_string()]);
    let (coins, p2s) = store
        .list_offer_coin_watches_for_offer("offer1")
        .expect("by offer");
    assert_eq!(coins, vec![coin.clone()]);
    assert_eq!(p2s, vec![p2]);
    store.clear_offer_coin_watches("offer1").expect("clear");
    assert!(store
        .list_watched_coin_ids_for_market("m1")
        .expect("list")
        .is_empty());
    assert!(store
        .list_watched_p2s_for_market("m1")
        .expect("p2s")
        .is_empty());
}

#[test]
fn list_market_ids_for_watched_keys() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let p2 = "cd".repeat(32);
    store
        .replace_offer_coin_watches("offer1", "m1", &[], std::slice::from_ref(&p2))
        .expect("replace");
    let markets = store
        .list_market_ids_for_watched_keys(&[p2])
        .expect("markets");
    assert_eq!(markets, vec!["m1".to_string()]);
}

#[test]
fn replace_rejects_when_all_watch_keys_invalid() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let err = store
        .replace_offer_coin_watches(
            "offer1",
            "m1",
            &["short".to_string()],
            &["also-bad".to_string()],
        )
        .expect_err("invalid keys");
    assert!(err.to_string().contains("all"), "unexpected error: {err}");
    assert!(store
        .list_watched_coin_ids_for_market("m1")
        .expect("list")
        .is_empty());
}

#[test]
fn replace_with_empty_inputs_clears_watches() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let coin = "ab".repeat(32);
    store
        .replace_offer_coin_watches("offer1", "m1", std::slice::from_ref(&coin), &[])
        .expect("seed");
    store
        .replace_offer_coin_watches("offer1", "m1", &[], &[])
        .expect("clear via empty replace");
    assert!(store
        .list_watched_coin_ids_for_market("m1")
        .expect("list")
        .is_empty());
}

#[test]
fn match_watch_keys_aggregates_kinds() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let offer_a = "aa".repeat(32);
    let offer_b = "bb".repeat(32);
    let coin = "11".repeat(32);
    let p2 = "22".repeat(32);
    store
        .upsert_offer_state(&offer_a, "m1", "open", None)
        .expect("a");
    store
        .upsert_offer_state(&offer_b, "m1", "mempool_observed", None)
        .expect("b");
    store
        .replace_offer_coin_watches(
            &offer_a,
            "m1",
            std::slice::from_ref(&coin),
            std::slice::from_ref(&p2),
        )
        .expect("watch a");
    store
        .replace_offer_coin_watches(&offer_b, "m1", std::slice::from_ref(&coin), &[])
        .expect("watch b");
    let hits = store.match_watch_keys(&[coin, p2]).expect("hits");
    assert_eq!(hits.len(), 2);
    let mut by_id: HashMap<_, _> = hits
        .into_iter()
        .map(|hit| (hit.row.offer_id, (hit.row.state, hit.kind)))
        .collect();
    assert_eq!(
        by_id.remove(&offer_a),
        Some(("open".to_string(), WatchMatchKind::Both))
    );
    assert_eq!(
        by_id.remove(&offer_b),
        Some(("mempool_observed".to_string(), WatchMatchKind::Coin))
    );
}

#[test]
fn watch_match_kind_merge() {
    assert_eq!(
        WatchMatchKind::Coin.merge(WatchMatchKind::Coin),
        WatchMatchKind::Coin
    );
    assert_eq!(
        WatchMatchKind::P2.merge(WatchMatchKind::P2),
        WatchMatchKind::P2
    );
    assert_eq!(
        WatchMatchKind::Coin.merge(WatchMatchKind::P2),
        WatchMatchKind::Both
    );
    assert_eq!(
        WatchMatchKind::Both.merge(WatchMatchKind::Coin),
        WatchMatchKind::Both
    );
}
