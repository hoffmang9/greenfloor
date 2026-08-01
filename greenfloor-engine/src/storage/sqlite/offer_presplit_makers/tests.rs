use super::super::offer_cancel::{OfferCancelWrite, OfferListingWrite};
use super::super::SqliteStore;
use super::{MAKER_CLAIM_STALE_SECONDS, REUSABLE_PAGE_SIZE};
use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
use tempfile::tempdir;

fn insert_expired_maker(store: &SqliteStore, offer_id: &str, coin_suffix: u8) {
    insert_expired_maker_with_side(store, offer_id, coin_suffix, Some("sell"));
}

fn insert_expired_maker_with_side(
    store: &SqliteStore,
    offer_id: &str,
    coin_suffix: u8,
    offer_side: Option<&str>,
) {
    let coin = format!("{coin_suffix:02x}").repeat(32);
    let fields =
        OfferCancelFields::from_presplit_build(coin.clone(), "bb".repeat(32), "cc".repeat(32));
    store
        .upsert_offer_state_with_metadata_at(
            offer_id,
            "m1",
            "expired",
            Some(6),
            "2026-01-01T00:00:00Z",
            OfferCancelWrite {
                fields: Some(&fields),
                execution_mode: Some(OfferExecutionMode::PresplitExisting),
                listing: OfferListingWrite {
                    publish_venue: Some("dexie"),
                    listing_expires_at: Some(1_700_000_000),
                    size_base_units: Some(10),
                    offer_nonce: Some(&"dd".repeat(32)),
                    offer_side,
                },
                ..OfferCancelWrite::default()
            },
        )
        .expect("upsert");
}

#[test]
fn soft_listing_expiry_and_expired_maker_queries() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let fields =
        OfferCancelFields::from_presplit_build("aa".repeat(32), "bb".repeat(32), "cc".repeat(32));
    let nonce = "dd".repeat(32);
    store
        .upsert_offer_state_with_metadata_at(
            "offer-soft",
            "m1",
            "open",
            None,
            "2026-01-01T00:00:00Z",
            OfferCancelWrite {
                fields: Some(&fields),
                execution_mode: Some(OfferExecutionMode::PresplitExisting),
                listing: OfferListingWrite {
                    publish_venue: Some("dexie"),
                    listing_expires_at: Some(1_700_000_000),
                    size_base_units: Some(10),
                    offer_nonce: Some(nonce.as_str()),
                    offer_side: Some("sell"),
                },
                ..OfferCancelWrite::default()
            },
        )
        .expect("upsert");
    let past = store
        .list_open_offers_past_listing_expiry("m1", 1_700_000_001)
        .expect("past");
    assert_eq!(past.len(), 1);
    assert_eq!(past[0].offer_id, "offer-soft");
    store
        .upsert_offer_state("offer-soft", "m1", "expired", Some(6))
        .expect("expire");
    let expired = store
        .list_reusable_presplit_makers_for_size("m1", 10, Some("sell"))
        .expect("expired makers");
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].offer_nonce.as_deref(), Some(nonce.as_str()));
    let unreturned = store
        .list_unreturned_presplit_makers(Some("m1"))
        .expect("unreturned");
    assert_eq!(unreturned.len(), 1);
    assert_eq!(unreturned[0].cancel_input_coin_id, "aa".repeat(32));
    assert_eq!(unreturned[0].fixed_delegated_puzzle_hash, "bb".repeat(32));
}

