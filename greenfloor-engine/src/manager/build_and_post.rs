use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::adapters::{dexie_offer_view_url, post_offer_phase_dexie, DexieClient, SplashClient};
use crate::coinset::get_conservative_fee_estimate;
use crate::config::{
    action_side_from_pricing, load_markets_config_with_overlay, load_program_config,
    load_signer_config, require_signer_offer_path, resolve_market_for_build,
    resolve_offer_publish_settings, resolve_quote_asset_for_offer, MarketConfig,
};
use crate::error::{SignerError, SignerResult};
use crate::offer::build_context::resolve_quote_price_for_pricing;
use crate::offer::codec::verify_offer_for_dexie;
use crate::offer::publish::expected_publish_asset_fields;
use crate::offer::{build_signer_offer_for_action, resolve_offer_assets_for_action, BuildOfferForActionRequest};

use crate::storage::{
    persist_offer_post_records, state_db_path_for_home, OfferPostPersistRecord, SqliteStore,
};

use super::bootstrap::{bootstrap_blocks_offer, signer_bootstrap_phase, BootstrapPhaseResult};

#[derive(Debug, Clone)]
pub struct BuildAndPostOfferRequest {
    pub program_path: PathBuf,
    pub markets_path: PathBuf,
    pub testnet_markets_path: Option<PathBuf>,
    pub network: String,
    pub market_id: Option<String>,
    pub pair: Option<String>,
    pub size_base_units: u64,
    pub repeat: u32,
    pub publish_venue: Option<String>,
    pub dexie_base_url: Option<String>,
    pub splash_base_url: Option<String>,
    pub drop_only: bool,
    pub claim_rewards: bool,
    pub dry_run: bool,
    pub compact_json: bool,
    pub persist_results: bool,
}

#[derive(Debug, Clone)]
pub struct BuildAndPostOfferResponse {
    pub exit_code: i32,
    pub payload: Value,
    pub output: String,
}

pub fn format_build_and_post_output(payload: &Value, compact_json: bool) -> String {
    if compact_json {
        serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string())
    } else {
        serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".to_string())
    }
}

