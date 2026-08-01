//! Ensure a size-N offer exists: prefer matching idle maker (`PresplitExisting`), else reclaim+new, else `PresplitNew`.

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

use super::{
    build_and_post_offer_with_persist_artifacts, flush_build_and_post_persist,
    BuildAndPostOfferRequest, BuildAndPostOfferRequestParts,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureSizeOutcome {
    PostedExisting,
    PostedNew,
    ReclaimedThenPostedNew,
    ReclaimedOnly,
    Skipped,
}

/// Result of [`ensure_size_n_offer`], including which maker was consumed so surplus reclaim can skip it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureSizeResult {
    pub outcome: EnsureSizeOutcome,
    /// Listing retired by reclaim or `PreferExisting` re-post.
    pub superseded_offer_id: Option<String>,
    /// Maker coin retained by `PreferExisting` (must not surplus-reclaim).
    pub retained_maker_coin_id: Option<String>,
}

impl EnsureSizeResult {
    fn skipped() -> Self {
        Self {
            outcome: EnsureSizeOutcome::Skipped,
            superseded_offer_id: None,
            retained_maker_coin_id: None,
        }
    }

    #[must_use]
    pub fn posted(&self) -> bool {
        !matches!(self.outcome, EnsureSizeOutcome::Skipped)
    }

    /// True when surplus reclaim must leave this expired row alone.
    #[must_use]
    pub fn protects_maker(&self, row: &ReusablePresplitMakerRow) -> bool {
        if self
            .retained_maker_coin_id
            .as_deref()
            .is_some_and(|coin| coin == row.cancel_input_coin_id)
        {
            return true;
        }
        self.superseded_offer_id
            .as_deref()
            .is_some_and(|id| id == row.offer_id)
    }
}

/// Pure selection among reusable makers before Coinset/post I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnsureCandidateDecision {
    PreferExisting,
    ReclaimThenNew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnsureSelection {
    PreferExisting(usize),
    ReclaimThenNew(usize),
    New,
}

fn decide_ensure_candidate(
    unspent: bool,
    has_offer_nonce: bool,
    planned_hash: &str,
    stored_hash: &str,
) -> Option<EnsureCandidateDecision> {
    if !unspent {
        return None;
    }
    if !has_offer_nonce {
        return Some(EnsureCandidateDecision::ReclaimThenNew);
    }
    Some(
        if normalize_hex_id(planned_hash) == normalize_hex_id(stored_hash) {
            EnsureCandidateDecision::PreferExisting
        } else {
            EnsureCandidateDecision::ReclaimThenNew
        },
    )
}

