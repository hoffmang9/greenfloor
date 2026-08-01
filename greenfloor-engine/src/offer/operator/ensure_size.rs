//! Ensure a size-N offer exists: prefer matching idle maker (`PresplitExisting`), else reclaim+new, else `PresplitNew`.

use chia_protocol::Bytes32;
use tracing::{info, warn};

use crate::bech32m::decode_address;
use crate::coinset::{client_for_signer_on_network, LiveCoinset, OfferCoinsetBackend};
use crate::config::{
    bake_expiry_into_conditions_for_quote, market_wants_ladder_size, ManagerProgramConfig,
    MarketConfig, SignerConfig,
};
use crate::error::{SignerError, SignerResult};
use crate::hex::{hex_to_bytes32, normalize_hex_id, tree_hash_to_hex};
use crate::offer::action::expires_at_unix_from_pricing;
use crate::offer::assets::OfferAssetResolver;
use crate::offer::presplit::PresplitOfferBinding;
use crate::offer::reclaim::reclaim_presplit_maker_coin;
use crate::offer::request::{compute_signer_offer_leg_amounts, normalize_offer_side};
use crate::offer::types::{OfferTerms, PresplitMakerReuse};
use crate::paths::resolve_cats_config_path;
use crate::storage::{upsert_offer_post_record, CycleWriteStore, ReusablePresplitMakerRow};
use crate::vault::session::resolve_vault_spend_context;

use super::{
    build_and_post_offer_with_persist_artifacts, flush_build_and_post_persist,
    BuildAndPostOfferRequest, BuildAndPostOfferRequestParts, BuildAndPostRunOptions,
    BuildAndPostVenueOptions,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureSizeOutcome {
    PostedExisting,
    PostedNew,
    ReclaimedThenPostedNew,
    ReclaimedOnly,
    Skipped,
}

/// Pure selection among reusable makers before Coinset/post I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnsureCandidateDecision {
    PreferExisting,
    ReclaimThenNew,
    Continue,
}

fn decide_ensure_candidate(
    unspent: bool,
    has_offer_nonce: bool,
    planned_hash: &str,
    stored_hash: &str,
) -> EnsureCandidateDecision {
    if !unspent {
        return EnsureCandidateDecision::Continue;
    }
    if !has_offer_nonce {
        return EnsureCandidateDecision::ReclaimThenNew;
    }
    if normalize_hex_id(planned_hash) == normalize_hex_id(stored_hash) {
        EnsureCandidateDecision::PreferExisting
    } else {
        EnsureCandidateDecision::ReclaimThenNew
    }
}

#[derive(Debug, Clone)]
pub struct EnsureSizeConfigPaths {
    pub program_path: std::path::PathBuf,
    pub markets_path: std::path::PathBuf,
    pub testnet_markets_path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone)]
pub struct EnsureSizeNOfferRequest {
    pub program_path: std::path::PathBuf,
    pub markets_path: std::path::PathBuf,
    pub testnet_markets_path: Option<std::path::PathBuf>,
    pub cats_path: Option<std::path::PathBuf>,
    pub network: String,
    pub market: MarketConfig,
    pub size_base_units: u64,
    pub side: String,
    pub publish_venue: Option<String>,
    pub dexie_base_url: Option<String>,
    pub splash_base_url: Option<String>,
    pub dry_run: bool,
    pub persist_results: bool,
}

