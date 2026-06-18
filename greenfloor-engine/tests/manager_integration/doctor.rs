use std::path::Path;

use super::fixtures::{copy_example_program_and_markets, parse_json_output, run_manager};
use serde_json::json;

fn run_doctor(
    program: &Path,
    markets: &Path,
    state_db: &Path,
    env: Option<&[(&str, &str)]>,
) -> (i32, serde_json::Value) {
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "--state-db",
            state_db.to_str().expect("state db"),
            "doctor",
        ],
        env,
        None,
    );
    (
        output.status.code().unwrap_or(-1),
        parse_json_output(&output.stdout),
    )
}

#[test]
fn doctor_reports_ok_with_example_configs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (program, markets) = copy_example_program_and_markets(dir.path());
    let state_db = dir.path().join("state.sqlite");
    let (code, _payload) = run_doctor(&program, &markets, &state_db, None);
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
    let (code, payload) = run_doctor(&program, &markets, &state_db, None);
    assert_eq!(code, 2);
    assert_eq!(payload.get("ok"), Some(&json!(false)));
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
    let env = [
        ("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "0"),
        ("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", "bad"),
    ];
    let (code, payload) = run_doctor(&program, &markets, &state_db, Some(&env));
    assert_eq!(code, 0);
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
