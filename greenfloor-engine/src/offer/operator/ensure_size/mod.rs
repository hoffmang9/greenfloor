//! Ensure a size-N offer exists: prefer matching idle maker (`PresplitExisting`), else reclaim+new, else `PresplitNew`.

mod selection;

use chia_protocol::Bytes32;
use tracing::{info, warn};

use crate::bech32m::decode_address;
use crate::coinset::{client_for_signer_on_network, LiveCoinset, OfferCoinsetBackend};
use crate::config::{market_wants_ladder_size, MarketConfig, SignerConfig};
use crate::error::{SignerError, SignerResult};
use crate::hex::{hex_to_bytes32, normalize_hex_id, tree_hash_to_hex};
use crate::offer::action::plan_offer_terms_for_market;
use crate::offer::lifecycle::supersede_listing_for_repost_synced;
use crate::offer::presplit::PresplitOfferBinding;
use crate::offer::reclaim::reclaim_presplit_maker_coin;
use crate::offer::request::normalize_offer_side;
use crate::offer::types::{OfferTerms, PresplitMakerReuse};
use crate::storage::{upsert_offer_post_record, CycleWriteStore, ReusablePresplitMakerRow};
use crate::vault::session::resolve_vault_spend_context;

use self::selection::{
    decide_ensure_candidate, select_from_decisions, EnsureCandidateDecision, EnsureSelection,
};
use super::{
    build_and_post_offer_with_persist_artifacts, flush_build_and_post_persist,
    BuildAndPostOfferRequest, BuildAndPostOfferRequestParts,
};

/// Result of [`ensure_size_n_offer`]. Variants own protection fields for surplus reclaim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureSizeResult {
    PostedExisting {
        superseded_offer_id: String,
        retained_maker_coin_id: String,
    },
    PostedNew,
    ReclaimedThenPostedNew {
        superseded_offer_id: String,
    },
    /// Ensure reclaimed a mismatched maker then failed to post a replacement.
    ReclaimedOnly {
        superseded_offer_id: String,
    },
    /// No venue listing created. Optional retained coin blocks surplus reclaim after failed `PreferExisting`.
    Skipped {
        retained_maker_coin_id: Option<String>,
    },
}

impl EnsureSizeResult {
    fn skipped() -> Self {
        Self::Skipped {
            retained_maker_coin_id: None,
        }
    }

    /// True when a venue listing was created/re-posted (not reclaim-only or skip).
    #[must_use]
    pub fn posted(&self) -> bool {
        matches!(
            self,
            Self::PostedExisting { .. } | Self::PostedNew | Self::ReclaimedThenPostedNew { .. }
        )
    }

    /// True when surplus reclaim must leave this expired row alone.
    #[must_use]
    pub fn protects_maker(&self, row: &ReusablePresplitMakerRow) -> bool {
        match self {
            Self::PostedExisting {
                superseded_offer_id,
                retained_maker_coin_id,
            } => {
                retained_maker_coin_id == &row.cancel_input_coin_id
                    || superseded_offer_id == &row.offer_id
            }
            Self::ReclaimedThenPostedNew {
                superseded_offer_id,
            }
            | Self::ReclaimedOnly {
                superseded_offer_id,
            } => superseded_offer_id == &row.offer_id,
            Self::Skipped {
                retained_maker_coin_id: Some(coin),
            } => coin == &row.cancel_input_coin_id,
            Self::PostedNew
            | Self::Skipped {
                retained_maker_coin_id: None,
            } => false,
        }
    }
}

/// Outcome of reclaiming one expired maker outside the ensure post path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReclaimMakerOutcome {
    Reclaimed { superseded_offer_id: String },
    Skipped,
}

