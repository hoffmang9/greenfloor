//! Ensure a size-N offer exists: prefer matching idle maker (`PreferExisting`), else reclaim+new, else `PresplitNew`.

mod selection;

use chia_protocol::Bytes32;
use tracing::{info, warn};

use crate::bech32m::decode_address;
use crate::coinset::{client_for_signer_on_network, coin_id_is_unspent, LiveCoinset};
use crate::config::{market_wants_ladder_size, MarketConfig, SignerConfig};
use crate::error::{SignerError, SignerResult};
use crate::hex::{hex_to_bytes32, normalize_hex_id, tree_hash_to_hex};
use crate::offer::action::plan_offer_terms_for_market;
use crate::offer::lifecycle::{restore_stale_maker_claims_synced, ExpiredMakerLease};
use crate::offer::presplit::PresplitOfferBinding;
use crate::offer::reclaim::reclaim_presplit_maker_coin;
use crate::offer::request::effective_offer_side;
use crate::offer::types::{effective_maker_reuse, OfferTerms, PresplitMakerReuse};
use crate::storage::{upsert_offer_post_record, CycleWriteStore, ReusablePresplitMakerRow};
use crate::vault::session::resolve_vault_spend_context;

use self::selection::{decide_ensure_reuse, EnsureReuseKind};
use super::{
    build_and_post_offer_with_persist_artifacts, flush_build_and_post_persist,
    BuildAndPostOfferRequest, BuildAndPostOfferRequestParts,
};

/// Shared plan inputs for hash compare (must match create/post asset resolution).
struct EnsurePlan {
    terms: OfferTerms,
    receive_puzzle_hash: Bytes32,
    launcher_id: Bytes32,
}

/// How to use a claimed reusable maker after Coinset confirms unspent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReuseKind {
    Reoffer,
    ReclaimAndPost,
}

async fn plan_ensure(
    signer: &SignerConfig,
    ticker_index: &crate::config::CatTickerIndex,
    network: &str,
    market: &MarketConfig,
    size_base_units: u64,
    side: &str,
) -> SignerResult<EnsurePlan> {
    let terms =
        plan_offer_terms_for_market(signer, ticker_index, network, market, size_base_units, side)
            .await?;
    let receive_puzzle_hash = decode_address(&terms.receive_address)?;
    let vault_ctx = resolve_vault_spend_context(signer.clone()).await?;
    Ok(EnsurePlan {
        terms,
        receive_puzzle_hash,
        launcher_id: vault_ctx.launcher_id,
    })
}

fn planned_fixed_hash(
    launcher_id: Bytes32,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
) -> SignerResult<String> {
    let binding = PresplitOfferBinding::plan(launcher_id, terms, receive_puzzle_hash, offer_nonce)?;
    Ok(tree_hash_to_hex(binding.fixed_conditions_tree_hash))
}

fn with_maker_reuse(
    parts: &BuildAndPostOfferRequestParts,
    reuse: Option<&ReusablePresplitMakerRow>,
) -> BuildAndPostOfferRequestParts {
    let mut next = parts.clone();
    next.maker_reuse = reuse.and_then(|row| {
        let candidate = PresplitMakerReuse {
            coin_id: row.cancel_input_coin_id.clone(),
            offer_nonce: row.offer_nonce.clone().unwrap_or_default(),
        };
        effective_maker_reuse(Some(&candidate)).cloned()
    });
    next
}

async fn post_offer(
    parts: &BuildAndPostOfferRequestParts,
    write_store: &CycleWriteStore,
    reuse: Option<&ReusablePresplitMakerRow>,
) -> SignerResult<bool> {
    let post_request = BuildAndPostOfferRequest::from_parts(with_maker_reuse(parts, reuse));
    let persist_store = write_store.clone();
    let mut persist = move |record: &crate::storage::OfferPostPersistRecord| {
        persist_store.sync(|store| upsert_offer_post_record(store, record))
    };
    let (response, artifacts) =
        build_and_post_offer_with_persist_artifacts(post_request, Some(&mut persist)).await?;
    if let Some(artifacts) = artifacts {
        let store = write_store.lock()?;
        flush_build_and_post_persist(&store, &artifacts)?;
    }
    Ok(response.exit_code == 0)
}

