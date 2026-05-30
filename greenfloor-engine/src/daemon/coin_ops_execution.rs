use std::collections::{HashMap, HashSet};
use std::path::Path;

use chia_protocol::Bytes32;
use serde_json::{json, Value};

use crate::coin_ops::{
    coin_op_min_amount_mojos, coin_op_target_amount_allowed, plan_auto_combine_inputs,
    plan_auto_split_selection, CoinOpPlan, CombineInputSelectionMode, SpendableCoin,
    SplitAutoSelectPlan, SplitCombinePrereqPlan, SplitPlanningProfile,
};
use crate::coinset::{list_wallet_unspent_coins, spend_bundle_hash_from_hex, WalletUnspentCoin};
use crate::config::{
    load_signer_config, require_signer_offer_path, MarketConfig, ManagerProgramConfig,
    SignerConfig,
};
use crate::error::SignerResult;
use crate::hex::default_mojo_multiplier_for_asset;
use crate::offer::resolve_offer_assets_for_action;
use crate::storage::SqliteStore;
use crate::vault::{
    build_and_optionally_broadcast_vault_cat_mixed_split, members::hex_to_bytes32, MixedSplitRequest,
};

use super::coinset_tx::extract_coin_ids_from_offer_payload;

const COIN_OP_ERROR_PREFIX: &str = "signer_coin_op_error";

#[derive(Debug, Clone)]
pub struct CoinOpExecItem {
    pub op_type: String,
    pub size_base_units: i64,
    pub op_count: i64,
    pub status: String,
    pub reason: String,
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CoinOpExecutionResult {
    pub dry_run: bool,
    pub planned_count: usize,
    pub executed_count: u64,
    pub status: String,
    pub items: Vec<CoinOpExecItem>,
    pub signer_selection: Value,
}

struct CoinOpExecContext {
    signer_config: SignerConfig,
    market: MarketConfig,
    program: ManagerProgramConfig,
    resolved_base_asset_id: String,
    base_unit_mojo_multiplier: i64,
    combine_input_cap: i64,
    watched_coin_ids: HashSet<String>,
}

pub fn combine_input_coin_cap() -> i64 {
    std::env::var("GREENFLOOR_COIN_OPS_COMBINE_INPUT_COIN_CAP")
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .map(|value| value.max(2))
        .unwrap_or(5)
}

fn watchlist_offer_ids(store: &SqliteStore, market_id: &str) -> SignerResult<HashSet<String>> {
    let tracked_states: HashSet<&str> = ["open", "refresh_due", "unknown_orphaned"]
        .into_iter()
        .collect();
    let mut offer_ids = HashSet::new();
    for row in store.list_offer_state_details(market_id, 500)? {
        let state = row.state.trim().to_ascii_lowercase();
        if tracked_states.contains(state.as_str()) || state == "mempool_observed" {
            offer_ids.insert(row.offer_id);
        }
    }
    Ok(offer_ids)
}

pub fn watched_coin_ids_for_market(
    store: &SqliteStore,
    market_id: &str,
    offers: &[Value],
) -> SignerResult<HashSet<String>> {
    let watch_offer_ids = watchlist_offer_ids(store, market_id)?;
    let mut watched = HashSet::new();
    for offer in offers {
        let offer_id = offer
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if offer_id.is_empty() || !watch_offer_ids.contains(offer_id) {
            continue;
        }
        for coin_id in extract_coin_ids_from_offer_payload(offer) {
            watched.insert(coin_id);
        }
    }
    Ok(watched)
}

fn wallet_coins_to_spendable(
    coins: &[WalletUnspentCoin],
    canonical_asset_id: &str,
) -> Vec<SpendableCoin> {
    let min_amount = coin_op_min_amount_mojos(canonical_asset_id);
    coins
        .iter()
        .filter(|coin| i64::try_from(coin.amount).unwrap_or(0) >= min_amount)
        .map(|coin| SpendableCoin {
            id: coin.id.clone(),
            amount: i64::try_from(coin.amount).unwrap_or(i64::MAX),
        })
        .collect()
}

impl CoinOpExecContext {
    async fn list_spendable_coins(&self) -> SignerResult<Vec<SpendableCoin>> {
        let coins = list_wallet_unspent_coins(
            &self.program.network,
            &self.market.receive_address,
            &self.resolved_base_asset_id,
        )
        .await?;
        Ok(wallet_coins_to_spendable(
            &coins,
            self.market.base_asset.trim(),
        ))
    }

