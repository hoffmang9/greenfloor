//! Soft-listing expiry and maker-claim transitions for durable maker reuse.

use tracing::info;

use crate::error::SignerResult;
use crate::storage::{CycleWriteStore, SqliteStore};

/// Run a maker-claim CAS transition unless `dry_run` (dry runs report success without
/// touching the store). Shared by [`try_claim_expired_maker_synced`],
/// [`restore_maker_claim_synced`], [`renew_maker_claim_synced`], and
/// [`finalize_maker_claim_synced`], which differ only in which [`SqliteStore`] CAS method
/// they call.
fn maker_claim_cas(
    write_store: &CycleWriteStore,
    market_id: &str,
    offer_id: &str,
    claim_token: &str,
    dry_run: bool,
    op: fn(&SqliteStore, &str, &str, &str) -> SignerResult<bool>,
) -> SignerResult<bool> {
    if dry_run {
        return Ok(true);
    }
    write_store.sync(|store| op(store, offer_id, market_id, claim_token))
}

/// Mark durable (presplit) listings past soft `listing_expires_at` as expired (Dexie status 6).
///
/// CAS per row: only transitions when still in `open` / `refresh_due` / `mempool_observed`,
/// so a concurrent take/cancel cannot be clobbered back to `expired`. Only rows with
/// reclaim metadata are marked (same filter as expired-maker reclaim/reuse). Legacy rows
/// with NULL `listing_expires_at` are included (treated as already elapsed).
/// No-ops when `dry_run` (same contract as claim helpers).
///
/// # Errors
///
/// Returns an error when listing or CAS update fails.
pub fn mark_listings_soft_expired(
    store: &SqliteStore,
    market_id: &str,
    now_unix: i64,
    dry_run: bool,
) -> SignerResult<usize> {
    if dry_run {
        return Ok(0);
    }
    let rows = store.list_open_offers_past_listing_expiry(market_id, now_unix)?;
    let mut marked = 0usize;
    for row in rows {
        if !store.try_mark_listing_soft_expired(&row.offer_id, market_id)? {
            continue;
        }
        marked = marked.saturating_add(1);
        info!(
            offer_id = %row.offer_id,
            market_id,
            "soft listing expiry → expired"
        );
    }
    Ok(marked)
}

/// CAS-claim an expired maker (`expired` → `maker_claimed`) with a fencing token.
///
/// No-ops win when `dry_run`.
///
/// # Errors
///
/// Returns an error when the store update fails.
pub(crate) fn try_claim_expired_maker_synced(
    write_store: &CycleWriteStore,
    market_id: &str,
    offer_id: &str,
    claim_token: &str,
    dry_run: bool,
) -> SignerResult<bool> {
    maker_claim_cas(
        write_store,
        market_id,
        offer_id,
        claim_token,
        dry_run,
        SqliteStore::try_claim_expired_maker,
    )
}

/// Undo [`try_claim_expired_maker_synced`] when I/O left the maker coin reusable.
///
/// Returns `true` when this token still owned the claim.
///
/// # Errors
///
/// Returns an error when the store update fails.
pub(crate) fn restore_maker_claim_synced(
    write_store: &CycleWriteStore,
    market_id: &str,
    offer_id: &str,
    claim_token: &str,
    dry_run: bool,
) -> SignerResult<bool> {
    maker_claim_cas(
        write_store,
        market_id,
        offer_id,
        claim_token,
        dry_run,
        SqliteStore::restore_maker_claim,
    )
}

/// Extend an in-flight claim lease (`updated_at`) while PreferExisting/reclaim I/O runs.
///
/// Returns `true` when this token still owned the claim.
///
/// # Errors
///
/// Returns an error when the store update fails.
pub(crate) fn renew_maker_claim_synced(
    write_store: &CycleWriteStore,
    market_id: &str,
    offer_id: &str,
    claim_token: &str,
    dry_run: bool,
) -> SignerResult<bool> {
    maker_claim_cas(
        write_store,
        market_id,
        offer_id,
        claim_token,
        dry_run,
        SqliteStore::renew_maker_claim,
    )
}

/// Commit a successful claim (`maker_claimed` → `cancelled`).
///
/// Returns `true` when this token still owned the claim.
///
/// # Errors
///
/// Returns an error when the store update fails.
pub(crate) fn finalize_maker_claim_synced(
    write_store: &CycleWriteStore,
    market_id: &str,
    offer_id: &str,
    claim_token: &str,
    dry_run: bool,
) -> SignerResult<bool> {
    maker_claim_cas(
        write_store,
        market_id,
        offer_id,
        claim_token,
        dry_run,
        SqliteStore::finalize_maker_claim,
    )
}

/// Restore stale `maker_claimed` rows left by crashed workers.
///
/// # Errors
///
/// Returns an error when the store update fails.
pub fn restore_stale_maker_claims_synced(
    write_store: &CycleWriteStore,
    now_unix: i64,
    dry_run: bool,
) -> SignerResult<u64> {
    if dry_run {
        return Ok(0);
    }
    write_store.sync(|store| store.restore_stale_maker_claims(now_unix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
    use crate::storage::{CycleWriteStore, OfferCancelWrite, OfferListingWrite};
    use tempfile::tempdir;

    #[test]
    fn mark_soft_expired_and_claim_finalize_roundtrip() {
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

        assert_eq!(
            mark_listings_soft_expired(&store, "m1", 1_700_000_001, true).expect("dry"),
            0
        );
        let marked = mark_listings_soft_expired(&store, "m1", 1_700_000_001, false).expect("mark");
        assert_eq!(marked, 1);
        let expired = store.list_expired_presplit_makers("m1").expect("expired");
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].state, "expired");

        assert!(store
            .try_claim_expired_maker("offer-soft", "m1", "token-a")
            .expect("claim"));
        assert!(store
            .finalize_maker_claim("offer-soft", "m1", "token-a")
            .expect("finalize"));
        let after = store
            .list_expired_presplit_makers("m1")
            .expect("expired after finalize");
        assert!(after.is_empty());
        // CAS mark must not clobber a finalized/cancelled row back to expired.
        assert!(!store
            .try_mark_listing_soft_expired("offer-soft", "m1")
            .expect("mark cancelled"));
    }

    #[test]
    fn claim_synced_skips_mutation_on_dry_run() {
        let dir = tempdir().expect("tempdir");
        let db = dir.path().join("state.db");
        let write_store = CycleWriteStore::open(&db).expect("open");
        {
            let store = write_store.lock().expect("lock");
            let fields = OfferCancelFields::from_presplit_build(
                "aa".repeat(32),
                "bb".repeat(32),
                "cc".repeat(32),
            );
            store
                .upsert_offer_state_with_metadata_at(
                    "offer-soft",
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
                            offer_side: Some("sell"),
                        },
                        ..OfferCancelWrite::default()
                    },
                )
                .expect("upsert");
        }
        assert!(try_claim_expired_maker_synced(
            &write_store,
            "m1",
            "offer-soft",
            "token-dry",
            true
        )
        .expect("dry"));
        let still = write_store
            .sync(|store| store.list_expired_presplit_makers("m1"))
            .expect("list");
        assert_eq!(still.len(), 1);
        assert!(try_claim_expired_maker_synced(
            &write_store,
            "m1",
            "offer-soft",
            "token-live",
            false
        )
        .expect("live"));
        assert!(
            finalize_maker_claim_synced(&write_store, "m1", "offer-soft", "token-live", false)
                .expect("finalize")
        );
        let gone = write_store
            .sync(|store| store.list_expired_presplit_makers("m1"))
            .expect("list");
        assert!(gone.is_empty());
    }
}
