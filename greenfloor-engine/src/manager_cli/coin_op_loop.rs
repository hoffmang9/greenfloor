//! Coin split/combine CLI iteration loops using canonical gate policy.

use std::collections::HashSet;
use std::path::Path;

use serde_json::{json, Value};

use crate::coin_ops::{
    coin_op_should_stop, combine_output_amounts, evaluate_coin_combine_gate,
    evaluate_coin_split_gate, plan_auto_combine_inputs, plan_auto_split_selection,
    total_for_coin_ids, CoinCombineGateResult, CoinSplitGateResult, CombineInputSelectionMode,
    SpendableCoin, SplitAutoSelectPlan, SplitCombinePrereqPlan, SplitPlanningProfile,
};
use crate::config::{
    load_markets_config_with_overlay, load_program_config, load_signer_config,
    require_signer_offer_path, resolve_market_for_build, MarketConfig,
};
use crate::daemon::{combine_input_coin_cap, CoinOpExecContext};
use crate::error::{SignerError, SignerResult};
use crate::hex::{default_mojo_multiplier_for_asset, is_hex_id, normalize_hex_id};
use crate::offer::resolve_offer_assets_for_action;

use super::json::emit_json;
use super::ladder::{
    combine_threshold_count, resolve_combine_count, resolve_split_targets, sell_ladder_entry_for_size,
    split_required_count,
};
const COIN_SPLIT_LOCKUP_ERROR: &str = "coin_split_lockup_guardrail_would_lock_all_spendable_coins";
const COIN_SPLIT_NO_SPENDABLE_ERROR: &str = "no_spendable_split_coin_available";

pub async fn run_coins_list(
    program_path: &Path,
    markets_path: &Path,
    asset: Option<&str>,
    vault_id: Option<&str>,
    cat_id: Option<&str>,
) -> SignerResult<i32> {
    let _ = vault_id;
    if let Err(err) = require_signer_offer_path(program_path) {
        emit_json(&json!({
            "ok": false,
            "error": "coin_list_requires_signer_backend",
            "detail": err.to_string(),
        }))?;
        return Ok(2);
    }
    let program = load_program_config(program_path)?;
    let markets = load_markets_config_with_overlay(markets_path, None)?;
    let market = select_list_market(&markets)?;
    let receive_address = market.receive_address.trim();
    if receive_address.is_empty() {
        return Err(SignerError::Other(
            "market missing receive_address for signer coin list".to_string(),
        ));
    }
    let filter = cat_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| asset.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string));
    let filter_label = filter.clone();
    let list_asset_id = if let Some(filter_value) = filter {
        resolve_asset_filter(program_path, &program.network, &filter_value).await?
    } else {
        market.base_asset.clone()
    };
    let coins = crate::coinset::list_wallet_unspent_coins(
        &program.network,
        receive_address,
        &list_asset_id,
    )
    .await?;
    let min_amount = crate::coin_ops::coin_op_min_amount_mojos(market.base_asset.trim());
    let items: Vec<Value> = coins
        .iter()
        .map(|coin| {
            let state = coin.state.trim().to_ascii_uppercase();
            let spendable = crate::coin_ops::is_spendable_wallet_coin(&json!({"state": state}))
                && i64::try_from(coin.amount).unwrap_or(0) >= min_amount;
            json!({
                "coin_id": coin.name,
                "amount": coin.amount,
                "state": state,
                "pending": state == "PENDING" || state == "MEMPOOL",
                "spendable": spendable,
                "asset": list_asset_id,
                "reported_asset": filter_label,
                "scoped_asset": filter_label,
            })
        })
        .collect();
    let spendable_items: Vec<_> = items
        .iter()
        .filter(|row| row.get("spendable").and_then(Value::as_bool) == Some(true))
        .collect();
    let spendable_amount: u64 = spendable_items
        .iter()
        .filter_map(|row| row.get("amount").and_then(Value::as_u64))
        .sum();
    emit_json(&json!({
        "execution_backend": "signer",
        "network": program.network,
        "market_id": market.market_id,
        "receive_address": receive_address,
        "resolved_asset_id": filter_label,
        "asset": list_asset_id,
        "coin_count": items.len(),
        "spendable_coin_count": spendable_items.len(),
        "spendable_count": spendable_items.len(),
        "spendable_amount": spendable_amount,
        "coins": items,
    }))?;
    Ok(0)
}

