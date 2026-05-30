use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{json, Value};

use crate::adapters::{dexie_offer_view_url, post_offer_phase_dexie, DexieClient, SplashClient};
use crate::coinset::get_conservative_fee_estimate;
use crate::config::{
    action_side_from_pricing, load_markets_config_with_overlay, load_program_config,
    load_signer_config, require_signer_offer_path, resolve_market_for_build,
    resolve_offer_publish_settings, MarketConfig, ManagerProgramConfig, SignerConfig,
};
use crate::error::{SignerError, SignerResult};
use crate::offer::action::BuildOfferForActionResult;
use crate::offer::build_context::resolve_quote_price_for_pricing;
use crate::offer::codec::verify_offer_for_dexie;
use crate::offer::publish::expected_publish_asset_fields;
use crate::offer::{
    build_signer_offer_for_action, normalize_offer_side, resolve_offer_assets_for_action,
    BuildOfferForActionRequest,
};
use crate::storage::{
    persist_offer_post_records, state_db_path_for_home, OfferPostPersistRecord, SqliteStore,
};

use super::bootstrap::{bootstrap_blocks_offer, signer_bootstrap_phase, BootstrapPhaseResult};
use super::logging::{initialize_manager_file_logging, warn_if_log_level_auto_healed};

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
    /// When set, overrides ``pricing.side`` for bootstrap and offer construction (daemon buy/sell actions).
    pub action_side: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BuildAndPostOfferResponse {
    pub exit_code: i32,
    pub payload: Value,
    pub output: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedBuildAndPostContext {
    program: ManagerProgramConfig,
    market: MarketConfig,
    signer_config: SignerConfig,
    publish_venue: String,
    dexie_base_url: String,
    splash_base_url: String,
    resolved_base_asset_id: String,
    resolved_quote_asset_id: String,
    quote_price: f64,
    action_side: String,
    offer_fee_mojos: u64,
    offer_fee_source: String,
}

#[derive(Debug, Clone)]
struct PublishResult {
    success: bool,
    offer_id: Option<String>,
    body: Value,
}

#[derive(Debug)]
struct PostFailure {
    error: String,
    started: Instant,
    create_phase_ms: Option<u64>,
    execution_mode: Option<String>,
    bootstrap: Option<Value>,
}

pub fn format_build_and_post_output(payload: &Value, compact_json: bool) -> String {
    if compact_json {
        serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string())
    } else {
        serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".to_string())
    }
}