#[test]
fn soft_expire_mark_list_skips_rows_without_presplit_cancel_metadata() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    store
        .upsert_offer_state_with_metadata_at(
            "offer-direct",
            "m1",
            "open",
            None,
            "2026-01-01T00:00:00Z",
            OfferCancelWrite {
                fields: None,
                execution_mode: Some(OfferExecutionMode::Direct),
                listing: OfferListingWrite {
                    publish_venue: Some("dexie"),
                    listing_expires_at: Some(1_700_000_000),
                    size_base_units: Some(10),
                    offer_nonce: Some(&"dd".repeat(32)),
                    offer_side: Some("sell"),
                },
                ..OfferCancelWrite::default()
            },
        )
        .expect("upsert direct");
    let fields =
        OfferCancelFields::from_presplit_build("aa".repeat(32), "bb".repeat(32), "cc".repeat(32));
    store
        .upsert_offer_state_with_metadata_at(
            "offer-presplit",
            "m1",
            "open",
            None,
            "2026-01-01T00:00:00Z",
            OfferCancelWrite {
                fields: Some(&fields),
                execution_mode: Some(OfferExecutionMode::PresplitNew),
                listing: OfferListingWrite {
                    publish_venue: Some("dexie"),
                    listing_expires_at: Some(1_700_000_000),
                    size_base_units: Some(10),
                    offer_nonce: Some(&"dd".repeat(32)),
                    offer_side: Some("sell"),
                },
                ..OfferCancelWrite::default()
            },
        )
        .expect("upsert presplit");
    let past = store
        .list_open_offers_past_listing_expiry("m1", 1_700_000_001)
        .expect("past");
    assert_eq!(past.len(), 1);
    assert_eq!(past[0].offer_id, "offer-presplit");
}

#[test]
fn soft_expire_mark_includes_mempool_observed_past_listing_expiry() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let fields =
        OfferCancelFields::from_presplit_build("aa".repeat(32), "bb".repeat(32), "cc".repeat(32));
    store
        .upsert_offer_state_with_metadata_at(
            "offer-mempool",
            "m1",
            "mempool_observed",
            Some(0),
            "2026-01-01T00:00:00Z",
            OfferCancelWrite {
                fields: Some(&fields),
                execution_mode: Some(OfferExecutionMode::PresplitExisting),
                listing: OfferListingWrite {
                    publish_venue: Some("dexie"),
                    listing_expires_at: Some(1_700_000_000),
                    size_base_units: Some(10),
                    offer_nonce: Some(&"dd".repeat(32)),
                    offer_side: Some("sell"),
                },
                ..OfferCancelWrite::default()
            },
        )
        .expect("upsert");
    let past = store
        .list_open_offers_past_listing_expiry("m1", 1_700_000_001)
        .expect("past");
    assert_eq!(past.len(), 1);
    assert_eq!(past[0].offer_id, "offer-mempool");
    assert_eq!(past[0].state, "mempool_observed");
}

#[test]
fn reusable_makers_treat_null_offer_side_as_sell() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    insert_expired_maker_with_side(&store, "offer-null-side", 0xaa, None);
    insert_expired_maker_with_side(&store, "offer-buy", 0xbb, Some("buy"));
    let sell = store
        .list_reusable_presplit_makers_for_size("m1", 10, Some("sell"))
        .expect("sell");
    let sell_ids: Vec<_> = sell.iter().map(|row| row.offer_id.as_str()).collect();
    assert_eq!(sell_ids, vec!["offer-null-side"]);
    let buy = store
        .list_reusable_presplit_makers_for_size("m1", 10, Some("buy"))
        .expect("buy");
    let buy_ids: Vec<_> = buy.iter().map(|row| row.offer_id.as_str()).collect();
    assert_eq!(buy_ids, vec!["offer-buy"]);
}

#[test]
fn try_claim_expired_maker_is_cas_then_finalize() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    insert_expired_maker(&store, "offer-a", 0xaa);
    assert!(store
        .try_claim_expired_maker("offer-a", "m1", "token-a")
        .expect("first"));
    assert!(!store
        .try_claim_expired_maker("offer-a", "m1", "token-b")
        .expect("second"));
    let expired = store.list_expired_presplit_makers("m1").expect("list");
    assert!(expired.is_empty());
    assert!(store
        .restore_maker_claim("offer-a", "m1", "token-a")
        .expect("restore"));
    let restored = store.list_expired_presplit_makers("m1").expect("list");
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].offer_id, "offer-a");
    assert!(store
        .try_claim_expired_maker("offer-a", "m1", "token-c")
        .expect("reclaim"));
    assert!(store
        .finalize_maker_claim("offer-a", "m1", "token-c")
        .expect("finalize"));
    assert!(store
        .list_expired_presplit_makers("m1")
        .expect("list")
        .is_empty());
}

