use serde_json::Value;

use super::super::yaml_fields::{config_err, optional_bool, parse_u64_field, req_value};
use crate::error::SignerResult;

#[allow(clippy::struct_field_names)]
pub(super) struct RuntimeFields {
    pub runtime_loop_interval_seconds: u64,
    pub runtime_dry_run: bool,
    pub runtime_market_slot_count: u64,
    pub runtime_offer_parallelism_enabled: bool,
    pub runtime_offer_parallelism_max_workers: usize,
    pub runtime_reservation_ttl_seconds: u64,
    pub runtime_offer_bootstrap_wait_timeout_seconds: u64,
}

pub(super) fn parse_runtime_config(
    runtime: &serde_json::Map<String, Value>,
) -> SignerResult<RuntimeFields> {
    Ok(RuntimeFields {
        runtime_loop_interval_seconds: parse_u64_field(
            req_value(runtime, "loop_interval_seconds")?,
            "runtime.loop_interval_seconds",
        )?,
        runtime_dry_run: optional_bool(runtime, "dry_run", false),
        runtime_market_slot_count: parse_u64_field(
            runtime
                .get("market_slot_count")
                .unwrap_or(&Value::Number(0.into())),
            "runtime.market_slot_count",
        )?,
        runtime_offer_parallelism_enabled: optional_bool(
            runtime,
            "offer_parallelism_enabled",
            false,
        ),
        runtime_offer_parallelism_max_workers: parse_u64_field(
            runtime
                .get("offer_parallelism_max_workers")
                .unwrap_or(&Value::Number(4.into())),
            "runtime.offer_parallelism_max_workers",
        )?
        .max(1)
        .try_into()
        .map_err(|_| config_err("runtime.offer_parallelism_max_workers must fit in usize"))?,
        runtime_reservation_ttl_seconds: parse_u64_field(
            runtime
                .get("reservation_ttl_seconds")
                .unwrap_or(&Value::Number(300.into())),
            "runtime.reservation_ttl_seconds",
        )?
        .max(30),
        runtime_offer_bootstrap_wait_timeout_seconds: runtime_timeout_seconds(
            runtime,
            "offer_bootstrap_wait_timeout_seconds",
            "cloud_wallet_bootstrap_wait_timeout_seconds",
            120,
            10,
        )?,
    })
}

fn runtime_timeout_seconds(
    runtime: &serde_json::Map<String, Value>,
    neutral_key: &str,
    legacy_key: &str,
    default: u64,
    minimum: u64,
) -> SignerResult<u64> {
    for key in [neutral_key, legacy_key] {
        if let Some(raw) = runtime.get(key) {
            let parsed = parse_u64_field(raw, key)?;
            return Ok(parsed.max(minimum));
        }
    }
    Ok(default.max(minimum))
}