impl EnsureSizeNOfferRequest {
    /// Build an ensure request from program venue settings + config paths.
    #[must_use]
    pub fn from_program(
        paths: EnsureSizeConfigPaths,
        program: &ManagerProgramConfig,
        network: impl Into<String>,
        market: MarketConfig,
        size_base_units: u64,
        side: impl Into<String>,
    ) -> Self {
        Self {
            cats_path: Some(resolve_cats_config_path(&paths.markets_path, None)),
            program_path: paths.program_path,
            markets_path: paths.markets_path,
            testnet_markets_path: paths.testnet_markets_path,
            network: network.into(),
            market,
            size_base_units,
            side: side.into(),
            publish_venue: Some(program.offer_publish_venue.clone()),
            dexie_base_url: Some(program.dexie_api_base.clone()),
            splash_base_url: Some(program.splash_api_base.clone()),
            dry_run: program.runtime_dry_run,
            persist_results: true,
        }
    }
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

async fn plan_terms_for_size(
    signer: &SignerConfig,
    ticker_index: &crate::config::CatTickerIndex,
    network: &str,
    market: &MarketConfig,
    size_base_units: u64,
    side: &str,
) -> SignerResult<OfferTerms> {
    let resolver = OfferAssetResolver::new(signer, ticker_index, network);
    let (base_id, quote_id) = resolver
        .resolve_pair(&market.base_asset, &market.quote_asset)
        .await?;
    let quote_price =
        crate::offer::build_context::resolve_quote_price_for_pricing(&market.pricing)?;
    let size_i64 = i64::try_from(size_base_units).map_err(|_| SignerError::InvalidSizeBaseUnits)?;
    let leg = compute_signer_offer_leg_amounts(
        size_i64,
        quote_price,
        &base_id,
        &quote_id,
        side,
        &market.pricing,
    )?;
    let expires_at = expires_at_unix_from_pricing(&market.pricing)?;
    Ok(OfferTerms {
        receive_address: market.receive_address.clone(),
        offer_asset_id: leg.offer_asset_id,
        offer_amount: leg.offer_amount_mojos,
        request_asset_id: leg.request_asset_id,
        request_amount: leg.request_amount_mojos,
        expires_at: Some(expires_at),
        bake_expiry_into_conditions: bake_expiry_into_conditions_for_quote(
            &market.quote_asset_type,
        ),
    })
}

async fn coin_unspent<C: OfferCoinsetBackend>(backend: &C, coin_id: &str) -> SignerResult<bool> {
    let id = hex_to_bytes32(coin_id)?;
    match backend.fetch_offer_input_cat(id).await {
        Ok(_) => Ok(true),
        Err(SignerError::PresplitCoinNotFound) => Ok(false),
        Err(err) => Err(err),
    }
}

fn post_request_parts(
    request: &EnsureSizeNOfferRequest,
    reuse: Option<&ReusablePresplitMakerRow>,
) -> BuildAndPostOfferRequestParts {
    let maker_reuse = reuse.and_then(|row| {
        let nonce = row.offer_nonce.as_deref()?.trim();
        if nonce.is_empty() {
            return None;
        }
        Some(PresplitMakerReuse {
            coin_id: row.cancel_input_coin_id.clone(),
            offer_nonce: nonce.to_string(),
        })
    });
    BuildAndPostOfferRequestParts {
        program_path: request.program_path.clone(),
        markets_path: request.markets_path.clone(),
        testnet_markets_path: request.testnet_markets_path.clone(),
        cats_path: request.cats_path.clone(),
        network: request.network.clone(),
        market_id: Some(request.market.market_id.clone()),
        pair: None,
        size_base_units: request.size_base_units,
        repeat: 1,
        publish_venue: request.publish_venue.clone(),
        dexie_base_url: request.dexie_base_url.clone(),
        splash_base_url: request.splash_base_url.clone(),
        venue: BuildAndPostVenueOptions {
            drop_only: true,
            claim_rewards: false,
        },
        run: BuildAndPostRunOptions {
            dry_run: request.dry_run,
            persist_results: request.persist_results,
        },
        action_side: Some(normalize_offer_side(&request.side).to_string()),
        maker_reuse,
    }
}

async fn post_offer(
    request: &EnsureSizeNOfferRequest,
    write_store: &CycleWriteStore,
    reuse: Option<&ReusablePresplitMakerRow>,
) -> SignerResult<bool> {
    let parts = post_request_parts(request, reuse);
    let post_request = BuildAndPostOfferRequest::from_parts(parts);
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

fn retire_superseded_offer(
    write_store: &CycleWriteStore,
    market_id: &str,
    offer_id: &str,
) -> SignerResult<()> {
    write_store.sync(|store| {
        store.upsert_offer_state(offer_id, market_id, "cancelled", Some(5))?;
        Ok(())
    })
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
    request: EnsureSizeNOfferRequest,
) -> SignerResult<EnsureSizeOutcome> {
    let side = normalize_offer_side(&request.side).to_string();
    let size_i64 =
        i64::try_from(request.size_base_units).map_err(|_| SignerError::InvalidSizeBaseUnits)?;
    if !market_wants_ladder_size(&request.market, &side, size_i64) {
        return Ok(EnsureSizeOutcome::Skipped);
    }

    let terms = plan_terms_for_size(
        &signer,
        ticker_index,
        &request.network,
        &request.market,
        request.size_base_units,
        &side,
    )
    .await?;
    let receive_puzzle_hash = decode_address(&terms.receive_address)?;
    let vault_ctx = resolve_vault_spend_context(signer.clone()).await?;
    let coinset_client = client_for_signer_on_network(&signer, &request.network)?;
    let backend = LiveCoinset(&coinset_client);

    let market_id = request.market.market_id.clone();
    let candidates = write_store.sync(|store| {
        store.list_reusable_presplit_makers_for_size(&market_id, size_i64, Some(&side))
    })?;

    for candidate in &candidates {
        let unspent = coin_unspent(&backend, &candidate.cancel_input_coin_id).await?;
        let has_nonce = candidate.offer_nonce.is_some();
        let planned = if let Some(nonce_hex) = candidate.offer_nonce.as_deref() {
            let offer_nonce = hex_to_bytes32(nonce_hex)?;
            planned_fixed_hash(
                vault_ctx.launcher_id,
                &terms,
                receive_puzzle_hash,
                offer_nonce,
            )?
        } else {
            String::new()
        };
        let stored = normalize_hex_id(&candidate.fixed_delegated_puzzle_hash);
        match decide_ensure_candidate(unspent, has_nonce, &planned, &stored) {
            EnsureCandidateDecision::Continue => {}
            EnsureCandidateDecision::PreferExisting => {
                info!(
                    offer_id = %candidate.offer_id,
                    size = request.size_base_units,
                    "ensure_size_n_offer: PresplitExisting re-offer"
                );
                let posted = post_offer(&request, write_store, Some(candidate)).await?;
                if posted {
                    retire_superseded_offer(write_store, &market_id, &candidate.offer_id)?;
                    return Ok(EnsureSizeOutcome::PostedExisting);
                }
                return Ok(EnsureSizeOutcome::Skipped);
            }
            EnsureCandidateDecision::ReclaimThenNew => {
                if has_nonce {
                    info!(
                        offer_id = %candidate.offer_id,
                        size = request.size_base_units,
                        "ensure_size_n_offer: CONDITIONS hash mismatch; reclaim then PresplitNew"
                    );
                } else {
                    warn!(
                        offer_id = %candidate.offer_id,
                        "presplit maker missing offer_nonce; reclaiming for rebuild"
                    );
                }
                reclaim_presplit_maker_coin(
                    signer.clone(),
                    &request.network,
                    &candidate.cancel_input_coin_id,
                    &candidate.fixed_delegated_puzzle_hash,
                    request.dry_run,
                )
                .await?;
                retire_superseded_offer(write_store, &market_id, &candidate.offer_id)?;
                let posted = post_offer(&request, write_store, None).await?;
                return Ok(if posted {
                    EnsureSizeOutcome::ReclaimedThenPostedNew
                } else {
                    EnsureSizeOutcome::Skipped
                });
            }
        }
    }

    let posted = post_offer(&request, write_store, None).await?;
    Ok(if posted {
        EnsureSizeOutcome::PostedNew
    } else {
        EnsureSizeOutcome::Skipped
    })
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
) -> SignerResult<EnsureSizeOutcome> {
    let coinset_client = client_for_signer_on_network(&signer, network)?;
    let backend = LiveCoinset(&coinset_client);
    if !coin_unspent(&backend, &row.cancel_input_coin_id).await? {
        return Ok(EnsureSizeOutcome::Skipped);
    }
    reclaim_presplit_maker_coin(
        signer,
        network,
        &row.cancel_input_coin_id,
        &row.fixed_delegated_puzzle_hash,
        dry_run,
    )
    .await?;
    retire_superseded_offer(write_store, &row.market_id, &row.offer_id)?;
    Ok(EnsureSizeOutcome::ReclaimedOnly)
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
            EnsureCandidateDecision::PreferExisting
        );
    }

    #[test]
    fn ensure_candidate_reclaims_on_hash_mismatch_or_missing_nonce() {
        assert_eq!(
            decide_ensure_candidate(true, true, "aa".repeat(32).as_str(), &"bb".repeat(32)),
            EnsureCandidateDecision::ReclaimThenNew
        );
        assert_eq!(
            decide_ensure_candidate(true, false, "", &"aa".repeat(32)),
            EnsureCandidateDecision::ReclaimThenNew
        );
    }

    #[test]
    fn ensure_candidate_skips_spent_makers() {
        assert_eq!(
            decide_ensure_candidate(false, true, "aa".repeat(32).as_str(), &"aa".repeat(32)),
            EnsureCandidateDecision::Continue
        );
    }
}
