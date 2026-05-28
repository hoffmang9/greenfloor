pub mod bls;
pub mod coin_ops;
pub mod coinset;
pub mod config;
pub mod cycle;
pub mod error;
pub mod kms;
pub mod offer;
pub mod vault;

use config::SignerConfig;
use error::SignerResult;

pub async fn resolve_vault_context(config: SignerConfig) -> SignerResult<vault::VaultContext> {
    Ok(vault::session::resolve_vault_session(config).await?.display)
}

pub async fn resolve_offer_asset_ids(
    config: SignerConfig,
    base_asset: &str,
    quote_asset: &str,
) -> SignerResult<(String, String)> {
    let msp = coinset::MspCoinset::for_network(&config.network, Some(&config.coinset_msp_base_url))?;
    coinset::resolve_offer_asset_ids(&msp, base_asset, quote_asset).await
}

pub use coin_ops::{
    amount_meets_coin_op_min_mojos, coin_op_min_amount_mojos, coin_op_target_amount_allowed,
    compute_bucket_counts_from_coins, fee_budget_allows_execution, partition_plans_by_budget,
    plan_coin_ops, projected_coin_ops_fee_mojos, BucketSpec, CoinOpKind, CoinOpPlan,
};
pub use coinset::{
    get_conservative_fee_estimate, get_fee_estimate, parse_coin_ids, push_tx_hex,
};
pub use config::load_signer_config;
pub use error::SignerError as Error;
pub use offer::{build_vault_cat_offer, CreateOfferRequest, CreateOfferResult};
pub use bls::{
    broadcast_bls_spend_bundle, build_bls_mixed_split_spend_bundle, build_bls_offer_spend_bundle,
    build_bls_xch_coin_op_spend_bundle, list_cat_coin_summaries, list_cat_coin_summaries_by_ids,
    list_xch_coin_summaries, load_bls_master_secret_key, BlsMixedSplitRequest, BlsMixedSplitResult,
    BlsOfferRequest, BlsOfferResult, BlsXchCoinOpRequest, BlsXchCoinOpResult, CoinRecordSummary,
};
pub use offer::codec::{
    encode_offer_from_spend_bundle_bytes, from_input_spend_bundle_bytes,
    from_input_spend_bundle_xch_bytes, validate_offer_text,
};
pub use cycle::{
    apply_offer_signal, can_parallelize_managed_offers, classify_dexie_visibility_outcome,
    classify_managed_post_result, classify_managed_transient_error, evaluate_market,
    expiry_seconds_for_action, is_managed_upstream_transient_error,
    is_managed_worker_transient_error, is_parallel_dispatch_transient_error,
    is_transient_dexie_visibility_404_error, is_transient_managed_upstream_error_text,
    managed_retry_decision, parallel_max_workers, reservation_release_status,
    should_apply_parallel_transient_cooldown,
    single_input_preferred_skip_reason, classify_dexie_stale_offer_status,
    collect_stale_sweep_candidates, count_parallel_transient_failures, dedupe_sorted_market_ids,
    enqueue_immediate_requeue, is_dexie_offer_missing_error_text, next_disabled_market_log_deadline,
    record_stale_sweep_check, select_market_batch, should_log_disabled_market,
    should_try_cat_inventory_fallback, should_use_market_slot_dispatch, ManagedActionOutcome,
    ManagedActionStatus, ManagedRetryDecision, ManagedRetryDecisionKind, aggregate_two_sided_offer_counts, is_two_sided_market_mode, market_cycle_phases,
    needs_inventory_fallback, one_sided_offer_counts_by_side, resolve_inventory_scan_source,
    resolve_tracked_sizes, wallet_fallback_source_label, MarketBatchSelection, MarketCyclePhase,
    MarketCycleResultState, MarketState, OfferLifecycleState, OfferSignal, OfferStateRow,
    OfferTransition, ParallelSubmissionDecision, PlannedAction, PlannedActionInput,
    SpendableAssetProfile, StaleSweepCandidate, StaleSweepHit, StaleSweepProgress,
    StrategyConfig, evaluate_two_sided_market_actions, expand_planned_actions,
    plan_reseed_actions_from_gap, reseed_skip_reason_labels, ReseedGapPlan, ReseedSkipReason,
    filter_planned_actions_with_positive_repeat, plan_parallel_managed_dispatch,
    resolve_missing_watched_offer_transition, resolve_watched_offer_transition_from_signals,
    sequential_action_route, unchanged_offer_transition, unsupported_venue_offer_transition,
    CycleOfferTransition, ParallelBatchPlan, ParallelQueueItem, ParallelReservationContext,
    ParallelSkipItem, SequentialActionRoute,
};
pub use vault::{
    build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest, MixedSplitResult,
};

#[cfg(test)]
mod test_support;