    async fn execute_mixed_split(
        &self,
        output_amounts: Vec<u64>,
        coin_ids: &[String],
        fee_mojos: u64,
    ) -> SignerResult<String> {
        let asset_id = hex_to_bytes32(&self.resolved_base_asset_id)?;
        let parsed_coin_ids: Vec<Bytes32> = coin_ids
            .iter()
            .map(|coin_id| hex_to_bytes32(coin_id))
            .collect::<SignerResult<Vec<_>>>()?;
        let request = MixedSplitRequest {
            receive_address: self.market.receive_address.clone(),
            asset_id,
            output_amounts,
            coin_ids: parsed_coin_ids,
            allow_sub_cat_output: false,
            fee_mojos,
        };
        let result = build_and_optionally_broadcast_vault_cat_mixed_split(
            self.signer_config.clone(),
            request,
            true,
        )
        .await?;
        spend_bundle_hash_from_hex(&result.spend_bundle_hex)
    }
}

fn skip_item(
    op_type: &str,
    size_base_units: i64,
    op_count: i64,
    reason: impl Into<String>,
) -> CoinOpExecItem {
    CoinOpExecItem {
        op_type: op_type.to_string(),
        size_base_units,
        op_count,
        status: "skipped".to_string(),
        reason: reason.into(),
        operation_id: None,
    }
}

fn executed_item(
    op_type: &str,
    size_base_units: i64,
    op_count: i64,
    reason: impl Into<String>,
    operation_id: String,
) -> CoinOpExecItem {
    CoinOpExecItem {
        op_type: op_type.to_string(),
        size_base_units,
        op_count,
        status: "executed".to_string(),
        reason: reason.into(),
        operation_id: Some(operation_id),
    }
}

fn combine_output_amounts(total: i64, output_count: usize) -> Vec<u64> {
    let output_count = output_count.max(1);
    let base = total.div_euclid(output_count as i64);
    let remainder = total.rem_euclid(output_count as i64);
    let mut output_amounts = vec![base.max(0) as u64; output_count];
    if let Some(last) = output_amounts.last_mut() {
        *last = last.saturating_add(remainder.max(0) as u64);
    }
    output_amounts
}

fn total_for_coin_ids(spendable: &[SpendableCoin], coin_ids: &[String]) -> i64 {
    let amount_by_id: HashMap<String, i64> = spendable
        .iter()
        .map(|coin| (coin.id.to_ascii_lowercase(), coin.amount))
        .collect();
    coin_ids
        .iter()
        .map(|coin_id| {
            amount_by_id
                .get(&coin_id.to_ascii_lowercase())
                .copied()
                .unwrap_or(0)
        })
        .sum()
}

async fn submit_combine_prereq_for_split(
    ctx: &CoinOpExecContext,
    op_type: &str,
    size_base_units: i64,
    op_count: i64,
    _required_amount: i64,
    prereq: &SplitCombinePrereqPlan,
) -> (Vec<CoinOpExecItem>, u64) {
    let combine_count = prereq.input_coin_ids.len() as i64;
    let spendable = match ctx.list_spendable_coins().await {
        Ok(coins) => coins,
        Err(err) => {
            return (
                vec![skip_item(
                    op_type,
                    size_base_units,
                    op_count,
                    format!("{COIN_OP_ERROR_PREFIX}:{err}:combine_for_split_prereq"),
                )],
                0,
            );
        }
    };
    let total = total_for_coin_ids(&spendable, &prereq.input_coin_ids);
    let output_amounts =
        combine_output_amounts(total, prereq.input_coin_ids.len());
    match ctx
        .execute_mixed_split(
            output_amounts,
            &prereq.input_coin_ids,
            ctx.program.coin_ops_combine_fee_mojos.max(0) as u64,
        )
        .await
    {
        Ok(operation_id) => {
            let reason = if prereq.exact_match {
                "signer_combine_submitted_for_split_prereq_exact"
            } else {
                "signer_combine_submitted_for_split_prereq_with_change"
            };
            (
                vec![executed_item(
                    "combine",
                    size_base_units,
                    combine_count,
                    reason,
                    operation_id,
                )],
                1,
            )
        }
        Err(err) => (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                format!("{COIN_OP_ERROR_PREFIX}:{err}:combine_for_split_prereq"),
            )],
            0,
        ),
    }
}

