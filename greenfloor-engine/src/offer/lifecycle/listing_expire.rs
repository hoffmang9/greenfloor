//! Soft-listing expiry and supersede transitions for durable maker reuse.

use tracing::info;

use crate::cycle::OfferLifecycleState;
use crate::error::SignerResult;
use crate::offer::dexie_payload::{DEXIE_STATUS_CANCELLED, DEXIE_STATUS_EXPIRED};
use crate::storage::{CycleWriteStore, SqliteStore};

/// Mark open listings past soft `listing_expires_at` as expired (Dexie status 6).
///
/// # Errors
///
/// Returns an error when listing or state upsert fails.
pub fn mark_listings_soft_expired(
    store: &SqliteStore,
    market_id: &str,
    now_unix: i64,
) -> SignerResult<usize> {
    let rows = store.list_open_offers_past_listing_expiry(market_id, now_unix)?;
    let mut marked = 0usize;
    for row in rows {
        store.upsert_offer_state(
            &row.offer_id,
            market_id,
            OfferLifecycleState::Expired.as_str(),
            Some(DEXIE_STATUS_EXPIRED),
        )?;
        marked = marked.saturating_add(1);
        info!(
            offer_id = %row.offer_id,
            market_id,
            "soft listing expiry → expired"
        );
    }
    Ok(marked)
}

/// Retire a listing superseded by reclaim or `PreferExisting` re-post (cancelled + Dexie 3).
///
/// # Errors
///
/// Returns an error when the state upsert fails.
pub fn supersede_listing_for_repost(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
) -> SignerResult<()> {
    store.upsert_offer_state(
        offer_id,
        market_id,
        "cancelled",
        Some(DEXIE_STATUS_CANCELLED),
    )
}

/// [`supersede_listing_for_repost`] via [`CycleWriteStore`]; no-ops when `dry_run`.
///
/// # Errors
///
/// Returns an error when the state upsert fails.
pub fn supersede_listing_for_repost_synced(
    write_store: &CycleWriteStore,
    market_id: &str,
    offer_id: &str,
    dry_run: bool,
) -> SignerResult<()> {
    if dry_run {
        return Ok(());
    }
    write_store.sync(|store| supersede_listing_for_repost(store, market_id, offer_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
    use crate::storage::{OfferCancelWrite, OfferListingWrite};
    use tempfile::tempdir;

    #[test]
    fn mark_soft_expired_and_supersede_roundtrip() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let fields = OfferCancelFields::from_presplit_build(
            "aa".repeat(32),
            "bb".repeat(32),
            "cc".repeat(32),
        );
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
                        offer_nonce: Some(&"dd".repeat(32)),
                        offer_side: Some("sell"),
                    },
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert");

        let marked = mark_listings_soft_expired(&store, "m1", 1_700_000_001).expect("mark");
        assert_eq!(marked, 1);
        let expired = store.list_expired_presplit_makers("m1").expect("expired");
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].state, "expired");

        supersede_listing_for_repost(&store, "m1", "offer-soft").expect("supersede");
        let after = store
            .list_expired_presplit_makers("m1")
            .expect("expired after supersede");
        assert!(after.is_empty());
    }
}
