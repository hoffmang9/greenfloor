use std::collections::HashSet;

use serde_json::{json, Value};

use crate::coinset::{get_conservative_fee_estimate, list_wallet_unspent_coins, WalletUnspentCoin};
use crate::coin_ops::is_spendable_wallet_coin;
use crate::config::{LadderEntry, ManagerProgramConfig, MarketConfig, SignerConfig};
use crate::cycle::retry::{poll_exponential_advance_sleep, poll_exponential_sleep_now};
use crate::error::{SignerError, SignerResult};
use crate::offer::bootstrap::{
    bootstrap_early_phase, bootstrap_executed_phase, plan_bootstrap_mixed_outputs, BootstrapCoin,
    BootstrapPhaseSnapshot, BootstrapPlan, BootstrapPlanOutcome, PlannerLadderRow,
};
use crate::offer::build_context::mojo_multiplier_for_leg;
use crate::offer::publish::bootstrap_block_error;
use crate::offer::request::{normalize_offer_side, quote_mojos_for_base_size, signer_split_asset_id};
use crate::vault::{build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest};

#[derive(Debug, Clone)]
pub struct BootstrapPhaseResult {
    pub status: String,
    pub reason: String,
    pub ready: bool,
    pub fee_mojos: u64,
    pub fee_source: String,
    pub fee_lookup_error: Option<String>,
    pub wait_error: Option<String>,
    pub split_result: Value,
    pub wait_events: Vec<Value>,
    pub plan: Option<BootstrapPlan>,
}

impl BootstrapPhaseResult {
    pub fn to_manager_json(&self) -> Value {
        let mut payload = json!({
            "status": self.status,
            "reason": self.reason,
            "ready": self.ready,
            "fee_mojos": self.fee_mojos,
            "fee_source": self.fee_source,
            "fee_lookup_error": self.fee_lookup_error,
        });
        if let Some(wait_error) = &self.wait_error {
            payload["wait_error"] = json!(wait_error);
        }
        if !self.split_result.is_null() && self.split_result != json!({}) {
            payload["split_result"] = self.split_result.clone();
        }
        if !self.wait_events.is_empty() {
            payload["wait_events"] = Value::Array(self.wait_events.clone());
        }
        if let Some(plan) = &self.plan {
            payload["plan"] = json!({
                "source_coin_id": plan.source_coin_id,
                "source_amount": plan.source_amount,
                "output_amounts_base_units": plan.output_amounts_base_units,
                "total_output_amount": plan.total_output_amount,
                "change_amount": plan.change_amount,
                "output_count": plan.output_amounts_base_units.len(),
            });
        }
        payload
    }

    fn from_snapshot(snapshot: BootstrapPhaseSnapshot) -> Self {
        Self {
            status: snapshot.status.to_string(),
            reason: snapshot.reason,
            ready: snapshot.ready,
            fee_mojos: 0,
            fee_source: String::new(),
            fee_lookup_error: None,
            wait_error: None,
            split_result: json!({}),
            wait_events: Vec::new(),
            plan: None,
        }
    }

    pub(crate) fn skipped(reason: impl Into<String>) -> Self {
        Self {
            status: "skipped".to_string(),
            reason: reason.into(),
            ready: false,
            fee_mojos: 0,
            fee_source: String::new(),
            fee_lookup_error: None,
            wait_error: None,
            split_result: json!({}),
            wait_events: Vec::new(),
            plan: None,
        }
    }
}

pub fn bootstrap_blocks_offer(result: &BootstrapPhaseResult) -> Option<String> {
    bootstrap_block_error(&result.status, &result.reason, result.ready)
}

fn bootstrap_ladder_entries_for_side(
    side: &str,
    side_ladder: &[LadderEntry],
    pricing: &Value,
    quote_price: f64,
    resolved_quote_asset_id: &str,
) -> SignerResult<Vec<PlannerLadderRow>> {
    let side = normalize_offer_side(side);
    let mut quote_unit_multiplier: Option<i64> = None;
    if side == "buy" {
        quote_unit_multiplier = Some(mojo_multiplier_for_leg(
            pricing,
            "quote_unit_mojo_multiplier",
            resolved_quote_asset_id,
        ));
    }
    let mut entries = Vec::new();
    for entry in side_ladder {
        let mut size_base_units = entry.size_base_units;
        if let Some(multiplier) = quote_unit_multiplier {
            size_base_units = quote_mojos_for_base_size(
                size_base_units,
                quote_price,
                multiplier,
            );
            if size_base_units <= 0 {
                continue;
            }
        }
        entries.push(PlannerLadderRow {
            size_base_units,
            target_count: entry.target_count,
            split_buffer_count: entry.split_buffer_count,
        });
    }
    Ok(entries)
}