#[test]
fn maker_claim_finalize_requires_matching_fence_token() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    insert_expired_maker(&store, "offer-a", 0xaa);
    assert!(store
        .try_claim_expired_maker("offer-a", "m1", "token-a")
        .expect("claim"));
    // Force age past stale window, then recover; late finalize with old token must lose.
    store
        .conn
        .execute(
            "UPDATE offer_state SET updated_at = '2020-01-01T00:00:00Z' WHERE offer_id = ?1",
            rusqlite::params!["offer-a"],
        )
        .expect("age claim");
    let now = 1_700_000_000i64 + MAKER_CLAIM_STALE_SECONDS + 1;
    assert_eq!(store.restore_stale_maker_claims(now).expect("stale"), 1);
    assert!(!store
        .finalize_maker_claim("offer-a", "m1", "token-a")
        .expect("late finalize"));
    assert!(store
        .try_claim_expired_maker("offer-a", "m1", "token-b")
        .expect("reclaim"));
    assert!(!store
        .finalize_maker_claim("offer-a", "m1", "token-a")
        .expect("wrong token"));
    assert!(store
        .finalize_maker_claim("offer-a", "m1", "token-b")
        .expect("owner finalize"));
}

#[test]
fn renew_maker_claim_requires_matching_fence_token() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    insert_expired_maker(&store, "offer-a", 0xaa);
    assert!(store
        .try_claim_expired_maker("offer-a", "m1", "token-a")
        .expect("claim"));
    assert!(store
        .renew_maker_claim("offer-a", "m1", "token-a")
        .expect("renew owner"));
    assert!(!store
        .renew_maker_claim("offer-a", "m1", "token-b")
        .expect("renew wrong"));
    store
        .conn
        .execute(
            "UPDATE offer_state SET updated_at = '2020-01-01T00:00:00Z' WHERE offer_id = ?1",
            rusqlite::params!["offer-a"],
        )
        .expect("age claim");
    let now = 1_700_000_000i64 + MAKER_CLAIM_STALE_SECONDS + 1;
    assert_eq!(store.restore_stale_maker_claims(now).expect("stale"), 1);
    assert!(!store
        .renew_maker_claim("offer-a", "m1", "token-a")
        .expect("renew after stale"));
}

#[test]
fn null_listing_expires_at_counts_as_past_soft_expiry() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let fields =
        OfferCancelFields::from_presplit_build("aa".repeat(32), "bb".repeat(32), "cc".repeat(32));
    store
        .upsert_offer_state_with_metadata_at(
            "offer-legacy",
            "m1",
            "open",
            None,
            "2026-01-01T00:00:00Z",
            OfferCancelWrite {
                fields: Some(&fields),
                execution_mode: Some(OfferExecutionMode::PresplitExisting),
                listing: OfferListingWrite {
                    publish_venue: Some("dexie"),
                    listing_expires_at: None,
                    size_base_units: Some(10),
                    offer_nonce: Some(&"dd".repeat(32)),
                    offer_side: Some("sell"),
                },
                ..OfferCancelWrite::default()
            },
        )
        .expect("upsert");
    let past = store
        .list_open_offers_past_listing_expiry("m1", 1_700_000_001)
        .expect("past");
    assert_eq!(past.len(), 1);
    assert_eq!(past[0].offer_id, "offer-legacy");
    assert!(past[0].listing_expires_at.is_none());
}

#[test]
fn expired_maker_queries_paginate_past_page_size() {
    let dir = tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
    let total = usize::try_from(REUSABLE_PAGE_SIZE).expect("page") + 3;
    for idx in 0..total {
        insert_expired_maker(
            &store,
            &format!("offer-{idx:04}"),
            u8::try_from(idx % 250).unwrap_or(0),
        );
    }
    let expired = store.list_expired_presplit_makers("m1").expect("list");
    assert_eq!(expired.len(), total);
}