async fn execute_daemon_split_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> (Vec<CoinOpExecItem>, u64) {
    let op_type = plan.op_type.as_str();
    let op_count = plan.op_count;
    let size_base_units = plan.size_base_units;

    if op_count == 1 {
        return (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "split_single_coin_noop_skipped",
            )],
            0,
        );
    }

    let amount_per_coin_mojos = size_base_units.saturating_mul(ctx.base_unit_mojo_multiplier);
    let canonical_asset_id = ctx.market.base_asset.trim();
    if !coin_op_target_amount_allowed(amount_per_coin_mojos, canonical_asset_id) {
        return (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "split_amount_below_coin_op_minimum",
            )],
            0,
        );
    }

    let required_amount = amount_per_coin_mojos.saturating_mul(op_count);
    let initial = match ctx.list_spendable_coins().await {
        Ok(coins) => coins,
        Err(err) => {
            return (
                vec![skip_item(
                    op_type,
                    size_base_units,
                    op_count,
                    format!("{COIN_OP_ERROR_PREFIX}:{err}"),
                )],
                0,
            );
        }
    };
    if initial.is_empty() {
        return (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "no_spendable_split_coin_available",
            )],
            0,
        );
    }

    let mut attempted_coin_ids = HashSet::new();
    for attempt_index in 0..2 {
        let fresh = match ctx.list_spendable_coins().await {
            Ok(coins) => coins,
            Err(err) => {
                return (
                    vec![skip_item(
                        op_type,
                        size_base_units,
                        op_count,
                        format!("{COIN_OP_ERROR_PREFIX}:{err}"),
                    )],
                    0,
                );
            }
        };
        let candidate_spendable: Vec<SpendableCoin> = fresh
            .into_iter()
            .filter(|coin| {
                !attempted_coin_ids.contains(&coin.id)
                    && !ctx.watched_coin_ids.contains(&coin.id.to_ascii_lowercase())
            })
            .collect();

        let selection = plan_auto_split_selection(
            &candidate_spendable,
            required_amount,
            canonical_asset_id,
            SplitPlanningProfile::DaemonAuto,
            ctx.combine_input_cap,
            Some(attempt_index == 0),
        );

        match selection {
            SplitAutoSelectPlan::CombinePrereq(prereq) => {
                return submit_combine_prereq_for_split(
                    ctx,
                    op_type,
                    size_base_units,
                    op_count,
                    required_amount,
                    &prereq,
                )
                .await;
            }
            SplitAutoSelectPlan::Skip(skip) => {
                if skip.reason == "no_spendable_split_coin_meets_required_amount" {
                    break;
                }
                return (vec![skip_item(op_type, size_base_units, op_count, skip.reason)], 0);
            }
            SplitAutoSelectPlan::Coin(selected) => {
                attempted_coin_ids.insert(selected.coin_id.clone());
                let output_amounts = vec![amount_per_coin_mojos.max(0) as u64; op_count as usize];
                match ctx
                    .execute_mixed_split(
                        output_amounts,
                        std::slice::from_ref(&selected.coin_id),
                        ctx.program.coin_ops_split_fee_mojos.max(0) as u64,
                    )
                    .await
                {
                    Ok(operation_id) => {
                        return (
                            vec![executed_item(
                                op_type,
                                size_base_units,
                                op_count,
                                "signer_split_submitted",
                                operation_id,
                            )],
                            1,
                        );
                    }
                    Err(err) => {
                        let error_text = err.to_string();
                        if error_text.contains("Some selected coins are not spendable")
                            && attempt_index == 0
                        {
                            continue;
                        }
                        return (
                            vec![skip_item(
                                op_type,
                                size_base_units,
                                op_count,
                                format!(
                                    "{COIN_OP_ERROR_PREFIX}:{err}:selected_coin_id={}",
                                    selected.coin_id
                                ),
                            )],
                            0,
                        );
                    }
                }
            }
        }
    }

    (
        vec![skip_item(
            op_type,
            size_base_units,
            op_count,
            "no_spendable_split_coin_meets_required_amount",
        )],
        0,
    )
}