fn bootstrap_fee_cost_for_output_count(output_count: usize) -> u64 {
    let count = output_count.max(1) as u64;
    1_000_000 + count.saturating_sub(1) * 250_000
}

async fn resolve_bootstrap_split_fee(
    network: &str,
    minimum_fee_mojos: u64,
    output_count: usize,
) -> (u64, String, Option<String>) {
    let fee_cost = bootstrap_fee_cost_for_output_count(output_count);
    let spend_count = output_count.max(1) as u64;
    match get_conservative_fee_estimate(network, None, fee_cost, Some(spend_count)).await {
        Ok(Some(fee_mojos)) => (fee_mojos, "coinset_conservative_fee".to_string(), None),
        Ok(None) => (minimum_fee_mojos, "config_minimum_fee_fallback".to_string(), None),
        Err(err) => (
            minimum_fee_mojos,
            "config_minimum_fee_fallback".to_string(),
            Some(err.to_string()),
        ),
    }
}

fn wallet_coin_spendable(coin: &WalletUnspentCoin) -> bool {
    is_spendable_wallet_coin(&json!({
        "state": coin.state,
    }))
}

async fn wait_for_coinset_confirmation(
    network: &str,
    receive_address: &str,
    asset_id: &str,
    initial_coin_ids: &HashSet<String>,
    timeout_seconds: u64,
) -> SignerResult<Vec<Value>> {
    let start = std::time::Instant::now();
    let timeout = timeout_seconds.max(10) as i64;
    let initial_sleep = 2.0f64;
    let max_sleep = 20.0f64;
    let mut sleep_seconds = 0.0f64;
    loop {
        let elapsed_seconds = start.elapsed().as_secs() as i64;
        let Some(next_sleep) = poll_exponential_sleep_now(
            elapsed_seconds,
            timeout,
            sleep_seconds,
            initial_sleep,
            max_sleep,
        ) else {
            return Err(SignerError::Other(
                "confirmation_wait_timeout".to_string(),
            ));
        };
        let coins = list_wallet_unspent_coins(network, receive_address, asset_id).await?;
        let new_confirmed: Vec<_> = coins
            .into_iter()
            .filter(|coin| !initial_coin_ids.contains(&coin.id))
            .collect();
        if let Some(first) = new_confirmed.first() {
            return Ok(vec![json!({
                "event": "confirmed",
                "coin_name": first.name,
                "elapsed_seconds": elapsed_seconds.to_string(),
            })]);
        }
        tokio::time::sleep(std::time::Duration::from_secs_f64(next_sleep)).await;
        sleep_seconds = poll_exponential_advance_sleep(sleep_seconds, initial_sleep, max_sleep, 1.5);
    }
}

