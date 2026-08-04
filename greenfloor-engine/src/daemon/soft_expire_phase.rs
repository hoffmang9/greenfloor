//! Soft-expire open listings and reclaim surplus durable maker coins after expiry.
//!
//! Runs only for soft-expiry markets (`quote_asset_type: stable`). Does not post:
//! when the ladder has a gap, all wanted-size expired makers stay for strategy
//! `ensure_size_n_offer`; when the gap is zero they are reclaimed. Unstable hard-expiry
//! cleanup stays on the reconcile path.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{info, warn};

use crate::config::market_uses_soft_listing_expiry;
use crate::config::MarketConfig;
use crate::daemon::market_context::MarketCycleContext;
use crate::daemon::strategy_support::active_capacity_counts_for_market;
use crate::error::SignerResult;
use crate::offer::lifecycle::{
    mark_listings_soft_expired, plan_soft_expire_reclaims, reclaim_expired_maker_if_unspent,
    restore_stale_maker_claims_synced, ReclaimMakerOutcome,
};
use crate::offer::request::effective_offer_side;
use crate::storage::{CycleWriteStore, ReusablePresplitMakerRow};

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Test I/O injection for soft-expire reclaim (no process-global hooks).
#[derive(Debug, Default)]
pub(crate) struct SoftExpireIoOverrides {
    /// Injected reclaim outcomes (`Err` soft-fails like live reclaim errors).
    pub reclaim_outcomes: VecDeque<Result<ReclaimMakerOutcome, String>>,
    /// Offer IDs passed to reclaim (including injected soft-failures).
    pub attempted_reclaim_ids: Vec<String>,
}

async fn reclaim_one(
    write_store: &CycleWriteStore,
    ctx: &MarketCycleContext<'_>,
    row: &ReusablePresplitMakerRow,
    dry_run: bool,
    io: &mut SoftExpireIoOverrides,
) -> SignerResult<ReclaimMakerOutcome> {
    io.attempted_reclaim_ids.push(row.offer_id.clone());
    if let Some(injected) = io.reclaim_outcomes.pop_front() {
        return injected.map_err(crate::error::SignerError::Other);
    }
    let signer = ctx.resources.signer_for_execution()?.clone();
    reclaim_expired_maker_if_unspent(write_store, signer, &ctx.resources.network, row, dry_run)
        .await
}

/// Soft-fail reclaim so one Coinset/spend error does not abort the market cycle.
async fn reclaim_one_soft(
    write_store: &CycleWriteStore,
    ctx: &MarketCycleContext<'_>,
    row: &ReusablePresplitMakerRow,
    dry_run: bool,
    io: &mut SoftExpireIoOverrides,
) {
    match reclaim_one(write_store, ctx, row, dry_run, io).await {
        Ok(outcome) => {
            info!(
                offer_id = %row.offer_id,
                outcome = ?outcome,
                "expired maker reclaim finished"
            );
        }
        Err(err) => {
            warn!(
                offer_id = %row.offer_id,
                error = %err,
                "expired maker reclaim failed; continuing soft-expire"
            );
        }
    }
}

/// Active open counts for soft-expire capacity — same watchlist helper as strategy.
fn active_open_by_side_size(
    write_store: &CycleWriteStore,
    market: &MarketConfig,
    expired: &[ReusablePresplitMakerRow],
    dexie_size_by_offer_id: &HashMap<String, i64>,
) -> SignerResult<HashMap<(String, i64), i64>> {
    let mut keys = HashSet::<(String, i64)>::new();
    for row in expired {
        let side = effective_offer_side(row.offer_side.as_deref()).to_string();
        let Some(ladder_size) = row.size_base_units.filter(|units| *units > 0) else {
            continue;
        };
        keys.insert((side, ladder_size));
    }
    if keys.is_empty() {
        return Ok(HashMap::new());
    }

    let tracked_sizes: Vec<i64> = keys.iter().map(|(_, size)| *size).collect();
    write_store.sync(|store| {
        let capacity = active_capacity_counts_for_market(
            store,
            market,
            Some(dexie_size_by_offer_id),
            &tracked_sizes,
        )?;
        let mut counts = HashMap::new();
        for (side, size) in keys {
            counts.insert((side.clone(), size), capacity.for_side_size(&side, size));
        }
        Ok(counts)
    })
}

/// Soft-expire listings; leave wanted-size makers when gap > 0; reclaim when gap is 0.
///
/// # Errors
///
/// Returns an error when store paths fail hard.
pub async fn run_soft_expire_phase(
    write_store: &CycleWriteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
) -> SignerResult<()> {
    let mut io = SoftExpireIoOverrides::default();
    run_soft_expire_phase_inner(write_store, ctx, market, &mut io).await
}