async fn execute_daemon_combine_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> (Vec<CoinOpExecItem>, u64) {
    let op_type = plan.op_type.as_str();
    let op_count = plan.op_count;
    let size_base_units = plan.size_base_units;
    let requested_number_of_coins = op_count.max(2);
    let capped_number_of_coins = requested_number_of_coins.min(ctx.combine_input_cap);
    let target_coin_amount_mojos = size_base_units.saturating_mul(ctx.base_unit_mojo_multiplier);
    let canonical_asset_id = ctx.market.base_asset.trim();

    if !coin_op_target_amount_allowed(target_coin_amount_mojos, canonical_asset_id) {
        return (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "combine_target_amount_below_coin_op_minimum",
            )],
            0,
        );
    }

    let spendable = match ctx.list_spendable_coins().await {
        Ok(coins) => coins,
        Err(err) => {
            return (
                vec![skip_item(
                    op_type,
                    size_base_units,
                    op_count,
                    format!("{COIN_OP_ERROR_PREFIX}:{err}"),
                )],
                0,
            );
        }
    };

    let combine_input_coin_ids = match plan_auto_combine_inputs(
        &spendable,
        requested_number_of_coins as usize,
        CombineInputSelectionMode::ExactAmount,
        Some(target_coin_amount_mojos),
        Some(&ctx.watched_coin_ids),
        Some(capped_number_of_coins as usize),
    ) {
        Ok(ids) => ids,
        Err(reason) => {
            return (
                vec![skip_item(op_type, size_base_units, op_count, reason)],
                0,
            );
        }
    };
    if combine_input_coin_ids.len() < 2 {
        return (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "no_spendable_combine_coin_available",
            )],
            0,
        );
    }

    let total = total_for_coin_ids(&spendable, &combine_input_coin_ids);
    let output_amounts = combine_output_amounts(total, combine_input_coin_ids.len());

    match ctx
        .execute_mixed_split(
            output_amounts,
            &combine_input_coin_ids,
            ctx.program.coin_ops_combine_fee_mojos.max(0) as u64,
        )
        .await
    {
        Ok(operation_id) => (
            vec![executed_item(
                op_type,
                size_base_units,
                op_count,
                "signer_combine_submitted",
                operation_id,
            )],
            1,
        ),
        Err(err) => (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                format!("coin_op_error:{err}"),
            )],
            0,
        ),
    }
}

fn skip_all_plans(
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    plans: &[CoinOpPlan],
    reason: &str,
    status: &str,
) -> CoinOpExecutionResult {
    CoinOpExecutionResult {
        dry_run: program.runtime_dry_run,
        planned_count: plans.len(),
        executed_count: 0,
        status: status.to_string(),
        items: plans
            .iter()
            .map(|plan| skip_item(plan.op_type.as_str(), plan.size_base_units, plan.op_count, reason))
            .collect(),
        signer_selection: json!({
            "selected_source": "signer_registry",
            "key_id": market.signer_key_id,
            "network": program.network,
        }),
    }
}

