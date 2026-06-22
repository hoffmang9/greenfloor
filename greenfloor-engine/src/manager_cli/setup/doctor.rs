use serde_json::json;

use crate::config::{load_markets_config_with_overlay, load_program_config};
use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::operator_log::{LogContext, DOCTOR_PING};
use crate::storage::{resolve_state_db_path, SqliteStore};

const ENV_INT_OVERRIDES: [(&str, i64); 7] = [
    ("GREENFLOOR_UNSTABLE_CANCEL_MOVE_BPS", 1),
    ("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", 1),
    ("GREENFLOOR_OFFER_POST_BACKOFF_MS", 0),
    ("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", 0),
    ("GREENFLOOR_OFFER_CANCEL_MAX_ATTEMPTS", 1),
    ("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", 0),
    ("GREENFLOOR_OFFER_CANCEL_COOLDOWN_SECONDS", 0),
];

pub fn run_doctor(ctx: &ManagerContext) -> SignerResult<i32> {
    let program_path = &ctx.program_config;
    let markets_path = &ctx.markets_config;
    let state_db = ctx.state_db_override();
    let testnet_markets_path = ctx.testnet_markets_path();
    let program = load_program_config(program_path)?;
    let markets = load_markets_config_with_overlay(markets_path, testnet_markets_path)?;
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    let enabled_markets: Vec<_> = markets.markets.iter().filter(|m| m.enabled).collect();
    if enabled_markets.is_empty() {
        warnings.push("no_enabled_markets".to_string());
    }
    let mut key_ids = Vec::new();
    for market in &enabled_markets {
        let key_id = market.signer_key_id.trim();
        if key_id.is_empty() {
            problems.push(format!(
                "market_key_error:{}:missing signer_key_id",
                market.market_id
            ));
        } else {
            key_ids.push(key_id.to_string());
        }
    }
    let db_path = resolve_state_db_path(&program.home_dir, state_db);
    match SqliteStore::open(&db_path) {
        Ok(store) => {
            if let Err(err) =
                LogContext::VALIDATION.audit(&store, DOCTOR_PING, &json!({"ok": true}), None)
            {
                problems.push(format!("db_error:{err}"));
            }
        }
        Err(err) => problems.push(format!("db_error:{err}")),
    }
    if !program.signer_offer_path_configured() {
        warnings.push("signer_not_configured:kms_key_id_or_vault_launcher_id".to_string());
    }
    collect_env_warnings(ctx, &mut warnings);
    key_ids.sort();
    key_ids.dedup();
    let ok = problems.is_empty();
    ctx.emit_json(&json!({
        "ok": ok,
        "program_config": program_path.display().to_string(),
        "markets_config": markets_path.display().to_string(),
        "state_db": db_path.display().to_string(),
        "enabled_markets": enabled_markets.len(),
        "resolved_key_ids": key_ids,
        "warnings": warnings,
        "problems": problems,
    }))?;
    Ok(if ok { 0 } else { 2 })
}

fn collect_env_warnings(ctx: &ManagerContext, warnings: &mut Vec<String>) {
    for (name, minimum) in ENV_INT_OVERRIDES {
        let raw = ctx.env_var(name);
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match trimmed.parse::<i64>() {
            Ok(value) if value < minimum => {
                warnings.push(format!("invalid_env_override:{name}:must_be>={minimum}"));
            }
            Err(_) => warnings.push(format!("invalid_env_override:{name}:not_integer")),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager_cli::test_support::{
        copy_example_program_and_markets, pop_json, ManagerContextBuilder,
    };

    #[test]
    fn doctor_reports_ok_with_example_configs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (program, markets) = copy_example_program_and_markets(dir.path());
        let state_db = dir.path().join("state.sqlite");
        let harness = ManagerContextBuilder::new(program, markets)
            .scratch_dir(dir.path().to_path_buf())
            .state_db(state_db.to_str().expect("state db"))
            .build_capturing();
        let code = run_doctor(&harness.ctx).expect("doctor");
        assert_eq!(code, 0);
    }

    #[test]
    fn doctor_fails_when_enabled_market_key_missing_from_registry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (program, markets) = copy_example_program_and_markets(dir.path());
        let markets_text = std::fs::read_to_string(&markets).expect("read markets");
        let patched = markets_text.replace("signer_key_id:", "signer_key_id: \"\" #");
        std::fs::write(&markets, patched).expect("patch markets");
        let state_db = dir.path().join("state.sqlite");
        let harness = ManagerContextBuilder::new(program, markets)
            .scratch_dir(dir.path().to_path_buf())
            .state_db(state_db.to_str().expect("state db"))
            .build_capturing();
        let code = run_doctor(&harness.ctx).expect("doctor");
        assert_eq!(code, 2);
        let payload = pop_json(&harness.captured);
        assert_eq!(payload.get("ok"), Some(&serde_json::json!(false)));
        let problems = payload
            .get("problems")
            .and_then(|v| v.as_array())
            .expect("problems");
        assert!(problems.iter().any(|problem| {
            problem
                .as_str()
                .is_some_and(|text| text.contains("missing signer_key_id"))
        }));
    }

    #[test]
    fn doctor_warns_on_invalid_runtime_override_env() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (program, markets) = copy_example_program_and_markets(dir.path());
        let state_db = dir.path().join("state.sqlite");
        let harness = ManagerContextBuilder::new(program, markets)
            .scratch_dir(dir.path().to_path_buf())
            .state_db(state_db.to_str().expect("state db"))
            .env_overrides(&[
                ("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "0"),
                ("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", "bad"),
            ])
            .build_capturing();
        let code = run_doctor(&harness.ctx).expect("doctor");
        assert_eq!(code, 0);
        let payload = pop_json(&harness.captured);
        let warnings = payload
            .get("warnings")
            .and_then(|v| v.as_array())
            .expect("warnings");
        assert!(warnings.iter().any(|warning| {
            warning
                .as_str()
                .is_some_and(|text| text.contains("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS"))
        }));
        assert!(warnings.iter().any(|warning| {
            warning
                .as_str()
                .is_some_and(|text| text.contains("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS"))
        }));
    }
}