pub(crate) async fn run_soft_expire_phase_inner(
    write_store: &CycleWriteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
    io: &mut SoftExpireIoOverrides,
) -> SignerResult<()> {
    if !market_uses_soft_listing_expiry(&market.quote_asset_type) {
        // Hard on-chain expiry: leave cleanup to reconcile; do not reclaim here.
        return Ok(());
    }

    let now = now_unix();
    let dry_run = ctx.resources.program().runtime_dry_run;
    let _ = restore_stale_maker_claims_synced(write_store, now, dry_run)?;
    let market_id = market.market_id.clone();
    let expired = write_store.sync(|store| {
        let _ = mark_listings_soft_expired(store, &market_id, now, dry_run)?;
        store.list_expired_presplit_makers(&market_id)
    })?;
    if expired.is_empty() {
        return Ok(());
    }

    let active_open = active_open_by_side_size(
        write_store,
        market,
        &expired,
        &ctx.reconcile.dexie_size_by_offer_id,
    )?;
    let reclaim = plan_soft_expire_reclaims(expired, market, &active_open);
    // Match cancel_phase: dry-run plans reclaim targets but skips Coinset/vault/spend I/O.
    if dry_run {
        for row in &reclaim {
            info!(offer_id = %row.offer_id, "dry_run: would reclaim expired maker");
            io.attempted_reclaim_ids.push(row.offer_id.clone());
        }
        return Ok(());
    }
    for row in &reclaim {
        if row.size_base_units.is_none_or(|units| units <= 0) {
            warn!(offer_id = %row.offer_id, "expired maker missing size_base_units; reclaim");
        }
        reclaim_one_soft(write_store, ctx, row, false, io).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::config::{LadderEntry, ManagerProgramConfig};
    use crate::daemon::test_support::test_cycle_context;
    use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
    use crate::storage::{CycleWriteStore, OfferCancelWrite, OfferListingWrite, SqliteStore};
    use crate::test_support::market_config::sample_market;
    use crate::test_support::signer_config::test_signer_config;

    fn stable_market_with_target(size: i64, target_count: i64) -> MarketConfig {
        let mut market = sample_market("xch1test");
        market.quote_asset_type = "stable".into();
        let mut ladders = HashMap::new();
        ladders.insert(
            "sell".to_string(),
            vec![LadderEntry {
                size_base_units: size,
                target_count,
                split_buffer_count: 0,
                combine_when_excess_factor: 0.0,
            }],
        );
        market.ladders = ladders;
        market
    }

    fn seed_maker(
        store: &SqliteStore,
        offer_id: &str,
        state: &str,
        size_base_units: Option<i64>,
        offer_side: Option<&str>,
        coin_byte: u8,
        listing_expires_at: Option<i64>,
    ) {
        let coin = hex::encode([coin_byte; 32]);
        let fields = OfferCancelFields::from_presplit_build(coin, "bb".repeat(32), "cc".repeat(32));
        let dexie_status = if state == "expired" { Some(6) } else { None };
        store
            .upsert_offer_state_with_metadata_at(
                offer_id,
                "m1",
                state,
                dexie_status,
                "2026-01-01T00:00:00Z",
                OfferCancelWrite {
                    fields: Some(&fields),
                    execution_mode: Some(OfferExecutionMode::PresplitExisting),
                    listing: OfferListingWrite {
                        publish_venue: Some("dexie"),
                        listing_expires_at,
                        size_base_units,
                        offer_nonce: Some(&"dd".repeat(32)),
                        offer_side,
                    },
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert");
    }

    fn open_phase_store(dir: &tempfile::TempDir) -> (CycleWriteStore, std::path::PathBuf) {
        let db_path = dir.path().join("state.db");
        let write_store = CycleWriteStore::open(&db_path).expect("open");
        (write_store, db_path)
    }

    fn phase_bundle(
        dir: &tempfile::TempDir,
        db_path: &Path,
        write_store: CycleWriteStore,
        runtime_dry_run: bool,
    ) -> crate::daemon::test_support::TestCycleContextBundle {
        let program = ManagerProgramConfig {
            runtime_dry_run,
            signer_kms_key_id: "kms-test".to_string(),
            vault_launcher_id: "11".repeat(32),
            ..Default::default()
        };
        test_cycle_context(
            dir,
            db_path,
            write_store,
            program,
            Some(test_signer_config("https://api.coinset.org")),
        )
    }

    #[tokio::test]
    async fn run_soft_expire_skips_unstable_markets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (write_store, db_path) = open_phase_store(&dir);
        let bundle = phase_bundle(&dir, Path::new(&db_path), write_store.clone(), true);
        let market = sample_market("xch1test");
        run_soft_expire_phase(&write_store, &bundle.cycle_context(), &market)
            .await
            .expect("skip unstable");
    }

    #[tokio::test]
    async fn run_soft_expire_dry_run_does_not_mark_or_reclaim() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (write_store, db_path) = open_phase_store(&dir);
        {
            let store = write_store.lock().expect("lock");
            seed_maker(
                &store,
                "offer-open",
                "open",
                Some(10),
                Some("sell"),
                0x11,
                Some(1_700_000_000),
            );
            // Already expired surplus — dry-run plans reclaim without I/O.
            seed_maker(
                &store,
                "offer-expired",
                "expired",
                Some(99),
                Some("sell"),
                0xaa,
                Some(1_700_000_000),
            );
        }
        let bundle = phase_bundle(&dir, Path::new(&db_path), write_store.clone(), true);
        let market = stable_market_with_target(10, 1);
        let mut io = SoftExpireIoOverrides {
            reclaim_outcomes: VecDeque::from([Ok(ReclaimMakerOutcome::Reclaimed {
                superseded_offer_id: "offer-expired".into(),
            })]),
            attempted_reclaim_ids: Vec::new(),
        };
        run_soft_expire_phase_inner(&write_store, &bundle.cycle_context(), &market, &mut io)
            .await
            .expect("dry-run soft expire");
        let state = write_store
            .sync(|store| store.offer_state_for_id("offer-open"))
            .expect("lookup")
            .expect("row");
        assert_eq!(state, "open");
        assert_eq!(io.attempted_reclaim_ids, vec!["offer-expired".to_string()]);
        // Injected outcome unused — dry-run must not call reclaim I/O.
        assert_eq!(io.reclaim_outcomes.len(), 1);
    }

    #[tokio::test]
    async fn run_soft_expire_continues_after_reclaim_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (write_store, db_path) = open_phase_store(&dir);
        {
            let store = write_store.lock().expect("lock");
            seed_maker(
                &store,
                "offer-a",
                "expired",
                Some(10),
                Some("sell"),
                0xaa,
                Some(1_700_000_000),
            );
            seed_maker(
                &store,
                "offer-b",
                "expired",
                Some(99),
                Some("sell"),
                0xbb,
                Some(1_700_000_000),
            );
        }
        let bundle = phase_bundle(&dir, Path::new(&db_path), write_store.clone(), false);
        let market = stable_market_with_target(10, 1);
        let mut io = SoftExpireIoOverrides {
            // keep offer-a (gap>0); unwanted offer-b reclaim fails soft
            reclaim_outcomes: VecDeque::from([Err("coinset unavailable".to_string())]),
            attempted_reclaim_ids: Vec::new(),
        };
        run_soft_expire_phase_inner(&write_store, &bundle.cycle_context(), &market, &mut io)
            .await
            .expect("soft expire continues after reclaim failure");
        assert_eq!(io.attempted_reclaim_ids, vec!["offer-b".to_string()]);
        assert!(io.reclaim_outcomes.is_empty());
    }

    #[tokio::test]
    async fn run_soft_expire_counts_active_via_dexie_watchlist_fallback() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (write_store, db_path) = open_phase_store(&dir);
        {
            let store = write_store.lock().expect("lock");
            // Open without listing size — Dexie hint must still fill capacity via watchlist.
            seed_maker(
                &store,
                "offer-open",
                "open",
                None,
                None,
                0x11,
                Some(4_000_000_000),
            );
            seed_maker(
                &store,
                "offer-a",
                "expired",
                Some(10),
                Some("sell"),
                0xaa,
                Some(1_700_000_000),
            );
            seed_maker(
                &store,
                "offer-b",
                "expired",
                Some(10),
                Some("sell"),
                0xbb,
                Some(1_700_000_000),
            );
        }
        let mut bundle = phase_bundle(&dir, Path::new(&db_path), write_store.clone(), false);
        bundle
            .reconcile
            .dexie_size_by_offer_id
            .insert("offer-open".into(), 10);
        let market = stable_market_with_target(10, 1);
        let mut io = SoftExpireIoOverrides {
            reclaim_outcomes: VecDeque::from([
                Ok(ReclaimMakerOutcome::Reclaimed {
                    superseded_offer_id: "offer-a".into(),
                }),
                Ok(ReclaimMakerOutcome::Reclaimed {
                    superseded_offer_id: "offer-b".into(),
                }),
            ]),
            attempted_reclaim_ids: Vec::new(),
        };
        run_soft_expire_phase_inner(&write_store, &bundle.cycle_context(), &market, &mut io)
            .await
            .expect("soft expire");
        assert_eq!(
            io.attempted_reclaim_ids,
            vec!["offer-a".to_string(), "offer-b".to_string()]
        );
        assert!(io.reclaim_outcomes.is_empty());
    }
}