/// Short-circuit on `PreferExisting`; otherwise first reclaim index or New.
#[cfg(test)]
fn select_from_decisions(decisions: &[Option<EnsureCandidateDecision>]) -> EnsureSelection {
    let mut reclaim_idx = None;
    for (idx, decision) in decisions.iter().enumerate() {
        match decision {
            Some(EnsureCandidateDecision::PreferExisting) => {
                return EnsureSelection::PreferExisting(idx);
            }
            Some(EnsureCandidateDecision::ReclaimThenNew) if reclaim_idx.is_none() => {
                reclaim_idx = Some(idx);
            }
            _ => {}
        }
    }
    reclaim_idx.map_or(EnsureSelection::New, EnsureSelection::ReclaimThenNew)
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
    match backend.fetch_offer_input_cat(id).await {
        Ok(_) => Ok(true),
        Err(SignerError::PresplitCoinNotFound) => Ok(false),
        Err(err) => Err(err),
    }
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
    launcher_id: Bytes32,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    candidate: &ReusablePresplitMakerRow,
) -> SignerResult<Option<EnsureCandidateDecision>> {
    let unspent = coin_unspent(backend, &candidate.cancel_input_coin_id).await?;
    let has_nonce = candidate.offer_nonce.is_some();
    let planned = if let Some(nonce_hex) = candidate.offer_nonce.as_deref() {
        let offer_nonce = hex_to_bytes32(nonce_hex)?;
        planned_fixed_hash(launcher_id, terms, receive_puzzle_hash, offer_nonce)?
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
    launcher_id: Bytes32,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    candidates: &[ReusablePresplitMakerRow],
) -> SignerResult<EnsureSelection> {
    let mut reclaim_idx = None;
    for (idx, candidate) in candidates.iter().enumerate() {
        match candidate_decision(backend, launcher_id, terms, receive_puzzle_hash, candidate)
            .await?
        {
            Some(EnsureCandidateDecision::PreferExisting) => {
                return Ok(EnsureSelection::PreferExisting(idx));
            }
            Some(EnsureCandidateDecision::ReclaimThenNew) if reclaim_idx.is_none() => {
                reclaim_idx = Some(idx);
            }
            _ => {}
        }
    }
    Ok(reclaim_idx.map_or(EnsureSelection::New, EnsureSelection::ReclaimThenNew))
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
        return Ok(EnsureSizeResult::skipped());
    }
    supersede_listing_for_repost_synced(
        write_store,
        market_id,
        &candidate.offer_id,
        parts.run.dry_run,
    )?;
    Ok(EnsureSizeResult {
        outcome: EnsureSizeOutcome::PostedExisting,
        superseded_offer_id: Some(candidate.offer_id.clone()),
        retained_maker_coin_id: Some(candidate.cancel_input_coin_id.clone()),
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
    Ok(EnsureSizeResult {
        outcome: outcome_after_reclaim_then_post(posted),
        superseded_offer_id: Some(candidate.offer_id.clone()),
        retained_maker_coin_id: None,
    })
}

fn outcome_after_reclaim_then_post(posted: bool) -> EnsureSizeOutcome {
    if posted {
        EnsureSizeOutcome::ReclaimedThenPostedNew
    } else {
        // Coin already reclaimed; do not report Skipped.
        EnsureSizeOutcome::ReclaimedOnly
    }
}

/// Prefer an existing matching maker for size N; reclaim+rebuild on hash mismatch; else `PresplitNew`.
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

    let terms = plan_offer_terms_for_market(
        &signer,
        ticker_index,
        &parts.network,
        market,
        parts.size_base_units,
        &side,
    )
    .await?;
    let receive_puzzle_hash = decode_address(&terms.receive_address)?;
    let vault_ctx = resolve_vault_spend_context(signer.clone()).await?;
    let coinset_client = client_for_signer_on_network(&signer, &parts.network)?;
    let backend = LiveCoinset(&coinset_client);

    let market_id = market.market_id.clone();
    let candidates = match candidates {
        Some(rows) => rows,
        None => write_store.sync(|store| {
            store.list_reusable_presplit_makers_for_size(&market_id, size_i64, Some(&side))
        })?,
    };
    let selection = select_ensure_candidate(
        &backend,
        vault_ctx.launcher_id,
        &terms,
        receive_puzzle_hash,
        &candidates,
    )
    .await?;

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
                EnsureSizeResult {
                    outcome: EnsureSizeOutcome::PostedNew,
                    superseded_offer_id: None,
                    retained_maker_coin_id: None,
                }
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
) -> SignerResult<EnsureSizeResult> {
    let coinset_client = client_for_signer_on_network(&signer, network)?;
    let backend = LiveCoinset(&coinset_client);
    if !coin_unspent(&backend, &row.cancel_input_coin_id).await? {
        return Ok(EnsureSizeResult::skipped());
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
    Ok(EnsureSizeResult {
        outcome: EnsureSizeOutcome::ReclaimedOnly,
        superseded_offer_id: Some(row.offer_id.clone()),
        retained_maker_coin_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::bake_expiry_into_conditions_for_quote;

    #[test]
    fn stable_quote_does_not_bake_expiry() {
        assert!(!bake_expiry_into_conditions_for_quote("stable"));
        assert!(bake_expiry_into_conditions_for_quote("unstable"));
    }

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
    fn ensure_candidate_prefers_existing_on_hash_match() {
        assert_eq!(
            decide_ensure_candidate(true, true, "aa".repeat(32).as_str(), &"aa".repeat(32)),
            Some(EnsureCandidateDecision::PreferExisting)
        );
    }

    #[test]
    fn ensure_candidate_reclaims_on_hash_mismatch_or_missing_nonce() {
        assert_eq!(
            decide_ensure_candidate(true, true, "aa".repeat(32).as_str(), &"bb".repeat(32)),
            Some(EnsureCandidateDecision::ReclaimThenNew)
        );
        assert_eq!(
            decide_ensure_candidate(true, false, "", &"aa".repeat(32)),
            Some(EnsureCandidateDecision::ReclaimThenNew)
        );
    }

    #[test]
    fn ensure_candidate_skips_spent_makers() {
        assert_eq!(
            decide_ensure_candidate(false, true, "aa".repeat(32).as_str(), &"aa".repeat(32)),
            None
        );
    }

    #[test]
    fn select_short_circuits_on_prefer_even_after_reclaim_candidate() {
        let decisions = [
            Some(EnsureCandidateDecision::ReclaimThenNew),
            Some(EnsureCandidateDecision::PreferExisting),
            Some(EnsureCandidateDecision::ReclaimThenNew),
        ];
        assert_eq!(
            select_from_decisions(&decisions),
            EnsureSelection::PreferExisting(1)
        );
    }

    #[test]
    fn select_uses_first_reclaim_when_no_prefer() {
        let decisions = [
            None,
            Some(EnsureCandidateDecision::ReclaimThenNew),
            Some(EnsureCandidateDecision::ReclaimThenNew),
        ];
        assert_eq!(
            select_from_decisions(&decisions),
            EnsureSelection::ReclaimThenNew(1)
        );
    }

    #[test]
    fn select_new_when_all_spent() {
        assert_eq!(select_from_decisions(&[None, None]), EnsureSelection::New);
    }

    #[test]
    fn protect_maker_retains_prefer_existing_coin() {
        let result = EnsureSizeResult {
            outcome: EnsureSizeOutcome::PostedExisting,
            superseded_offer_id: Some("offer-a".into()),
            retained_maker_coin_id: Some("coin-a".into()),
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
            outcome_after_reclaim_then_post(false),
            EnsureSizeOutcome::ReclaimedOnly
        );
        assert_eq!(
            outcome_after_reclaim_then_post(true),
            EnsureSizeOutcome::ReclaimedThenPostedNew
        );
        assert!(EnsureSizeResult {
            outcome: EnsureSizeOutcome::ReclaimedOnly,
            superseded_offer_id: Some("x".into()),
            retained_maker_coin_id: None,
        }
        .posted());
    }
}