pub async fn run_coin_status(
    program_path: &Path,
    markets_path: &Path,
    asset: Option<&str>,
    vault_id: Option<&str>,
    cat_id: Option<&str>,
) -> SignerResult<i32> {
    run_coins_list(program_path, markets_path, asset, vault_id, cat_id).await
}

pub async fn run_coin_split(
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    coin_ids: &[String],
    amount_per_coin: i64,
    number_of_coins: i64,
    no_wait: bool,
    size_base_units: Option<i64>,
    until_ready: bool,
    max_iterations: i32,
    allow_lock_all_spendable: bool,
    force_split_when_ready: bool,
) -> SignerResult<i32> {
    if until_ready && no_wait {
        return Err(SignerError::Other(
            "until-ready mode requires wait mode (do not pass --no-wait)".to_string(),
        ));
    }
    if until_ready && size_base_units.filter(|value| *value > 0).is_none() {
        return Err(SignerError::Other(
            "until-ready mode requires --size-base-units".to_string(),
        ));
    }
    let ctx = build_coin_op_exec_context(
        program_path,
        markets_path,
        testnet_markets_path,
        network,
        market_id,
        pair,
        None,
    )
    .await?;
    let (amount_per_coin, number_of_coins) =
        resolve_split_targets(&ctx.market, amount_per_coin, number_of_coins, size_base_units)?;
    if amount_per_coin <= 0 || number_of_coins <= 0 {
        return Err(SignerError::Other(
            "amount_per_coin and number_of_coins must be positive".to_string(),
        ));
    }
    let amount_per_coin_mojos =
        amount_per_coin.saturating_mul(ctx.base_unit_mojo_multiplier);
    let required_amount = amount_per_coin_mojos.saturating_mul(number_of_coins);
    let split_target = size_base_units
        .filter(|value| *value > 0)
        .map(|size| sell_ladder_entry_for_size(&ctx.market, size))
        .transpose()?;
    let max_iterations = max_iterations.max(1);
    let mut operations = Vec::new();
    let mut stop_reason = "single_pass".to_string();
    let explicit_coin_ids = !coin_ids.is_empty();

    for iteration in 1..=max_iterations {
        let spendable = ctx.list_spendable_coins().await?;
        let gate_coins = spendable_coins_for_gate(&spendable);
        let split_gate = split_target
            .as_ref()
            .map(|entry| {
                evaluate_coin_split_gate(
                    &gate_coins,
                    &ctx.resolved_base_asset_id,
                    amount_per_coin_mojos,
                    split_required_count(entry),
                )
            });

        if let Some(ref gate) = split_gate {
            if until_ready && gate.ready && !force_split_when_ready {
                stop_reason = "ready".to_string();
                break;
            }
            let (should_stop, reason) = coin_op_should_stop(
                until_ready,
                Some(gate.ready),
                explicit_coin_ids,
                i64::from(iteration),
                i64::from(max_iterations),
            );
            if should_stop && until_ready {
                stop_reason = reason.to_string();
                break;
            }
        }

        let selected_coin_ids = if explicit_coin_ids {
            coin_ids.to_vec()
        } else {
            if spendable.is_empty() {
                emit_json(&json!({"error": COIN_SPLIT_NO_SPENDABLE_ERROR}))?;
                return Ok(2);
            }
            match plan_auto_split_selection(
                &spendable,
                required_amount,
                ctx.market.base_asset.trim(),
                SplitPlanningProfile::CliAuto,
                ctx.combine_input_cap,
                Some(iteration == 1),
            ) {
                SplitAutoSelectPlan::CombinePrereq(prereq) => {
                    let operation_id = execute_combine_prereq(&ctx, &prereq).await?;
                    operations.push(json!({
                        "iteration": iteration,
                        "op": "combine-prereq",
                        "signature_request_id": operation_id,
                        "input_coin_ids": prereq.input_coin_ids,
                        "waited": !no_wait,
                    }));
                    if no_wait {
                        stop_reason = "combine_prereq_submitted".to_string();
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    continue;
                }
                SplitAutoSelectPlan::Skip(skip) => {
                    emit_json(&json!({"error": skip.reason}))?;
                    return Ok(2);
                }
                SplitAutoSelectPlan::Coin(plan) => vec![plan.coin_id],
            }
        };

        if let Some(code) = enforce_split_lockup_guardrail(
            &spendable,
            &selected_coin_ids,
            allow_lock_all_spendable,
            &ctx.resolved_base_asset_id,
        )? {
            return Ok(code);
        }

        let operation_id = ctx
            .execute_mixed_split(
                vec![amount_per_coin_mojos.max(0) as u64; number_of_coins as usize],
                &selected_coin_ids,
                ctx.program.coin_ops_split_fee_mojos.max(0) as u64,
            )
            .await?;
        operations.push(json!({
            "iteration": iteration,
            "signature_request_id": operation_id,
            "selected_coin_ids": selected_coin_ids,
            "waited": !no_wait,
            "denomination_readiness": split_gate.as_ref().map(gate_to_json),
        }));

        let (should_stop, reason) = coin_op_should_stop(
            until_ready,
            split_gate.as_ref().map(|gate| gate.ready),
            explicit_coin_ids,
            i64::from(iteration),
            i64::from(max_iterations),
        );
        if should_stop {
            stop_reason = reason.to_string();
            break;
        }
        if no_wait {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    emit_json(&json!({
        "op": "coin-split",
        "coin_selection_mode": if explicit_coin_ids { "explicit" } else { "adapter_auto_select" },
        "amount_per_coin": amount_per_coin,
        "number_of_coins": number_of_coins,
        "resolved_asset_id": ctx.resolved_base_asset_id,
        "until_ready": until_ready,
        "max_iterations": max_iterations,
        "stop_reason": stop_reason,
        "operations": operations,
    }))?;
    Ok(if until_ready && stop_reason != "ready" { 2 } else { 0 })
}

pub async fn run_coin_combine(
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    coin_ids: &[String],
    number_of_coins: i64,
    asset_id: Option<&str>,
    no_wait: bool,
    size_base_units: Option<i64>,
    until_ready: bool,
    max_iterations: i32,
) -> SignerResult<i32> {
    if until_ready && no_wait {
        return Err(SignerError::Other(
            "until-ready mode requires wait mode (do not pass --no-wait)".to_string(),
        ));
    }
    if until_ready && size_base_units.filter(|value| *value > 0).is_none() {
        return Err(SignerError::Other(
            "until-ready mode requires --size-base-units".to_string(),
        ));
    }
    let ctx = build_coin_op_exec_context(
        program_path,
        markets_path,
        testnet_markets_path,
        network,
        market_id,
        pair,
        asset_id,
    )
    .await?;
    let number_of_coins = resolve_combine_count(&ctx.market, number_of_coins, size_base_units)?;
    if number_of_coins <= 1 {
        return Err(SignerError::Other("number_of_coins must be > 1".to_string()));
    }
    let target_coin_amount_mojos = size_base_units
        .unwrap_or(0)
        .max(0)
        .saturating_mul(ctx.base_unit_mojo_multiplier);
    let combine_target = size_base_units
        .filter(|value| *value > 0)
        .map(|size| sell_ladder_entry_for_size(&ctx.market, size))
        .transpose()?;
    let max_iterations = max_iterations.max(1);
    let mut operations = Vec::new();
    let mut stop_reason = "single_pass".to_string();
    let explicit_coin_ids = !coin_ids.is_empty();

    for iteration in 1..=max_iterations {
        let spendable = ctx.list_spendable_coins().await?;
        let gate_coins = spendable_coins_for_gate(&spendable);
        let combine_gate = combine_target.as_ref().map(|entry| {
            evaluate_coin_combine_gate(
                &gate_coins,
                &ctx.resolved_base_asset_id,
                target_coin_amount_mojos,
                combine_threshold_count(entry),
            )
        });

        if let Some(ref gate) = combine_gate {
            if until_ready && gate.ready {
                stop_reason = "ready".to_string();
                break;
            }
            let (should_stop, reason) = coin_op_should_stop(
                until_ready,
                Some(gate.ready),
                explicit_coin_ids,
                i64::from(iteration),
                i64::from(max_iterations),
            );
            if should_stop && until_ready {
                stop_reason = reason.to_string();
                break;
            }
        }

        let input_coin_ids = if coin_ids.is_empty() {
            plan_auto_combine_inputs(
                &spendable,
                number_of_coins as usize,
                CombineInputSelectionMode::ExactAmount,
                if target_coin_amount_mojos > 0 {
                    Some(target_coin_amount_mojos)
                } else {
                    None
                },
                None,
                Some(ctx.combine_input_cap as usize),
            )
            .map_err(|reason| SignerError::Other(reason.to_string()))?
        } else {
            coin_ids.to_vec()
        };
        if input_coin_ids.len() < 2 {
            emit_json(&json!({"error": "insufficient_combine_inputs"}))?;
            return Ok(2);
        }

        let total = total_for_coin_ids(&spendable, &input_coin_ids);
        let output_amounts = combine_output_amounts(total, 1);
        let operation_id = ctx
            .execute_mixed_split(
                output_amounts,
                &input_coin_ids,
                ctx.program.coin_ops_combine_fee_mojos.max(0) as u64,
            )
            .await?;
        operations.push(json!({
            "iteration": iteration,
            "signature_request_id": operation_id,
            "input_coin_ids": input_coin_ids,
            "waited": !no_wait,
            "denomination_readiness": combine_gate.as_ref().map(combine_gate_to_json),
        }));

        let (should_stop, reason) = coin_op_should_stop(
            until_ready,
            combine_gate.as_ref().map(|gate| gate.ready),
            explicit_coin_ids,
            i64::from(iteration),
            i64::from(max_iterations),
        );
        if should_stop {
            stop_reason = reason.to_string();
            break;
        }
        if no_wait {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    emit_json(&json!({
        "op": "coin-combine",
        "coin_selection_mode": if explicit_coin_ids { "explicit" } else { "adapter_auto_select" },
        "number_of_coins": number_of_coins,
        "resolved_asset_id": ctx.resolved_base_asset_id,
        "until_ready": until_ready,
        "max_iterations": max_iterations,
        "stop_reason": stop_reason,
        "operations": operations,
    }))?;
    Ok(if until_ready && stop_reason != "ready" { 2 } else { 0 })
}

async fn build_coin_op_exec_context(
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    asset_id_override: Option<&str>,
) -> SignerResult<CoinOpExecContext> {
    let program = load_program_config(program_path)?;
    let markets = load_markets_config_with_overlay(markets_path, testnet_markets_path)?;
    let market = resolve_market_for_build(&markets, market_id, pair, network)?;
    let signer_config = load_signer_config(program_path)?;
    let canonical = asset_id_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(market.base_asset.trim());
    let (resolved_base_asset_id, _) =
        resolve_offer_assets_for_action(&signer_config, canonical, "xch").await?;
    Ok(CoinOpExecContext {
        signer_config,
        market: market.clone(),
        program: program.clone(),
        resolved_base_asset_id,
        base_unit_mojo_multiplier: default_mojo_multiplier_for_asset(market.base_asset.trim())
            as i64,
        combine_input_cap: combine_input_coin_cap(),
        watched_coin_ids: HashSet::new(),
    })
}

async fn execute_combine_prereq(
    ctx: &CoinOpExecContext,
    prereq: &SplitCombinePrereqPlan,
) -> SignerResult<String> {
    let spendable = ctx.list_spendable_coins().await?;
    let total = total_for_coin_ids(&spendable, &prereq.input_coin_ids);
    let output_amounts = combine_output_amounts(total, prereq.input_coin_ids.len());
    ctx.execute_mixed_split(
        output_amounts,
        &prereq.input_coin_ids,
        ctx.program.coin_ops_combine_fee_mojos.max(0) as u64,
    )
    .await
}

fn enforce_split_lockup_guardrail(
    spendable: &[SpendableCoin],
    selected_coin_ids: &[String],
    allow_lock_all_spendable: bool,
    resolved_asset_id: &str,
) -> SignerResult<Option<i32>> {
    if allow_lock_all_spendable {
        return Ok(None);
    }
    let spendable_ids: HashSet<_> = spendable.iter().map(|coin| coin.id.clone()).collect();
    let selected_set: HashSet<String> = selected_coin_ids.iter().cloned().collect();
    if spendable_ids.is_empty() || selected_set != spendable_ids {
        return Ok(None);
    }
    emit_json(&json!({
        "error": COIN_SPLIT_LOCKUP_ERROR,
        "resolved_asset_id": resolved_asset_id,
        "spendable_asset_coin_count": spendable_ids.len(),
        "selected_spendable_coin_count": selected_set.len(),
    }))?;
    Ok(Some(2))
}

fn spendable_coins_for_gate(spendable: &[SpendableCoin]) -> Vec<Value> {
    spendable
        .iter()
        .map(|coin| {
            json!({
                "amount": coin.amount,
                "state": "CONFIRMED",
            })
        })
        .collect()
}

fn gate_to_json(gate: &CoinSplitGateResult) -> Value {
    json!({
        "asset_id": gate.asset_id,
        "size_base_units": gate.size_base_units,
        "required_min_count": gate.required_min_count,
        "current_count": gate.current_count,
        "larger_reserve_coin_count": gate.larger_reserve_coin_count,
        "extra_denom_coin_count": gate.extra_denom_coin_count,
        "reserve_ready": gate.reserve_ready,
        "ready": gate.ready,
    })
}

fn combine_gate_to_json(gate: &CoinCombineGateResult) -> Value {
    json!({
        "asset_id": gate.asset_id,
        "size_base_units": gate.size_base_units,
        "max_allowed_count": gate.max_allowed_count,
        "current_count": gate.current_count,
        "ready": gate.ready,
    })
}

async fn resolve_asset_filter(
    program_path: &Path,
    network: &str,
    filter: &str,
) -> SignerResult<String> {
    if is_hex_id(filter) {
        return Ok(normalize_hex_id(filter));
    }
    let signer_config = load_signer_config(program_path)?;
    let (resolved, _) = resolve_offer_assets_for_action(&signer_config, filter, "xch").await?;
    let _ = network;
    Ok(resolved)
}

fn select_list_market(markets: &crate::config::MarketsConfig) -> SignerResult<&MarketConfig> {
    let enabled: Vec<_> = markets.markets.iter().filter(|m| m.enabled).collect();
    let candidates = if enabled.is_empty() {
        markets.markets.iter().collect::<Vec<_>>()
    } else {
        enabled
    };
    if candidates.is_empty() {
        return Err(SignerError::Other("no markets configured".to_string()));
    }
    if candidates.len() == 1 {
        return Ok(candidates[0]);
    }
    Ok(candidates
        .iter()
        .min_by_key(|market| market.market_id.as_str())
        .copied()
        .expect("non-empty"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coin_ops::SpendableCoin;

    #[test]
    fn lockup_guardrail_blocks_when_all_spendable_selected() {
        let spendable = vec![
            SpendableCoin {
                id: "coin-a".to_string(),
                amount: 100,
            },
            SpendableCoin {
                id: "coin-b".to_string(),
                amount: 200,
            },
        ];
        let code = enforce_split_lockup_guardrail(
            &spendable,
            &["coin-a".to_string(), "coin-b".to_string()],
            false,
            "asset-1",
        )
        .expect("guardrail payload");
        assert_eq!(code, Some(2));
    }

    #[test]
    fn lockup_guardrail_allows_partial_selection() {
        let spendable = vec![
            SpendableCoin {
                id: "coin-a".to_string(),
                amount: 100,
            },
            SpendableCoin {
                id: "coin-b".to_string(),
                amount: 200,
            },
        ];
        let code = enforce_split_lockup_guardrail(
            &spendable,
            &["coin-a".to_string()],
            false,
            "asset-1",
        )
        .expect("guardrail");
        assert_eq!(code, None);
    }

    #[test]
    fn split_gate_ready_skips_execution_path() {
        let spendable = vec![
            SpendableCoin {
                id: "a".to_string(),
                amount: 100,
            },
            SpendableCoin {
                id: "b".to_string(),
                amount: 100,
            },
            SpendableCoin {
                id: "c".to_string(),
                amount: 200,
            },
        ];
        let gate = evaluate_coin_split_gate(
            &spendable_coins_for_gate(&spendable),
            "asset",
            100,
            2,
        );
        assert!(gate.ready);
        let (stop, reason) = coin_op_should_stop(true, Some(gate.ready), false, 1, 3);
        assert!(stop);
        assert_eq!(reason, "ready");
    }
}