pub async fn execute_managed_coin_op_plans(
    program_path: &Path,
    market: &MarketConfig,
    program: &ManagerProgramConfig,
    plans: &[CoinOpPlan],
    watched_coin_ids: &HashSet<String>,
) -> CoinOpExecutionResult {
    if let Err(err) = require_signer_offer_path(program_path) {
        return skip_all_plans(
            program,
            market,
            plans,
            &err.to_string(),
            "skipped",
        );
    }
    if market.receive_address.trim().is_empty() {
        return skip_all_plans(
            program,
            market,
            plans,
            "signer_coin_ops_missing_receive_address",
            "skipped",
        );
    }

    let signer_config = match load_signer_config(program_path) {
        Ok(config) => config,
        Err(err) => {
            return skip_all_plans(program, market, plans, &err.to_string(), "skipped");
        }
    };
    let (resolved_base_asset_id, _) = match resolve_offer_assets_for_action(
        &signer_config,
        market.base_asset.trim(),
        "xch",
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(err) => {
            return skip_all_plans(program, market, plans, &err.to_string(), "skipped");
        }
    };

    let ctx = CoinOpExecContext {
        signer_config,
        market: market.clone(),
        program: program.clone(),
        resolved_base_asset_id,
        base_unit_mojo_multiplier: default_mojo_multiplier_for_asset(market.base_asset.trim()) as i64,
        combine_input_cap: combine_input_coin_cap(),
        watched_coin_ids: watched_coin_ids.clone(),
    };

    let mut items = Vec::new();
    let mut executed_count = 0_u64;
    for plan in plans {
        if plan.op_count <= 0 || plan.size_base_units <= 0 {
            items.push(skip_item(
                plan.op_type.as_str(),
                plan.size_base_units,
                plan.op_count,
                "invalid_plan",
            ));
            continue;
        }
        if program.runtime_dry_run {
            items.push(CoinOpExecItem {
                op_type: plan.op_type.as_str().to_string(),
                size_base_units: plan.size_base_units,
                op_count: plan.op_count,
                status: "planned".to_string(),
                reason: "dry_run:signer".to_string(),
                operation_id: None,
            });
            continue;
        }
        let (plan_items, plan_executed) = match plan.op_type {
            crate::coin_ops::CoinOpKind::Split => execute_daemon_split_plan(&ctx, plan).await,
            crate::coin_ops::CoinOpKind::Combine => execute_daemon_combine_plan(&ctx, plan).await,
        };
        items.extend(plan_items);
        executed_count += plan_executed;
    }

    CoinOpExecutionResult {
        dry_run: program.runtime_dry_run,
        planned_count: plans.len(),
        executed_count,
        status: "signer".to_string(),
        items,
        signer_selection: json!({
            "selected_source": "signer_registry",
            "key_id": market.signer_key_id,
            "network": program.network,
        }),
    }
}

pub fn persist_coin_op_execution(
    store: &SqliteStore,
    market: &MarketConfig,
    program: &ManagerProgramConfig,
    execution: &CoinOpExecutionResult,
) -> SignerResult<()> {
    for item in &execution.items {
        let fee_mojos = if item.status == "executed" {
            let per_op_fee = if item.op_type == "split" {
                program.coin_ops_split_fee_mojos
            } else {
                program.coin_ops_combine_fee_mojos
            };
            per_op_fee.saturating_mul(item.op_count)
        } else {
            0
        };
        store.add_audit_event(
            &format!("coin_op_{}", item.status),
            &json!({
                "market_id": market.market_id,
                "op_type": item.op_type,
                "size_base_units": item.size_base_units,
                "op_count": item.op_count,
                "reason": item.reason,
                "operation_id": item.operation_id,
                "fee_mojos": fee_mojos,
            }),
            Some(&market.market_id),
        )?;
        store.add_coin_op_ledger_entry(
            &market.market_id,
            &item.op_type,
            item.op_count,
            fee_mojos,
            &item.status,
            &item.reason,
            item.operation_id.as_deref(),
        )?;
    }
    Ok(())
}