pub async fn build_and_post_offer(request: BuildAndPostOfferRequest) -> SignerResult<BuildAndPostOfferResponse> {
    if request.size_base_units == 0 {
        return Err(SignerError::Other(
            "size_base_units must be positive".to_string(),
        ));
    }
    if request.repeat == 0 {
        return Err(SignerError::Other("repeat must be positive".to_string()));
    }

    require_signer_offer_path(&request.program_path)?;
    let program = load_program_config(&request.program_path)?;
    let markets = load_markets_config_with_overlay(
        &request.markets_path,
        request.testnet_markets_path.as_deref(),
    )?;
    let market = resolve_market_for_build(
        &markets,
        request.market_id.as_deref(),
        request.pair.as_deref(),
        &request.network,
    )?;
    let (publish_venue, dexie_base_url, splash_base_url) = resolve_offer_publish_settings(
        &program,
        &request.network,
        request.publish_venue.as_deref(),
        request.dexie_base_url.as_deref(),
        request.splash_base_url.as_deref(),
    )?;

    let signer_config = load_signer_config(&request.program_path)?;
    let (resolved_base_asset_id, resolved_quote_asset_id) = resolve_offer_assets_for_action(
        &signer_config,
        &market.base_asset,
        &market.quote_asset,
    )
    .await?;
    let _resolved_quote_for_pricing =
        resolve_quote_asset_for_offer(&market.quote_asset, &request.network);
    let quote_price = resolve_quote_price_for_pricing(&market.pricing)?;
    let action_side = action_side_from_pricing(&market.pricing);

    let (offer_fee_mojos, offer_fee_source) =
        resolve_maker_offer_fee(&request.network).await;

    let mut post_results = Vec::new();
    let mut built_offers_preview = Vec::new();
    let mut bootstrap_actions = Vec::new();
    let mut publish_failures = 0u32;
    let mut persist_records: Vec<OfferPostPersistRecord> = Vec::new();

    let dexie = if !request.dry_run && publish_venue.as_str() == "dexie" {
        Some(DexieClient::new(dexie_base_url.clone()))
    } else {
        None
    };
    let splash = if !request.dry_run && publish_venue.as_str() == "splash" {
        Some(SplashClient::new(splash_base_url.clone()))
    } else {
        None
    };

    for _ in 0..request.repeat {
        let started = std::time::Instant::now();
        let bootstrap_result = if request.dry_run {
            BootstrapPhaseResult::skipped("dry_run")
        } else {
            signer_bootstrap_phase(
                &program,
                &market,
                &request.program_path,
                &resolved_base_asset_id,
                &resolved_quote_asset_id,
                quote_price,
                &action_side,
            )
            .await?
        };
        bootstrap_actions.push(bootstrap_result.to_manager_json());
        if let Some(error) = bootstrap_blocks_offer(&bootstrap_result) {
            publish_failures += 1;
            post_results.push(failure_result(
                &publish_venue,
                started,
                &error,
                None,
                Some(bootstrap_result.to_manager_json()),
            ));
            continue;
        }

        let create_started = std::time::Instant::now();
        let create_result = match create_offer(
            &request.program_path,
            &market,
            request.size_base_units,
            quote_price,
            &action_side,
        )
        .await
        {
            Ok(result) => result,
            Err(err) => {
                publish_failures += 1;
                post_results.push(failure_result(
                    &publish_venue,
                    started,
                    &err.to_string(),
                    Some(create_started.elapsed()),
                    None,
                ));
                continue;
            }
        };
        let create_phase_ms = create_started.elapsed().as_millis() as u64;
        let offer_text = create_result
            .get("offer_text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if offer_text.is_empty() {
            publish_failures += 1;
            post_results.push(json!({
                "venue": publish_venue,
                "result": {
                    "success": false,
                    "error": "signer_offer_text_unavailable",
                    "execution_mode": create_result.get("execution_mode").cloned().unwrap_or(Value::Null),
                    "timing_ms": timing_payload(started, Some(create_phase_ms), None, None),
                }
            }));
            continue;
        }

        if request.dry_run {
            built_offers_preview.push(json!({
                "offer_prefix": &offer_text[..offer_text.len().min(24)],
                "offer_length": offer_text.len().to_string(),
            }));
            continue;
        }

        if let Some(verify_error) = verify_offer_for_dexie(&offer_text) {
            publish_failures += 1;
            post_results.push(json!({
                "venue": publish_venue,
                "result": {
                    "success": false,
                    "error": verify_error,
                    "timing_ms": timing_payload(started, Some(create_phase_ms), None, None),
                }
            }));
            continue;
        }

        let asset_fields = expected_publish_asset_fields(
            create_result
                .get("side")
                .and_then(Value::as_str)
                .unwrap_or(&action_side),
            &market.base_symbol,
            &market.quote_asset,
            &resolved_base_asset_id,
            &resolved_quote_asset_id,
        );
        let publish_started = std::time::Instant::now();
        let publish_result = publish_offer(
            &publish_venue,
            dexie.as_ref(),
            splash.as_ref(),
            &offer_text,
            request.drop_only,
            request.claim_rewards,
            &asset_fields.expected_offered_asset_id,
            &asset_fields.expected_offered_symbol,
            &asset_fields.expected_requested_asset_id,
            &asset_fields.expected_requested_symbol,
        )
        .await?;
        let publish_ms = publish_started.elapsed().as_millis() as u64;
        if publish_result.get("success").and_then(Value::as_bool) != Some(true) {
            publish_failures += 1;
        }

        let mut result_payload = publish_result;
        if let Value::Object(obj) = &mut result_payload {
            obj.insert(
                "execution_mode".to_string(),
                create_result
                    .get("execution_mode")
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            obj.insert(
                "timing_ms".to_string(),
                timing_payload(
                    started,
                    Some(create_phase_ms),
                    Some(create_phase_ms),
                    Some(publish_ms),
                ),
            );
        }
        if publish_venue.as_str() == "dexie" {
            let offer_id = result_payload
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            if let Some(offer_id) = offer_id {
                if let Value::Object(obj) = &mut result_payload {
                    obj.insert(
                        "offer_view_url".to_string(),
                        Value::String(dexie_offer_view_url(&dexie_base_url, &offer_id)),
                    );
                }
                if result_payload.get("success").and_then(Value::as_bool) == Some(true) {
                    let side = create_result
                        .get("side")
                        .and_then(Value::as_str)
                        .unwrap_or(&action_side)
                        .to_string();
                    let execution_mode = create_result
                        .get("execution_mode")
                        .cloned()
                        .unwrap_or(Value::Null);
                    persist_records.push(OfferPostPersistRecord {
                        offer_id,
                        market_id: market.market_id.clone(),
                        side,
                        size_base_units: request.size_base_units,
                        publish_venue: publish_venue.clone(),
                        resolved_base_asset_id: resolved_base_asset_id.clone(),
                        resolved_quote_asset_id: resolved_quote_asset_id.clone(),
                        created_extra: json!({"execution_mode": execution_mode}),
                    });
                }
            }
        } else if result_payload.get("success").and_then(Value::as_bool) == Some(true) {
            if let Some(offer_id) = result_payload
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
            {
                let side = create_result
                    .get("side")
                    .and_then(Value::as_str)
                    .unwrap_or(&action_side)
                    .to_string();
                let execution_mode = create_result
                    .get("execution_mode")
                    .cloned()
                    .unwrap_or(Value::Null);
                persist_records.push(OfferPostPersistRecord {
                    offer_id,
                    market_id: market.market_id.clone(),
                    side,
                    size_base_units: request.size_base_units,
                    publish_venue: publish_venue.clone(),
                    resolved_base_asset_id: resolved_base_asset_id.clone(),
                    resolved_quote_asset_id: resolved_quote_asset_id.clone(),
                    created_extra: json!({"execution_mode": execution_mode}),
                });
            }
        }
        post_results.push(json!({
            "venue": publish_venue,
            "result": result_payload,
        }));
    }

    if request.persist_results && !request.dry_run && !persist_records.is_empty() {
        let db_path = state_db_path_for_home(&program.home_dir);
        let store = SqliteStore::open(&db_path)?;
        persist_offer_post_records(&store, &persist_records)?;
    }

    let payload = json!({
        "market_id": market.market_id,
        "pair": format!("{}:{}", market.base_asset, market.quote_asset),
        "resolved_base_asset_id": resolved_base_asset_id,
        "resolved_quote_asset_id": resolved_quote_asset_id,
        "network": program.network,
        "size_base_units": request.size_base_units,
        "repeat": request.repeat,
        "publish_venue": publish_venue,
        "dexie_base_url": dexie_base_url,
        "splash_base_url": if publish_venue.as_str() == "splash" { Value::String(splash_base_url) } else { Value::Null },
        "drop_only": request.drop_only,
        "claim_rewards": request.claim_rewards,
        "dry_run": request.dry_run,
        "publish_attempts": post_results.len(),
        "publish_failures": publish_failures,
        "built_offers_preview": built_offers_preview,
        "bootstrap_actions": bootstrap_actions,
        "results": post_results,
        "offer_fee_mojos": offer_fee_mojos,
        "offer_fee_source": offer_fee_source,
        "execution_backend": "signer",
        "signer_path": true,
    });
    let exit_code = if publish_failures == 0 { 0 } else { 2 };
    let output = format_build_and_post_output(&payload, request.compact_json);
    Ok(BuildAndPostOfferResponse {
        exit_code,
        payload,
        output,
    })
}

async fn create_offer(
    program_path: &Path,
    market: &MarketConfig,
    size_base_units: u64,
    quote_price: f64,
    action_side: &str,
) -> SignerResult<Value> {
    let signer_config = load_signer_config(program_path)?;
    let request = BuildOfferForActionRequest {
        receive_address: market.receive_address.clone(),
        base_asset: market.base_asset.clone(),
        quote_asset: market.quote_asset.clone(),
        size_base_units,
        action_side: action_side.to_string(),
        pricing: market.pricing.clone(),
        quote_price: Some(quote_price),
        split_input_coins: true,
        broadcast_split: true,
        offer_coin_ids: Vec::new(),
    };
    let result = build_signer_offer_for_action(signer_config, request).await?;
    Ok(json!({
        "offer_text": result.offer_text,
        "side": result.side,
        "expires_at_unix": result.expires_at_unix,
        "execution_mode": result.execution_mode,
    }))
}

async fn publish_offer(
    publish_venue: &str,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
    offer_text: &str,
    drop_only: bool,
    claim_rewards: bool,
    expected_offered_asset_id: &str,
    expected_offered_symbol: &str,
    expected_requested_asset_id: &str,
    expected_requested_symbol: &str,
) -> SignerResult<Value> {
    match publish_venue {
        "dexie" => {
            let dexie = dexie.ok_or_else(|| {
                SignerError::Other("dexie adapter missing for dexie publish".to_string())
            })?;
            post_offer_phase_dexie(
                dexie,
                offer_text,
                drop_only,
                claim_rewards,
                expected_offered_asset_id,
                expected_offered_symbol,
                expected_requested_asset_id,
                expected_requested_symbol,
            )
            .await
        }
        "splash" => {
            let splash = splash.ok_or_else(|| {
                SignerError::Other("splash adapter missing for splash publish".to_string())
            })?;
            splash.post_offer(offer_text).await
        }
        other => Err(SignerError::Other(format!(
            "unsupported publish venue: {other}"
        ))),
    }
}

async fn resolve_maker_offer_fee(network: &str) -> (u64, String) {
    match get_conservative_fee_estimate(network, None, 1_000_000, Some(1)).await {
        Ok(Some(fee)) => (fee, "coinset_conservative_fee".to_string()),
        Ok(None) => (0, "coinset_fee_unavailable".to_string()),
        Err(_) => (0, "coinset_fee_unavailable".to_string()),
    }
}

fn timing_payload(
    started: std::time::Instant,
    create_phase_ms: Option<u64>,
    create_total_ms: Option<u64>,
    publish_ms: Option<u64>,
) -> Value {
    json!({
        "create_phase_ms": create_phase_ms,
        "publish_ms": publish_ms,
        "total_ms": started.elapsed().as_millis() as u64,
        "create_total_ms": create_total_ms.or(create_phase_ms),
    })
}

fn failure_result(
    publish_venue: &str,
    started: std::time::Instant,
    error: &str,
    create_phase: Option<std::time::Duration>,
    bootstrap: Option<Value>,
) -> Value {
    let create_phase_ms = create_phase.map(|duration| duration.as_millis() as u64);
    let mut result = json!({
        "venue": publish_venue,
        "result": {
            "success": false,
            "error": error,
            "timing_ms": timing_payload(started, create_phase_ms, create_phase_ms, None),
        }
    });
    if let Some(bootstrap) = bootstrap {
        if let Value::Object(obj) = &mut result {
            if let Some(result_obj) = obj.get_mut("result").and_then(Value::as_object_mut) {
                result_obj.insert("bootstrap".to_string(), bootstrap);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_pretty_and_compact_json() {
        let payload = json!({"ok": true});
        assert!(format_build_and_post_output(&payload, false).contains('\n'));
        assert_eq!(
            format_build_and_post_output(&payload, true),
            r#"{"ok":true}"#
        );
    }
}
