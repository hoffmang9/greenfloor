#[path = "fixtures/json_util.rs"]
mod json_util;
#[path = "fixtures/manager.rs"]
mod manager_fixtures;

use manager_fixtures::{parse_json_output, run_manager, write_manager_program_with_signer};
use serde_json::json;

#[test]
fn build_and_post_offer_dry_run_returns_preview() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_manager_program_with_signer(&program, dir.path());
    let markets_yaml = r#"markets:
  - id: m1
    enabled: true
    base_asset: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    base_symbol: "TCAT"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
"#;
    std::fs::write(&markets, markets_yaml).expect("write markets");
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "build-and-post-offer",
            "--market-id",
            "m1",
            "--size-base-units",
            "1",
            "--dry-run",
            "--network",
            "mainnet",
        ],
        Some(&[("GREENFLOOR_TEST_OFFER_TEXT", "offer1dryrunpreviewstub")]),
        None,
    );
    assert_eq!(output.status.code(), Some(0));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("dry_run"), Some(&json!(true)));
    assert_eq!(payload.get("publish_attempts"), Some(&json!(0)));
    assert!(payload
        .get("built_offers_preview")
        .and_then(|v| v.as_array())
        .is_some_and(|rows| !rows.is_empty()));
    assert_eq!(payload.get("results"), Some(&json!([])));
}