/// Shared plan inputs for hash compare (must match create/post asset resolution).
struct EnsurePlan {
    terms: OfferTerms,
    receive_puzzle_hash: Bytes32,
    launcher_id: Bytes32,
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

async fn coin_unspent<C: OfferCoinsetBackend>(backend: &C, coin_id: &str) -> SignerResult<bool> {
    let id = hex_to_bytes32(coin_id)?;
    Ok(!backend.offer_input_coin_is_spent(id).await?)
}

fn with_maker_reuse(
    parts: &BuildAndPostOfferRequestParts,
    reuse: Option<&ReusablePresplitMakerRow>,
) -> BuildAndPostOfferRequestParts {
    let mut next = parts.clone();
    next.maker_reuse = reuse.and_then(|row| {
        let nonce = row.offer_nonce.as_deref()?.trim();
        if nonce.is_empty() {
            return None;
        }
        Some(PresplitMakerReuse {
            coin_id: row.cancel_input_coin_id.clone(),
            offer_nonce: nonce.to_string(),
        })
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

async fn candidate_decision<C: OfferCoinsetBackend>(
    backend: &C,
    plan: &EnsurePlan,
    candidate: &ReusablePresplitMakerRow,
) -> SignerResult<Option<EnsureCandidateDecision>> {
    let unspent = coin_unspent(backend, &candidate.cancel_input_coin_id).await?;
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
    Ok(decide_ensure_candidate(
        unspent, has_nonce, &planned, &stored,
    ))
}

/// Prefer a hash-matching maker; stop Coinset checks once `PreferExisting` is found.
async fn select_ensure_candidate<C: OfferCoinsetBackend>(
    backend: &C,
    plan: &EnsurePlan,
    candidates: &[ReusablePresplitMakerRow],
) -> SignerResult<EnsureSelection> {
    let mut decisions = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let decision = candidate_decision(backend, plan, candidate).await?;
        if matches!(decision, Some(EnsureCandidateDecision::PreferExisting)) {
            decisions.push(decision);
            break;
        }
        decisions.push(decision);
    }
    Ok(select_from_decisions(&decisions))
}

async fn apply_prefer_existing(
    write_store: &CycleWriteStore,
    parts: &BuildAndPostOfferRequestParts,
    market_id: &str,
    candidate: &ReusablePresplitMakerRow,
) -> SignerResult<EnsureSizeResult> {
    info!(
        offer_id = %candidate.offer_id,
        size = parts.size_base_units,
        "ensure_size_n_offer: PresplitExisting re-offer"
    );
    let posted = post_offer(parts, write_store, Some(candidate)).await?;
    if !posted {
        // Keep the selected maker out of surplus reclaim; coin is still the intended reuse target.
        return Ok(EnsureSizeResult::Skipped {
            retained_maker_coin_id: Some(candidate.cancel_input_coin_id.clone()),
        });
    }
    supersede_listing_for_repost_synced(
        write_store,
        market_id,
        &candidate.offer_id,
        parts.run.dry_run,
    )?;
    Ok(EnsureSizeResult::PostedExisting {
        superseded_offer_id: candidate.offer_id.clone(),
        retained_maker_coin_id: candidate.cancel_input_coin_id.clone(),
    })
}

async fn apply_reclaim_then_new(
    write_store: &CycleWriteStore,
    signer: SignerConfig,
    parts: &BuildAndPostOfferRequestParts,
    market_id: &str,
    candidate: &ReusablePresplitMakerRow,
) -> SignerResult<EnsureSizeResult> {
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
    reclaim_presplit_maker_coin(
        signer,
        &parts.network,
        &candidate.cancel_input_coin_id,
        &candidate.fixed_delegated_puzzle_hash,
        parts.run.dry_run,
    )
    .await?;
    // Coin is spent: retire listing even if the replacement post soft-fails.
    supersede_listing_for_repost_synced(
        write_store,
        market_id,
        &candidate.offer_id,
        parts.run.dry_run,
    )?;
    let posted = post_offer(parts, write_store, None).await?;
    Ok(result_after_reclaim_then_post(
        posted,
        candidate.offer_id.clone(),
    ))
}

fn result_after_reclaim_then_post(posted: bool, superseded_offer_id: String) -> EnsureSizeResult {
    if posted {
        EnsureSizeResult::ReclaimedThenPostedNew {
            superseded_offer_id,
        }
    } else {
        // Coin already reclaimed; do not report Skipped.
        EnsureSizeResult::ReclaimedOnly {
            superseded_offer_id,
        }
    }
}

/// Prefer an existing matching maker for size N; reclaim+rebuild on hash mismatch; else `PresplitNew`.
///
/// Hash compare uses the same market asset resolution as create/post (`resolve_market_assets`).
///
/// # Errors
///
/// Returns an error when planning, reclaim, or post fails hard (soft post failures return Ok outcome).
pub async fn ensure_size_n_offer(
    write_store: &CycleWriteStore,
    signer: SignerConfig,
    ticker_index: &crate::config::CatTickerIndex,
    market: &MarketConfig,
    parts: BuildAndPostOfferRequestParts,
    candidates: Option<Vec<ReusablePresplitMakerRow>>,
) -> SignerResult<EnsureSizeResult> {
    let side = normalize_offer_side(parts.action_side.as_deref().unwrap_or("sell")).to_string();
    let size_i64 =
        i64::try_from(parts.size_base_units).map_err(|_| SignerError::InvalidSizeBaseUnits)?;
    if !market_wants_ladder_size(market, &side, size_i64) {
        return Ok(EnsureSizeResult::skipped());
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
    let candidates = match candidates {
        Some(rows) => rows,
        None => write_store.sync(|store| {
            store.list_reusable_presplit_makers_for_size(&market_id, size_i64, Some(&side))
        })?,
    };
    let selection = select_ensure_candidate(&backend, &plan, &candidates).await?;

    match selection {
        EnsureSelection::PreferExisting(idx) => {
            apply_prefer_existing(write_store, &parts, &market_id, &candidates[idx]).await
        }
        EnsureSelection::ReclaimThenNew(idx) => {
            apply_reclaim_then_new(write_store, signer, &parts, &market_id, &candidates[idx]).await
        }
        EnsureSelection::New => {
            let posted = post_offer(&parts, write_store, None).await?;
            Ok(if posted {
                EnsureSizeResult::PostedNew
            } else {
                EnsureSizeResult::skipped()
            })
        }
    }
}

/// Reclaim an expired maker when the ladder no longer wants its size.
///
/// # Errors
///
/// Returns an error when reclaim build/broadcast fails.
pub async fn reclaim_expired_maker_if_unspent(
    write_store: &CycleWriteStore,
    signer: SignerConfig,
    network: &str,
    row: &ReusablePresplitMakerRow,
    dry_run: bool,
) -> SignerResult<ReclaimMakerOutcome> {
    let coinset_client = client_for_signer_on_network(&signer, network)?;
    let backend = LiveCoinset(&coinset_client);
    if !coin_unspent(&backend, &row.cancel_input_coin_id).await? {
        return Ok(ReclaimMakerOutcome::Skipped);
    }
    reclaim_presplit_maker_coin(
        signer,
        network,
        &row.cancel_input_coin_id,
        &row.fixed_delegated_puzzle_hash,
        dry_run,
    )
    .await?;
    supersede_listing_for_repost_synced(write_store, &row.market_id, &row.offer_id, dry_run)?;
    Ok(ReclaimMakerOutcome::Reclaimed {
        superseded_offer_id: row.offer_id.clone(),
    })
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

    #[test]
    fn protect_maker_retains_prefer_existing_coin() {
        let result = EnsureSizeResult::PostedExisting {
            superseded_offer_id: "offer-a".into(),
            retained_maker_coin_id: "coin-a".into(),
        };
        let protected = ReusablePresplitMakerRow {
            offer_id: "offer-a".into(),
            market_id: "m1".into(),
            state: "expired".into(),
            size_base_units: Some(10),
            offer_side: Some("sell".into()),
            cancel_input_coin_id: "coin-a".into(),
            fixed_delegated_puzzle_hash: "hh".into(),
            offer_nonce: Some("nn".into()),
            listing_expires_at: None,
        };
        let surplus = ReusablePresplitMakerRow {
            offer_id: "offer-b".into(),
            cancel_input_coin_id: "coin-b".into(),
            ..protected.clone()
        };
        assert!(result.protects_maker(&protected));
        assert!(!result.protects_maker(&surplus));
    }

    #[test]
    fn reclaim_then_failed_post_is_reclaimed_only_not_skipped() {
        assert_eq!(
            result_after_reclaim_then_post(false, "x".into()),
            EnsureSizeResult::ReclaimedOnly {
                superseded_offer_id: "x".into(),
            }
        );
        assert_eq!(
            result_after_reclaim_then_post(true, "x".into()),
            EnsureSizeResult::ReclaimedThenPostedNew {
                superseded_offer_id: "x".into(),
            }
        );
        assert!(!EnsureSizeResult::ReclaimedOnly {
            superseded_offer_id: "x".into(),
        }
        .posted());
        assert!(EnsureSizeResult::ReclaimedThenPostedNew {
            superseded_offer_id: "x".into(),
        }
        .posted());
    }

    #[test]
    fn failed_prefer_existing_still_protects_maker_coin() {
        let result = EnsureSizeResult::Skipped {
            retained_maker_coin_id: Some("coin-a".into()),
        };
        let selected = ReusablePresplitMakerRow {
            offer_id: "offer-a".into(),
            market_id: "m1".into(),
            state: "expired".into(),
            size_base_units: Some(10),
            offer_side: Some("sell".into()),
            cancel_input_coin_id: "coin-a".into(),
            fixed_delegated_puzzle_hash: "hh".into(),
            offer_nonce: Some("nn".into()),
            listing_expires_at: None,
        };
        let surplus = ReusablePresplitMakerRow {
            offer_id: "offer-b".into(),
            cancel_input_coin_id: "coin-b".into(),
            ..selected.clone()
        };
        assert!(result.protects_maker(&selected));
        assert!(!result.protects_maker(&surplus));
        assert!(!result.posted());
    }

    #[tokio::test]
    async fn coin_unspent_uses_spent_flag() {
        let unspent = coin_unspent(&SpentFlagCoinset { spent: false }, &"aa".repeat(32))
            .await
            .expect("unspent");
        assert!(unspent);
        let spent = coin_unspent(&SpentFlagCoinset { spent: true }, &"aa".repeat(32))
            .await
            .expect("spent");
        assert!(!spent);

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
        let selection = select_ensure_candidate(&SpentFlagCoinset { spent: true }, &plan, &[row])
            .await
            .expect("select");
        assert_eq!(selection, EnsureSelection::New);
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
        let result = ensure_size_n_offer(
            &write_store,
            test_signer_config("https://api.coinset.org"),
            &empty_cat_ticker_index(),
            &market,
            parts,
            Some(Vec::new()),
        )
        .await
        .expect("ensure");
        assert!(!result.posted());
        assert!(matches!(
            result,
            EnsureSizeResult::Skipped {
                retained_maker_coin_id: None
            }
        ));
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
            ..row
        };
        assert!(with_maker_reuse(&parts, Some(&empty_nonce))
            .maker_reuse
            .is_none());
        assert!(with_maker_reuse(&parts, None).maker_reuse.is_none());
    }

    #[test]
    fn protect_maker_covers_remaining_result_variants() {
        let row = ReusablePresplitMakerRow {
            offer_id: "offer-a".into(),
            market_id: "m1".into(),
            state: "expired".into(),
            size_base_units: Some(10),
            offer_side: Some("sell".into()),
            cancel_input_coin_id: "coin-a".into(),
            fixed_delegated_puzzle_hash: "hh".into(),
            offer_nonce: Some("nn".into()),
            listing_expires_at: None,
        };
        assert!(EnsureSizeResult::ReclaimedThenPostedNew {
            superseded_offer_id: "offer-a".into(),
        }
        .protects_maker(&row));
        assert!(EnsureSizeResult::ReclaimedOnly {
            superseded_offer_id: "offer-a".into(),
        }
        .protects_maker(&row));
        assert!(!EnsureSizeResult::PostedNew.protects_maker(&row));
        assert!(!EnsureSizeResult::skipped().protects_maker(&row));
        assert!(EnsureSizeResult::PostedNew.posted());
        assert!(!EnsureSizeResult::skipped().posted());
    }
}
