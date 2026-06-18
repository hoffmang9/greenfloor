use super::fixtures::{parse_json_output, run_manager, write_manager_program, write_markets_one};
use serde_json::json;

#[test]
fn coins_list_requires_signer_backend() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_manager_program(&program, dir.path());
    write_markets_one(&markets);
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "coins-list",
        ],
        None,
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(
        payload.get("error"),
        Some(&json!("coin_list_requires_signer_backend"))
    );
}