fn local_reuse_kind(
    plan: &EnsurePlan,
    candidate: &ReusablePresplitMakerRow,
) -> SignerResult<ReuseKind> {
    let has_nonce = candidate.offer_nonce.is_some();
    let planned = if let Some(nonce_hex) = candidate.offer_nonce.as_deref() {
        let offer_nonce = hex_to_bytes32(nonce_hex)?;
        planned_fixed_hash(
            plan.launcher_id,
            &plan.terms,
            plan.receive_puzzle_hash,
            offer_nonce,
        )?
    } else {
        String::new()
    };
    let stored = normalize_hex_id(&candidate.fixed_delegated_puzzle_hash);
    Ok(match decide_ensure_reuse(has_nonce, &planned, &stored) {
        EnsureReuseKind::PreferExisting => ReuseKind::Reoffer,
        EnsureReuseKind::ReclaimThenNew => ReuseKind::ReclaimAndPost,
    })
}

/// Prefer hash-match reoffer, else first reclaim — no Coinset yet.
fn pick_local_reuse(
    plan: &EnsurePlan,
    candidates: &[ReusablePresplitMakerRow],
) -> SignerResult<Option<(usize, ReuseKind)>> {
    let mut reclaim = None;
    for (idx, candidate) in candidates.iter().enumerate() {
        match local_reuse_kind(plan, candidate)? {
            ReuseKind::Reoffer => return Ok(Some((idx, ReuseKind::Reoffer))),
            ReuseKind::ReclaimAndPost if reclaim.is_none() => {
                reclaim = Some((idx, ReuseKind::ReclaimAndPost));
            }
            ReuseKind::ReclaimAndPost => {}
        }
    }
    Ok(reclaim)
}

/// Prefer an existing matching maker for size N; reclaim+rebuild on hash mismatch; else `PresplitNew`.
///
/// Returns `true` when a venue listing was created/re-posted.
///
/// # Errors
///
/// Returns an error when planning, reclaim, or post fails hard (soft post failures return `Ok(false)`).
pub async fn ensure_size_n_offer(
    write_store: &CycleWriteStore,
    signer: SignerConfig,
    ticker_index: &crate::config::CatTickerIndex,
    market: &MarketConfig,
    parts: BuildAndPostOfferRequestParts,
) -> SignerResult<bool> {
    let side = effective_offer_side(parts.action_side.as_deref()).to_string();
    let size_i64 =
        i64::try_from(parts.size_base_units).map_err(|_| SignerError::InvalidSizeBaseUnits)?;
    if !market_wants_ladder_size(market, &side, size_i64) {
        return Ok(false);
    }

    let plan = plan_ensure(
        &signer,
        ticker_index,
        &parts.network,
        market,
        parts.size_base_units,
        &side,
    )
    .await?;
    let coinset_client = client_for_signer_on_network(&signer, &parts.network)?;
    let backend = LiveCoinset(&coinset_client);

    let market_id = market.market_id.clone();
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let _ = restore_stale_maker_claims_synced(write_store, now_unix, parts.run.dry_run)?;
    let mut candidates = write_store.sync(|store| {
        store.list_reusable_presplit_makers_for_size(&market_id, size_i64, Some(&side))
    })?;

    // Claim the locally chosen PreferExisting/reclaim candidate, then Coinset-verify unspent.
    // Spent pirates are retired without blocking PreferExisting of another coin. Lost CAS:
    // drop and continue so parallel gap fill still fills this worker's slot.
    loop {
        let Some((idx, kind)) = pick_local_reuse(&plan, &candidates)? else {
            return post_offer(&parts, write_store, None).await;
        };
        let candidate = candidates[idx].clone();
        let Some(lease) = ExpiredMakerLease::try_claim(
            write_store,
            &market_id,
            &candidate.offer_id,
            parts.run.dry_run,
        )?
        else {
            candidates.remove(idx);
            continue;
        };
        let unspent = coin_id_is_unspent(&backend, &candidate.cancel_input_coin_id).await?;
        if !unspent {
            info!(
                offer_id = %candidate.offer_id,
                "ensure_size_n_offer: retiring spent expired maker"
            );
            lease.commit()?;
            candidates.remove(idx);
            continue;
        }
        return match kind {
            ReuseKind::Reoffer => apply_reoffer(write_store, &parts, &candidate, lease).await,
            ReuseKind::ReclaimAndPost => {
                apply_reclaim_and_post(write_store, signer, &parts, &candidate, lease).await
            }
        };
    }
}

