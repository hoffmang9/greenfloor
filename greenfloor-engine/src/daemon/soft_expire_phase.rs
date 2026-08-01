//! Soft-expire open listings and ensure/reclaim durable maker coins after expiry.
//!
//! Runs only for soft-expiry markets (`quote_asset_type: stable`). Unstable hard-expiry
//! cleanup stays on the reconcile path.

use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{info, warn};

use crate::config::{market_uses_soft_listing_expiry, market_wants_ladder_size, MarketConfig};
use crate::daemon::market_context::MarketCycleContext;
use crate::error::SignerResult;
use crate::offer::lifecycle::mark_listings_soft_expired;
use crate::offer::operator::{
    ensure_size_n_offer, reclaim_expired_maker_if_unspent, BuildAndPostOfferRequestParts,
    EnsureSizeResult, ReclaimMakerOutcome,
};
use crate::offer::request::normalize_offer_side;
use crate::storage::{CycleWriteStore, ReusablePresplitMakerRow};

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SoftExpireSizeGroup {
    pub side: String,
    pub size_base_units: i64,
    pub makers: Vec<ReusablePresplitMakerRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SoftExpirePlanItem {
    EnsureGroup(SoftExpireSizeGroup),
    Reclaim(ReusablePresplitMakerRow),
}

/// Local I/O overrides for soft-expire tests (no process-global hooks).
#[derive(Debug, Default)]
pub(crate) struct SoftExpireIoOverrides {
    pub ensure_results: VecDeque<EnsureSizeResult>,
    pub reclaim_results: VecDeque<ReclaimMakerOutcome>,
}

/// Group expired makers by `(side, size)`: ensure once per wanted key, reclaim the rest.
#[must_use]
pub(crate) fn plan_soft_expire_work(
    expired: Vec<ReusablePresplitMakerRow>,
    market: &MarketConfig,
) -> Vec<SoftExpirePlanItem> {
    let mut order: Vec<(String, i64)> = Vec::new();
    let mut groups: HashMap<(String, i64), Vec<ReusablePresplitMakerRow>> = HashMap::new();
    let mut missing_size = Vec::new();

    for row in expired {
        let side = normalize_offer_side(row.offer_side.as_deref().unwrap_or("sell")).to_string();
        let Some(ladder_size) = row.size_base_units.filter(|units| *units > 0) else {
            missing_size.push(row);
            continue;
        };
        let key = (side, ladder_size);
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(row);
    }

    let mut plan = Vec::with_capacity(missing_size.len().saturating_add(order.len()));
    for row in missing_size {
        plan.push(SoftExpirePlanItem::Reclaim(row));
    }
    for key in order {
        let makers = groups.remove(&key).unwrap_or_default();
        if market_wants_ladder_size(market, &key.0, key.1) {
            plan.push(SoftExpirePlanItem::EnsureGroup(SoftExpireSizeGroup {
                side: key.0,
                size_base_units: key.1,
                makers,
            }));
        } else {
            for row in makers {
                plan.push(SoftExpirePlanItem::Reclaim(row));
            }
        }
    }
    plan
}

async fn reclaim_one(
    write_store: &CycleWriteStore,
    ctx: &MarketCycleContext<'_>,
    row: &ReusablePresplitMakerRow,
    dry_run: bool,
    io: &mut SoftExpireIoOverrides,
) -> SignerResult<ReclaimMakerOutcome> {
    if let Some(injected) = io.reclaim_results.pop_front() {
        return Ok(injected);
    }
    let signer = ctx.resources.signer_for_execution()?.clone();
    reclaim_expired_maker_if_unspent(write_store, signer, &ctx.resources.network, row, dry_run)
        .await
}

async fn reclaim_surplus_makers(
    write_store: &CycleWriteStore,
    ctx: &MarketCycleContext<'_>,
    makers: &[ReusablePresplitMakerRow],
    ensure_result: &EnsureSizeResult,
    dry_run: bool,
    io: &mut SoftExpireIoOverrides,
) -> SignerResult<()> {
    for row in makers {
        if ensure_result.protects_maker(row) {
            continue;
        }
        let outcome = reclaim_one(write_store, ctx, row, dry_run, io).await?;
        info!(
            offer_id = %row.offer_id,
            outcome = ?outcome,
            "expired surplus maker reclaimed"
        );
    }
    Ok(())
}

/// Soft-expire listings then ensure size-N offer or reclaim expired makers (stable markets only).
///
/// Wanted sizes are ensured once per `(side, size)` group; surplus makers in that group
/// are reclaimed except the maker retained/superseded by ensure.
///
/// # Errors
///
/// Returns an error when store or ensure/reclaim paths fail hard.
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
        // Hard on-chain expiry: leave cleanup to reconcile; do not ensure/reclaim here.
        return Ok(());
    }

    let now = now_unix();
    let market_id = market.market_id.clone();
    let expired = write_store.sync(|store| {
        let _ = mark_listings_soft_expired(store, &market_id, now)?;
        store.list_expired_presplit_makers(&market_id)
    })?;
    if expired.is_empty() {
        return Ok(());
    }

    let signer = ctx.resources.signer_for_execution()?.clone();
    let program = ctx.resources.program();
    let dry_run = program.runtime_dry_run;
    let plan = plan_soft_expire_work(expired, market);

    for item in plan {
        match item {
            SoftExpirePlanItem::Reclaim(row) => {
                if row.size_base_units.is_none_or(|units| units <= 0) {
                    warn!(offer_id = %row.offer_id, "expired maker missing size_base_units; reclaim");
                }
                let outcome = reclaim_one(write_store, ctx, &row, dry_run, io).await?;
                info!(
                    offer_id = %row.offer_id,
                    outcome = ?outcome,
                    "expired maker reclaimed"
                );
            }
            SoftExpirePlanItem::EnsureGroup(group) => {
                let result = if let Some(injected) = io.ensure_results.pop_front() {
                    injected
                } else {
                    let size_u64 = crate::config::parse_non_negative_u64(
                        group.size_base_units,
                        "size_base_units",
                    )?;
                    let parts = BuildAndPostOfferRequestParts::for_ensure_size(
                        &ctx.resources.paths.as_operator_paths(),
                        program,
                        ctx.resources.network.clone(),
                        market.market_id.clone(),
                        size_u64,
                        &group.side,
                    );
                    ensure_size_n_offer(
                        write_store,
                        signer.clone(),
                        &ctx.resources.ticker_index,
                        market,
                        parts,
                        Some(group.makers.clone()),
                    )
                    .await?
                };
                info!(
                    market_id = %market.market_id,
                    side = %group.side,
                    ladder_size = group.size_base_units,
                    outcome = ?result,
                    "ensure_size_n_offer after expire"
                );
                reclaim_surplus_makers(write_store, ctx, &group.makers, &result, dry_run, io)
                    .await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use super::*;
    use crate::config::{market_uses_soft_listing_expiry, LadderEntry, ManagerProgramConfig};
    use crate::daemon::test_support::test_cycle_context;
    use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
    use crate::storage::{CycleWriteStore, OfferCancelWrite, OfferListingWrite, SqliteStore};
    use crate::test_support::market_config::sample_market;
    use crate::test_support::signer_config::test_signer_config;

    fn maker(
        offer_id: &str,
        size_base_units: Option<i64>,
        offer_side: &str,
    ) -> ReusablePresplitMakerRow {
        ReusablePresplitMakerRow {
            offer_id: offer_id.into(),
            market_id: "m1".into(),
            state: "expired".into(),
            size_base_units,
            offer_side: Some(offer_side.into()),
            cancel_input_coin_id: format!("coin-{offer_id}"),
            fixed_delegated_puzzle_hash: "hh".into(),
            offer_nonce: Some("nn".into()),
            listing_expires_at: None,
        }
    }

    fn stable_market_with_size(size: i64) -> MarketConfig {
        let mut market = sample_market("xch1test");
        market.quote_asset_type = "stable".into();
        let mut ladders = HashMap::new();
        ladders.insert(
            "sell".to_string(),
            vec![LadderEntry {
                size_base_units: size,
                target_count: 1,
                split_buffer_count: 0,
                combine_when_excess_factor: 0.0,
            }],
        );
        market.ladders = ladders;
        market
    }

    fn seed_expired_maker(store: &SqliteStore, offer_id: &str, size: i64, coin_byte: u8) {
        let coin = hex::encode([coin_byte; 32]);
        let fields = OfferCancelFields::from_presplit_build(coin, "bb".repeat(32), "cc".repeat(32));
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
                        size_base_units: Some(size),
                        offer_nonce: Some(&"dd".repeat(32)),
                        offer_side: Some("sell"),
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

    #[test]
    fn plan_groups_wanted_size_and_reclaims_unwanted() {
        let market = stable_market_with_size(10);
        let plan = plan_soft_expire_work(
            vec![
                maker("a", Some(10), "sell"),
                maker("b", Some(10), "sell"),
                maker("c", Some(99), "sell"),
                maker("d", None, "sell"),
            ],
            &market,
        );

        assert!(matches!(
            &plan[0],
            SoftExpirePlanItem::Reclaim(row) if row.offer_id == "d"
        ));
        match &plan[1] {
            SoftExpirePlanItem::EnsureGroup(group) => {
                assert_eq!(group.size_base_units, 10);
                assert_eq!(group.makers.len(), 2);
                assert_eq!(group.makers[0].offer_id, "a");
                assert_eq!(group.makers[1].offer_id, "b");
            }
            SoftExpirePlanItem::Reclaim(row) => {
                panic!("expected ensure group, got reclaim {}", row.offer_id)
            }
        }
        assert!(matches!(
            &plan[2],
            SoftExpirePlanItem::Reclaim(row) if row.offer_id == "c"
        ));
    }

    #[test]
    fn hard_expiry_markets_skip_soft_expire_phase_gate() {
        let market = sample_market("xch1test");
        assert!(!market_uses_soft_listing_expiry(&market.quote_asset_type));
    }

    #[tokio::test]
    async fn run_soft_expire_skips_unstable_markets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (write_store, db_path) = open_phase_store(&dir);
        let program = ManagerProgramConfig {
            runtime_dry_run: true,
            ..Default::default()
        };
        let bundle = test_cycle_context(
            &dir,
            Path::new(&db_path),
            write_store.clone(),
            program,
            Some(test_signer_config("https://api.coinset.org")),
        );
        let market = sample_market("xch1test");
        run_soft_expire_phase(&write_store, &bundle.cycle_context(), &market)
            .await
            .expect("skip unstable");
    }

    #[tokio::test]
    async fn run_soft_expire_returns_when_no_expired_makers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (write_store, db_path) = open_phase_store(&dir);
        let program = ManagerProgramConfig {
            runtime_dry_run: true,
            ..Default::default()
        };
        let bundle = test_cycle_context(
            &dir,
            Path::new(&db_path),
            write_store.clone(),
            program,
            Some(test_signer_config("https://api.coinset.org")),
        );
        let market = stable_market_with_size(10);
        run_soft_expire_phase(&write_store, &bundle.cycle_context(), &market)
            .await
            .expect("empty expired");
    }

    #[tokio::test]
    async fn run_soft_expire_ensure_group_protects_selected_and_reclaims_surplus() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (write_store, db_path) = open_phase_store(&dir);
        {
            let store = write_store.lock().expect("lock");
            seed_expired_maker(&store, "offer-a", 10, 0xaa);
            seed_expired_maker(&store, "offer-b", 10, 0xbb);
            seed_expired_maker(&store, "offer-c", 99, 0xcc);
        }
        let program = ManagerProgramConfig {
            runtime_dry_run: true,
            signer_kms_key_id: "kms-test".to_string(),
            vault_launcher_id: "11".repeat(32),
            ..Default::default()
        };
        let bundle = test_cycle_context(
            &dir,
            Path::new(&db_path),
            write_store.clone(),
            program,
            Some(test_signer_config("https://api.coinset.org")),
        );
        let market = stable_market_with_size(10);
        let retained = hex::encode([0xaa; 32]);
        let ensure_result = EnsureSizeResult::PostedExisting {
            superseded_offer_id: "offer-a".into(),
            retained_maker_coin_id: retained.clone(),
        };
        assert!(ensure_result.protects_maker(&ReusablePresplitMakerRow {
            offer_id: "offer-a".into(),
            market_id: "m1".into(),
            state: "expired".into(),
            size_base_units: Some(10),
            offer_side: Some("sell".into()),
            cancel_input_coin_id: retained,
            fixed_delegated_puzzle_hash: "hh".into(),
            offer_nonce: Some("nn".into()),
            listing_expires_at: None,
        }));

        let mut io = SoftExpireIoOverrides {
            ensure_results: VecDeque::from([ensure_result]),
            // surplus offer-b in ensure group + unwanted-size offer-c reclaim plan item
            reclaim_results: VecDeque::from([
                ReclaimMakerOutcome::Reclaimed {
                    superseded_offer_id: "offer-b".into(),
                },
                ReclaimMakerOutcome::Reclaimed {
                    superseded_offer_id: "offer-c".into(),
                },
            ]),
        };
        run_soft_expire_phase_inner(&write_store, &bundle.cycle_context(), &market, &mut io)
            .await
            .expect("soft expire");
        assert!(
            io.ensure_results.is_empty() && io.reclaim_results.is_empty(),
            "protected maker must skip surplus reclaim; exactly two reclaims expected"
        );
    }
}