pub async fn build_and_post_offer(
    request: BuildAndPostOfferRequest,
) -> SignerResult<BuildAndPostOfferResponse> {
    if request.size_base_units == 0 {
        return Err(SignerError::Other(
            "size_base_units must be positive".to_string(),
        ));
    }
    if request.repeat == 0 {
        return Err(SignerError::Other("repeat must be positive".to_string()));
    }

    let ctx = resolve_build_and_post_context(&request).await?;

    let mut post_results = Vec::new();
    let mut built_offers_preview = Vec::new();
    let mut bootstrap_actions = Vec::new();
    let mut publish_failures = 0u32;
    let mut persist_records: Vec<OfferPostPersistRecord> = Vec::new();

    let dexie = if !request.dry_run && ctx.publish_venue == "dexie" {
        Some(DexieClient::new(ctx.dexie_base_url.clone()))
    } else {
        None
    };
    let splash = if !request.dry_run && ctx.publish_venue == "splash" {
        Some(SplashClient::new(ctx.splash_base_url.clone()))
    } else {
        None
    };

    for _ in 0..request.repeat {
        let (bootstrap_action, iteration) = run_post_iteration(
            &request,
            &ctx,
            dexie.as_ref(),
            splash.as_ref(),
        )
        .await?;
        bootstrap_actions.push(bootstrap_action);
        match iteration {
            PostIterationOutcome::Preview(preview) => built_offers_preview.push(preview),
            PostIterationOutcome::Failure(failure) => {
                publish_failures += 1;
                post_results.push(failure.to_venue_result(&ctx.publish_venue));
            }
            PostIterationOutcome::Success(success) => {
                if !success.success {
                    publish_failures += 1;
                }
                let venue_result = success.to_venue_result();
                if let Some(record) = success.persist_record {
                    persist_records.push(record);
                }
                post_results.push(venue_result);
            }
        }
    }

    persist_post_records_if_enabled(
        &ctx.program.home_dir,
        request.persist_results,
        request.dry_run,
        &persist_records,
    )?;

    let payload = json!({
        "market_id": ctx.market.market_id,
        "pair": format!("{}:{}", ctx.market.base_asset, ctx.market.quote_asset),
        "resolved_base_asset_id": ctx.resolved_base_asset_id,
        "resolved_quote_asset_id": ctx.resolved_quote_asset_id,
        "network": ctx.program.network,
        "size_base_units": request.size_base_units,
        "repeat": request.repeat,
        "publish_venue": ctx.publish_venue,
        "dexie_base_url": ctx.dexie_base_url,
        "splash_base_url": if ctx.publish_venue == "splash" { Value::String(ctx.splash_base_url.clone()) } else { Value::Null },
        "drop_only": request.drop_only,
        "claim_rewards": request.claim_rewards,
        "dry_run": request.dry_run,
        "publish_attempts": post_results.len(),
        "publish_failures": publish_failures,
        "built_offers_preview": built_offers_preview,
        "bootstrap_actions": bootstrap_actions,
        "results": post_results,
        "offer_fee_mojos": ctx.offer_fee_mojos,
        "offer_fee_source": ctx.offer_fee_source,
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

enum PostIterationOutcome {
    Preview(Value),
    Failure(PostFailure),
    Success(PostAttemptSuccess),
}

#[derive(Debug)]
struct PostAttemptSuccess {
    publish_venue: String,
    result: Value,
    success: bool,
    persist_record: Option<OfferPostPersistRecord>,
}

impl PostAttemptSuccess {
    fn to_venue_result(&self) -> Value {
        json!({
            "venue": self.publish_venue,
            "result": self.result,
        })
    }
}

impl PostFailure {
    fn to_venue_result(&self, publish_venue: &str) -> Value {
        let mut result = json!({
            "success": false,
            "error": self.error,
            "timing_ms": timing_payload(
                self.started,
                self.create_phase_ms,
                self.create_phase_ms,
                None,
            ),
        });
        if let Some(execution_mode) = &self.execution_mode {
            result["execution_mode"] = json!(execution_mode);
        }
        if let Some(bootstrap) = &self.bootstrap {
            result["bootstrap"] = bootstrap.clone();
        }
        json!({
            "venue": publish_venue,
            "result": result,
        })
    }
}

async fn resolve_build_and_post_context(
    request: &BuildAndPostOfferRequest,
) -> SignerResult<ResolvedBuildAndPostContext> {
    require_signer_offer_path(&request.program_path)?;
    let program = load_program_config(&request.program_path)?;
    initialize_manager_file_logging(&program.home_dir, &program.app_log_level)?;
    warn_if_log_level_auto_healed(
        program.app_log_level_was_missing,
        &request.program_path,
    );
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
    let quote_price = resolve_quote_price_for_pricing(&market.pricing)?;
    let action_side = resolve_action_side(request.action_side.as_deref(), &market.pricing);
    let (offer_fee_mojos, offer_fee_source) = resolve_maker_offer_fee(&request.network).await;

    Ok(ResolvedBuildAndPostContext {
        program,
        market,
        signer_config,
        publish_venue,
        dexie_base_url,
        splash_base_url,
        resolved_base_asset_id,
        resolved_quote_asset_id,
        quote_price,
        action_side,
        offer_fee_mojos,
        offer_fee_source,
    })
}

async fn run_post_iteration(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
) -> SignerResult<(Value, PostIterationOutcome)> {
    let started = Instant::now();

    let bootstrap_result = if request.dry_run {
        BootstrapPhaseResult::skipped("dry_run")
    } else {
        signer_bootstrap_phase(
            &ctx.program,
            &ctx.market,
            &ctx.signer_config,
            &ctx.resolved_base_asset_id,
            &ctx.resolved_quote_asset_id,
            ctx.quote_price,
            &ctx.action_side,
        )
        .await?
    };
    let bootstrap_action = bootstrap_result.to_manager_json();
    if let Some(error) = bootstrap_blocks_offer(&bootstrap_result) {
        return Ok((
            bootstrap_action,
            PostIterationOutcome::Failure(PostFailure {
                error,
                started,
                create_phase_ms: None,
                execution_mode: None,
                bootstrap: Some(bootstrap_result.to_manager_json()),
            }),
        ));
    }

    let create_started = Instant::now();
    let created = match create_offer(
        &ctx.signer_config,
        &ctx.market,
        request.size_base_units,
        ctx.quote_price,
        &ctx.action_side,
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            return Ok((
                bootstrap_action,
                PostIterationOutcome::Failure(PostFailure {
                    error: err.to_string(),
                    started,
                    create_phase_ms: Some(create_started.elapsed().as_millis() as u64),
                    execution_mode: None,
                    bootstrap: None,
                }),
            ));
        }
    };
    let create_phase_ms = create_started.elapsed().as_millis() as u64;

    if created.offer_text.trim().is_empty() {
        return Ok((
            bootstrap_action,
            PostIterationOutcome::Failure(PostFailure {
                error: "signer_offer_text_unavailable".to_string(),
                started,
                create_phase_ms: Some(create_phase_ms),
                execution_mode: Some(created.execution_mode.clone()),
                bootstrap: None,
            }),
        ));
    }

    if request.dry_run {
        let offer_text = created.offer_text.trim();
        return Ok((
            bootstrap_action,
            PostIterationOutcome::Preview(json!({
                "offer_prefix": &offer_text[..offer_text.len().min(24)],
                "offer_length": offer_text.len().to_string(),
            })),
        ));
    }

    if let Some(verify_error) = verify_offer_for_dexie(&created.offer_text) {
        return Ok((
            bootstrap_action,
            PostIterationOutcome::Failure(PostFailure {
                error: verify_error,
                started,
                create_phase_ms: Some(create_phase_ms),
                execution_mode: None,
                bootstrap: None,
            }),
        ));
    }

    let side = created.side.as_str();
    let asset_fields = expected_publish_asset_fields(
        side,
        &ctx.market.base_symbol,
        &ctx.market.quote_asset,
        &ctx.resolved_base_asset_id,
        &ctx.resolved_quote_asset_id,
    );
    let publish_started = Instant::now();
    let publish = publish_offer(
        &ctx.publish_venue,
        dexie,
        splash,
        created.offer_text.trim(),
        request.drop_only,
        request.claim_rewards,
        &asset_fields.expected_offered_asset_id,
        &asset_fields.expected_offered_symbol,
        &asset_fields.expected_requested_asset_id,
        &asset_fields.expected_requested_symbol,
    )
    .await?;
    let publish_ms = publish_started.elapsed().as_millis() as u64;

    let persist_record = offer_post_persist_record(
        &publish,
        side,
        &created.execution_mode,
        ctx,
        request.size_base_units,
    );
    let publish_success = publish.success;
    let result_payload = finalize_publish_payload(
        publish,
        &created.execution_mode,
        timing_payload(started, Some(create_phase_ms), Some(create_phase_ms), Some(publish_ms)),
        if ctx.publish_venue == "dexie" {
            Some(ctx.dexie_base_url.as_str())
        } else {
            None
        },
    );

    Ok((
        bootstrap_action,
        PostIterationOutcome::Success(PostAttemptSuccess {
            publish_venue: ctx.publish_venue.clone(),
            result: result_payload,
            success: publish_success,
            persist_record,
        }),
    ))
}

async fn create_offer(
    signer_config: &SignerConfig,
    market: &MarketConfig,
    size_base_units: u64,
    quote_price: f64,
    action_side: &str,
) -> SignerResult<BuildOfferForActionResult> {
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
    build_signer_offer_for_action(signer_config.clone(), request).await
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
) -> SignerResult<PublishResult> {
    let body = match publish_venue {
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
            .await?
        }
        "splash" => {
            let splash = splash.ok_or_else(|| {
                SignerError::Other("splash adapter missing for splash publish".to_string())
            })?;
            splash.post_offer(offer_text).await?
        }
        other => {
            return Err(SignerError::Other(format!(
                "unsupported publish venue: {other}"
            )));
        }
    };
    Ok(PublishResult::from_adapter_body(body))
}

impl PublishResult {
    fn from_adapter_body(body: Value) -> Self {
        let success = body.get("success").and_then(Value::as_bool) == Some(true);
        let offer_id = body
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        Self {
            success,
            offer_id,
            body,
        }
    }
}

fn finalize_publish_payload(
    publish: PublishResult,
    execution_mode: &str,
    timing_ms: Value,
    dexie_base_url: Option<&str>,
) -> Value {
    let mut payload = publish.body;
    if let Value::Object(obj) = &mut payload {
        obj.insert("execution_mode".to_string(), json!(execution_mode));
        obj.insert("timing_ms".to_string(), timing_ms);
        if publish.success {
            if let (Some(base_url), Some(offer_id)) = (dexie_base_url, publish.offer_id.as_deref())
            {
                obj.insert(
                    "offer_view_url".to_string(),
                    Value::String(dexie_offer_view_url(base_url, offer_id)),
                );
            }
        }
    }
    payload
}

fn offer_post_persist_record(
    publish: &PublishResult,
    side: &str,
    execution_mode: &str,
    ctx: &ResolvedBuildAndPostContext,
    size_base_units: u64,
) -> Option<OfferPostPersistRecord> {
    if !publish.success {
        return None;
    }
    let offer_id = publish.offer_id.clone()?;
    Some(OfferPostPersistRecord {
        offer_id,
        market_id: ctx.market.market_id.clone(),
        side: side.to_string(),
        size_base_units,
        publish_venue: ctx.publish_venue.clone(),
        resolved_base_asset_id: ctx.resolved_base_asset_id.clone(),
        resolved_quote_asset_id: ctx.resolved_quote_asset_id.clone(),
        created_extra: json!({"execution_mode": execution_mode}),
    })
}

pub(crate) fn persist_post_records_if_enabled(
    home_dir: &Path,
    persist_results: bool,
    dry_run: bool,
    records: &[OfferPostPersistRecord],
) -> SignerResult<()> {
    if !persist_results || dry_run || records.is_empty() {
        return Ok(());
    }
    let db_path = state_db_path_for_home(home_dir);
    let store = SqliteStore::open(&db_path)?;
    persist_offer_post_records(&store, records)
}

async fn resolve_maker_offer_fee(network: &str) -> (u64, String) {
    match get_conservative_fee_estimate(network, None, 1_000_000, Some(1)).await {
        Ok(Some(fee)) => (fee, "coinset_conservative_fee".to_string()),
        Ok(None) => (0, "coinset_fee_unavailable".to_string()),
        Err(_) => (0, "coinset_fee_unavailable".to_string()),
    }
}

fn resolve_action_side(action_side_override: Option<&str>, pricing: &Value) -> String {
    if let Some(side) = action_side_override.map(str::trim).filter(|value| !value.is_empty()) {
        return normalize_offer_side(side).to_string();
    }
    action_side_from_pricing(pricing)
}

fn timing_payload(
    started: Instant,
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

#[cfg(test)]
pub(crate) fn sample_resolved_build_and_post_context() -> ResolvedBuildAndPostContext {
    use std::collections::HashMap;

    use chia_protocol::Bytes32;

    use crate::vault::context::VaultCustodySnapshot;

    ResolvedBuildAndPostContext {
        program: ManagerProgramConfig {
            network: "mainnet".to_string(),
            home_dir: PathBuf::from("/tmp/gf"),
            app_log_level: "INFO".to_string(),
            app_log_level_was_missing: false,
            dexie_api_base: "https://api.dexie.space".to_string(),
            splash_api_base: "http://localhost:4000".to_string(),
            offer_publish_venue: "dexie".to_string(),
            coin_ops_minimum_fee_mojos: 0,
            runtime_offer_bootstrap_wait_timeout_seconds: 120,
            runtime_market_slot_count: 0,
            runtime_parallel_markets: false,
            runtime_dry_run: false,
            runtime_loop_interval_seconds: 30,
            tx_block_trigger_mode: "websocket".to_string(),
        },
        market: MarketConfig {
            market_id: "m1".to_string(),
            enabled: true,
            base_asset: "a1".to_string(),
            base_symbol: "A1".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "unstable".to_string(),
            receive_address: "xch1".to_string(),
            pricing: json!({}),
            cancel_move_threshold_bps: None,
            ladders: HashMap::new(),
        },
        signer_config: SignerConfig {
            network: "mainnet".to_string(),
            coinset_msp_base_url: String::new(),
            kms_key_id: String::new(),
            kms_region: String::new(),
            kms_public_key_hex: None,
            vault: VaultCustodySnapshot {
                launcher_id: Bytes32::default(),
                custody_threshold: 1,
                recovery_threshold: 1,
                recovery_clawback_timelock: 0,
                custody_keys: Vec::new(),
                recovery_keys: Vec::new(),
            },
        },
        publish_venue: "dexie".to_string(),
        dexie_base_url: "https://api.dexie.space".to_string(),
        splash_base_url: "http://localhost:4000".to_string(),
        resolved_base_asset_id: "a1".to_string(),
        resolved_quote_asset_id: "xch".to_string(),
        quote_price: 1.0,
        action_side: "sell".to_string(),
        offer_fee_mojos: 0,
        offer_fee_source: "coinset_fee_unavailable".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn formats_pretty_and_compact_json() {
        let payload = json!({"ok": true});
        assert!(format_build_and_post_output(&payload, false).contains('\n'));
        assert_eq!(
            format_build_and_post_output(&payload, true),
            r#"{"ok":true}"#
        );
    }

    #[test]
    fn offer_post_persist_record_requires_success_and_offer_id() {
        let ctx = sample_resolved_build_and_post_context();
        let failed = PublishResult {
            success: false,
            offer_id: Some("offer-1".to_string()),
            body: json!({"success": false}),
        };
        assert!(offer_post_persist_record(&failed, "sell", "direct", &ctx, 1).is_none());

        let success = PublishResult {
            success: true,
            offer_id: Some("offer-1".to_string()),
            body: json!({"success": true, "id": "offer-1"}),
        };
        let record = offer_post_persist_record(&success, "sell", "direct", &ctx, 10)
            .expect("record");
        assert_eq!(record.offer_id, "offer-1");
        assert_eq!(record.market_id, "m1");
    }

    #[test]
    fn post_attempt_success_tracks_publish_outcome_without_json_reparse() {
        let success = PostAttemptSuccess {
            publish_venue: "dexie".to_string(),
            result: json!({"success": false, "error": "dexie_http_error:500"}),
            success: false,
            persist_record: None,
        };
        assert!(!success.success);
        assert_eq!(
            success.to_venue_result()
                .get("result")
                .and_then(|value| value.get("error"))
                .and_then(Value::as_str),
            Some("dexie_http_error:500")
        );
    }

    #[test]
    fn persist_post_records_if_enabled_writes_sqlite() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        persist_post_records_if_enabled(
            &home,
            true,
            false,
            &[OfferPostPersistRecord {
                offer_id: "offer-abc".to_string(),
                market_id: "m1".to_string(),
                side: "sell".to_string(),
                size_base_units: 5,
                publish_venue: "dexie".to_string(),
                resolved_base_asset_id: "a1".to_string(),
                resolved_quote_asset_id: "xch".to_string(),
                created_extra: json!({"execution_mode": "direct"}),
            }],
        )
        .expect("persist");

        let db_path = state_db_path_for_home(Path::new(&home));
        let store = SqliteStore::open(&db_path).expect("open");
        assert_eq!(
            store
                .offer_state_for_id("offer-abc")
                .expect("state")
                .as_deref(),
            Some("open")
        );
    }

    #[test]
    fn persist_post_records_if_enabled_skips_dry_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        persist_post_records_if_enabled(
            dir.path(),
            true,
            true,
            &[OfferPostPersistRecord {
                offer_id: "offer-abc".to_string(),
                market_id: "m1".to_string(),
                side: "sell".to_string(),
                size_base_units: 5,
                publish_venue: "dexie".to_string(),
                resolved_base_asset_id: "a1".to_string(),
                resolved_quote_asset_id: "xch".to_string(),
                created_extra: json!({}),
            }],
        )
        .expect("skip");
        assert!(!state_db_path_for_home(dir.path()).exists());
    }

    #[test]
    fn resolve_action_side_prefers_explicit_override() {
        let pricing = json!({"side": "sell"});
        assert_eq!(
            resolve_action_side(Some("buy"), &pricing),
            "buy".to_string()
        );
        assert_eq!(resolve_action_side(None, &pricing), "sell".to_string());
        assert_eq!(resolve_action_side(Some(""), &pricing), "sell".to_string());
    }
}