async fn apply_reoffer(
    write_store: &CycleWriteStore,
    parts: &BuildAndPostOfferRequestParts,
    candidate: &ReusablePresplitMakerRow,
    lease: ExpiredMakerLease<'_>,
) -> SignerResult<bool> {
    info!(
        offer_id = %candidate.offer_id,
        size = parts.size_base_units,
        "ensure_size_n_offer: PresplitExisting re-offer"
    );
    let posted = lease
        .run_with_heartbeat(post_offer(parts, write_store, Some(candidate)))
        .await?;
    if posted {
        lease.commit()?;
    }
    // Drop restores expired when PreferExisting post failed (coin still reusable).
    Ok(posted)
}

async fn apply_reclaim_and_post(
    write_store: &CycleWriteStore,
    signer: SignerConfig,
    parts: &BuildAndPostOfferRequestParts,
    candidate: &ReusablePresplitMakerRow,
    lease: ExpiredMakerLease<'_>,
) -> SignerResult<bool> {
    if candidate.offer_nonce.is_some() {
        info!(
            offer_id = %candidate.offer_id,
            size = parts.size_base_units,
            "ensure_size_n_offer: CONDITIONS hash mismatch; reclaim then PresplitNew"
        );
    } else {
        warn!(
            offer_id = %candidate.offer_id,
            "presplit maker missing offer_nonce; reclaiming for rebuild"
        );
    }
    lease
        .run_with_heartbeat(reclaim_presplit_maker_coin(
            signer,
            &parts.network,
            &candidate.cancel_input_coin_id,
            &candidate.fixed_delegated_puzzle_hash,
            parts.run.dry_run,
        ))
        .await?;
    // Coin spent: finalize claim even if the replacement post fails.
    lease.commit()?;
    post_offer(parts, write_store, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::noop_coinset::SpentFlagCoinset;

    #[test]
    fn planned_hash_ignores_listing_expiry_when_soft() {
        let launcher = Bytes32::new([0x11; 32]);
        let receive = Bytes32::new([0x22; 32]);
        let nonce = Bytes32::new([0x33; 32]);
        let soft = OfferTerms {
            receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w"
                .to_string(),
            offer_asset_id: hex::encode(Bytes32::new([0x44; 32])),
            offer_amount: 10_000,
            request_asset_id: "xch".to_string(),
            request_amount: 1,
            expires_at: Some(4_000_000_000),
            bake_expiry_into_conditions: false,
        };
        let hard = OfferTerms {
            bake_expiry_into_conditions: true,
            ..soft.clone()
        };
        let soft_hash = planned_fixed_hash(launcher, &soft, receive, nonce).expect("soft");
        let soft_hash2 = planned_fixed_hash(
            launcher,
            &OfferTerms {
                expires_at: Some(4_100_000_000),
                ..soft
            },
            receive,
            nonce,
        )
        .expect("soft2");
        let hard_hash = planned_fixed_hash(launcher, &hard, receive, nonce).expect("hard");
        assert_eq!(soft_hash, soft_hash2);
        assert_ne!(soft_hash, hard_hash);
    }

    #[tokio::test]
    async fn coin_unspent_uses_spent_flag() {
        let unspent = coin_id_is_unspent(&SpentFlagCoinset { spent: false }, &"aa".repeat(32))
            .await
            .expect("unspent");
        assert!(unspent);
        let spent = coin_id_is_unspent(&SpentFlagCoinset { spent: true }, &"aa".repeat(32))
            .await
            .expect("spent");
        assert!(!spent);
    }

    #[test]
    fn pick_local_reuse_prefers_hash_match_over_reclaim() {
        let terms = OfferTerms {
            receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w".into(),
            offer_asset_id: "bb".repeat(32),
            offer_amount: 1_000,
            request_asset_id: "xch".into(),
            request_amount: 1,
            expires_at: Some(4_000_000_000),
            bake_expiry_into_conditions: false,
        };
        let plan = EnsurePlan {
            terms: terms.clone(),
            receive_puzzle_hash: Bytes32::new([0x22; 32]),
            launcher_id: Bytes32::new([0x11; 32]),
        };
        let nonce = Bytes32::new([0x33; 32]);
        let match_hash = planned_fixed_hash(
            plan.launcher_id,
            &plan.terms,
            plan.receive_puzzle_hash,
            nonce,
        )
        .expect("hash");
        let mismatch = ReusablePresplitMakerRow {
            offer_id: "o-reclaim".into(),
            market_id: "m1".into(),
            state: "expired".into(),
            size_base_units: Some(10),
            offer_side: Some("sell".into()),
            cancel_input_coin_id: "aa".repeat(32),
            fixed_delegated_puzzle_hash: "cc".repeat(32),
            offer_nonce: Some(hex::encode(nonce)),
            listing_expires_at: None,
        };
        let matched = ReusablePresplitMakerRow {
            offer_id: "o-match".into(),
            cancel_input_coin_id: "ee".repeat(32),
            fixed_delegated_puzzle_hash: match_hash,
            ..mismatch.clone()
        };
        let (idx, kind) = pick_local_reuse(&plan, &[mismatch, matched])
            .expect("pick")
            .expect("some");
        assert_eq!(idx, 1);
        assert_eq!(kind, ReuseKind::Reoffer);
    }

    #[tokio::test]
    async fn ensure_skips_when_ladder_does_not_want_size() {
        use crate::config::{empty_cat_ticker_index, ManagerProgramConfig};
        use crate::storage::CycleWriteStore;
        use crate::test_support::market_config::sample_market;
        use crate::test_support::signer_config::test_signer_config;
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let write_store = CycleWriteStore::open(&dir.path().join("state.db")).expect("open");
        let market = sample_market("xch1test");
        let program = ManagerProgramConfig::default();
        let paths = super::super::OperatorConfigPaths {
            program_path: dir.path().join("program.yaml"),
            markets_path: dir.path().join("markets.yaml"),
            testnet_markets_path: None,
        };
        let parts = BuildAndPostOfferRequestParts::for_ensure_size(
            &paths,
            &program,
            "mainnet",
            market.market_id.clone(),
            10,
            "sell",
        );
        let posted = ensure_size_n_offer(
            &write_store,
            test_signer_config("https://api.coinset.org"),
            &empty_cat_ticker_index(),
            &market,
            parts,
        )
        .await
        .expect("ensure");
        assert!(!posted);
    }

    #[test]
    fn lost_reuse_claim_drop_then_select_reaches_post_new() {
        // Mirrors the ensure loop after a lost CAS: remove the reuse idx, re-select.
        // With no remaining candidates the worker must PresplitNew (not abort).
        let terms = OfferTerms {
            receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w".into(),
            offer_asset_id: "bb".repeat(32),
            offer_amount: 1_000,
            request_asset_id: "xch".into(),
            request_amount: 1,
            expires_at: Some(4_000_000_000),
            bake_expiry_into_conditions: false,
        };
        let row = ReusablePresplitMakerRow {
            offer_id: "o1".into(),
            market_id: "m1".into(),
            state: "expired".into(),
            size_base_units: Some(10),
            offer_side: Some("sell".into()),
            cancel_input_coin_id: "aa".repeat(32),
            fixed_delegated_puzzle_hash: "cc".repeat(32),
            offer_nonce: Some("dd".repeat(32)),
            listing_expires_at: None,
        };
        let plan = EnsurePlan {
            terms,
            receive_puzzle_hash: Bytes32::new([0x22; 32]),
            launcher_id: Bytes32::new([0x11; 32]),
        };
        let mut candidates = vec![row];
        let (idx, _) = pick_local_reuse(&plan, &candidates)
            .expect("pick")
            .expect("reuse");
        candidates.remove(idx);
        assert!(pick_local_reuse(&plan, &candidates)
            .expect("re-pick")
            .is_none());
    }

    #[test]
    fn lost_reuse_claim_drop_then_select_keeps_other_candidate() {
        let terms = OfferTerms {
            receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w".into(),
            offer_asset_id: "bb".repeat(32),
            offer_amount: 1_000,
            request_asset_id: "xch".into(),
            request_amount: 1,
            expires_at: Some(4_000_000_000),
            bake_expiry_into_conditions: false,
        };
        let row = |offer_id: &str, coin: &str| ReusablePresplitMakerRow {
            offer_id: offer_id.into(),
            market_id: "m1".into(),
            state: "expired".into(),
            size_base_units: Some(10),
            offer_side: Some("sell".into()),
            cancel_input_coin_id: coin.repeat(32),
            fixed_delegated_puzzle_hash: "cc".repeat(32),
            offer_nonce: Some("dd".repeat(32)),
            listing_expires_at: None,
        };
        let plan = EnsurePlan {
            terms,
            receive_puzzle_hash: Bytes32::new([0x22; 32]),
            launcher_id: Bytes32::new([0x11; 32]),
        };
        let mut candidates = vec![row("o1", "aa"), row("o2", "ee")];
        let (idx, _) = pick_local_reuse(&plan, &candidates)
            .expect("pick first")
            .expect("reuse");
        assert_eq!(candidates[idx].offer_id, "o1");
        candidates.remove(idx);
        let (idx2, _) = pick_local_reuse(&plan, &candidates)
            .expect("pick second")
            .expect("second reuse");
        assert_eq!(candidates[idx2].offer_id, "o2");
    }

    #[test]
    fn with_maker_reuse_maps_nonce_and_skips_empty() {
        use crate::config::ManagerProgramConfig;
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let program = ManagerProgramConfig::default();
        let paths = super::super::OperatorConfigPaths {
            program_path: dir.path().join("program.yaml"),
            markets_path: dir.path().join("markets.yaml"),
            testnet_markets_path: None,
        };
        let parts = BuildAndPostOfferRequestParts::for_ensure_size(
            &paths, &program, "mainnet", "m1", 10, "sell",
        );
        let row = ReusablePresplitMakerRow {
            offer_id: "o1".into(),
            market_id: "m1".into(),
            state: "expired".into(),
            size_base_units: Some(10),
            offer_side: Some("sell".into()),
            cancel_input_coin_id: "aa".repeat(32),
            fixed_delegated_puzzle_hash: "bb".repeat(32),
            offer_nonce: Some("cc".repeat(32)),
            listing_expires_at: None,
        };
        let mapped = with_maker_reuse(&parts, Some(&row));
        let reuse = mapped.maker_reuse.expect("reuse");
        assert_eq!(reuse.coin_id, "aa".repeat(32));
        assert_eq!(reuse.offer_nonce, "cc".repeat(32));
        let empty_nonce = ReusablePresplitMakerRow {
            offer_nonce: Some("   ".into()),
            ..row.clone()
        };
        assert!(with_maker_reuse(&parts, Some(&empty_nonce))
            .maker_reuse
            .is_none());
        let empty_coin = ReusablePresplitMakerRow {
            cancel_input_coin_id: String::new(),
            ..row
        };
        assert!(with_maker_reuse(&parts, Some(&empty_coin))
            .maker_reuse
            .is_none());
        assert!(with_maker_reuse(&parts, None).maker_reuse.is_none());
    }
}