pub async fn signer_bootstrap_phase(
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    signer_config: &SignerConfig,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    quote_price: f64,
    action_side: &str,
) -> SignerResult<BootstrapPhaseResult> {
    let side = normalize_offer_side(action_side);
    let side_ladder = market
        .ladders
        .get(side)
        .cloned()
        .unwrap_or_default();
    if side_ladder.is_empty() {
        return Ok(BootstrapPhaseResult::skipped(format!("missing_{side}_ladder")));
    }

    let ladder_entries = bootstrap_ladder_entries_for_side(
        side,
        &side_ladder,
        &market.pricing,
        quote_price,
        resolved_quote_asset_id,
    )?;
    if ladder_entries.is_empty() {
        return Ok(BootstrapPhaseResult::skipped(format!(
            "empty_{side}_ladder_after_quote_conversion"
        )));
    }

    let split_asset_id = signer_split_asset_id(
        side,
        resolved_base_asset_id,
        resolved_quote_asset_id,
    );
    if split_asset_id.trim().is_empty() {
        return Ok(BootstrapPhaseResult::skipped(format!(
            "missing_{side}_asset_for_bootstrap"
        )));
    }

    let receive_address = market.receive_address.trim();
    if receive_address.is_empty() {
        return Ok(BootstrapPhaseResult::skipped(
            "missing_receive_address_for_bootstrap",
        ));
    }

    let asset_scoped_coins = match list_wallet_unspent_coins(
        &program.network,
        receive_address,
        &split_asset_id,
    )
    .await
    {
        Ok(coins) => coins,
        Err(err) => {
            return Ok(BootstrapPhaseResult::skipped(format!(
                "bootstrap_coin_list_failed:{err}"
            )));
        }
    };

    let spendable_coins: Vec<BootstrapCoin> = asset_scoped_coins
        .iter()
        .filter(|coin| wallet_coin_spendable(coin))
        .map(|coin| BootstrapCoin {
            id: coin.id.clone(),
            amount: i64::try_from(coin.amount).unwrap_or(i64::MAX),
        })
        .collect();

    let outcome = plan_bootstrap_mixed_outputs(&ladder_entries, &spendable_coins);
    if let Some(early) = bootstrap_early_phase(&outcome) {
        return Ok(BootstrapPhaseResult::from_snapshot(early));
    }
    let BootstrapPlanOutcome::NeedsSplit(bootstrap_plan) = outcome else {
        return Ok(BootstrapPhaseResult::skipped("bootstrap_precheck_failed"));
    };

    let (fee_mojos, fee_source, fee_lookup_error) = resolve_bootstrap_split_fee(
        &program.network,
        program.coin_ops_minimum_fee_mojos,
        bootstrap_plan.output_amounts_base_units.len(),
    )
    .await;
    if fee_mojos > 0 {
        return Ok(BootstrapPhaseResult {
            status: "failed".to_string(),
            reason: "signer_mixed_split_fee_not_supported".to_string(),
            ready: false,
            fee_mojos,
            fee_source,
            fee_lookup_error,
            wait_error: None,
            split_result: json!({}),
            wait_events: Vec::new(),
            plan: None,
        });
    }

    let existing_coin_ids: HashSet<String> = asset_scoped_coins
        .iter()
        .map(|coin| coin.id.clone())
        .collect();

    let split_result = match build_and_optionally_broadcast_vault_cat_mixed_split(
        signer_config.clone(),
        MixedSplitRequest {
            receive_address: receive_address.to_string(),
            asset_id: crate::vault::members::hex_to_bytes32(&split_asset_id)?,
            output_amounts: bootstrap_plan
                .output_amounts_base_units
                .iter()
                .map(|amount| u64::try_from(*amount).unwrap_or(0))
                .collect(),
            coin_ids: crate::coinset::parse_coin_ids(&[bootstrap_plan.source_coin_id.clone()])?,
            allow_sub_cat_output: false,
            fee_mojos: 0,
        },
        true,
    )
    .await
    {
        Ok(result) => json!({
            "offered_total": result.offered_total,
            "target_total": result.target_total,
            "change_amount": result.change_amount,
            "selected_coin_ids": result.selected_coin_ids,
            "broadcast_status": result.broadcast_status,
            "spend_bundle_hex": result.spend_bundle_hex,
        }),
        Err(err) => {
            return Ok(BootstrapPhaseResult {
                status: "failed".to_string(),
                reason: format!("signer_mixed_split_error:{err}"),
                ready: false,
                fee_mojos,
                fee_source,
                fee_lookup_error,
                wait_error: None,
                split_result: json!({}),
                wait_events: Vec::new(),
                plan: Some(bootstrap_plan),
            });
        }
    };

    let wait_events = match wait_for_coinset_confirmation(
        &program.network,
        receive_address,
        &split_asset_id,
        &existing_coin_ids,
        program.runtime_offer_bootstrap_wait_timeout_seconds,
    )
    .await
    {
        Ok(events) => events,
        Err(err) => {
            return Ok(BootstrapPhaseResult {
                status: "failed".to_string(),
                reason: "bootstrap_wait_failed".to_string(),
                ready: false,
                fee_mojos,
                fee_source,
                fee_lookup_error,
                wait_error: Some(err.to_string()),
                split_result,
                wait_events: Vec::new(),
                plan: Some(bootstrap_plan),
            });
        }
    };

    let refreshed_asset_coins =
        list_wallet_unspent_coins(&program.network, receive_address, &split_asset_id).await?;
    let refreshed_spendable: Vec<BootstrapCoin> = refreshed_asset_coins
        .iter()
        .filter(|coin| wallet_coin_spendable(coin))
        .map(|coin| BootstrapCoin {
            id: coin.id.clone(),
            amount: i64::try_from(coin.amount).unwrap_or(i64::MAX),
        })
        .collect();
    let remaining = plan_bootstrap_mixed_outputs(&ladder_entries, &refreshed_spendable);
    let executed = bootstrap_executed_phase(&remaining);
    Ok(BootstrapPhaseResult {
        status: executed.status.to_string(),
        reason: executed.reason,
        ready: executed.ready,
        fee_mojos,
        fee_source,
        fee_lookup_error,
        wait_error: None,
        split_result,
        wait_events,
        plan: Some(bootstrap_plan),
    })
}
