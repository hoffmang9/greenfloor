use super::fixtures::{parse_json_output, run_manager, write_manager_program_with_signer};

#[test]
fn combine_market_cat_dust_dry_run_emits_single_json_document() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    let cats = dir.path().join("cats.yaml");
    write_manager_program_with_signer(&program, dir.path());
    let cat_hex = "f".repeat(64);
    std::fs::write(
        &markets,
        format!(
            r#"markets:
  - id: hex_m
    enabled: true
    base_asset: "{cat_hex}"
    base_symbol: HEX
    quote_asset: xch
    quote_asset_type: unstable
    signer_key_id: key-main-1
    receive_address: xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h
    mode: sell_only
    inventory:
      low_watermark_base_units: 100
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
"#
        ),
    )
    .expect("write markets");
    std::fs::write(
        &cats,
        format!("cats:\n  - base_symbol: HEX\n    asset_id: \"{cat_hex}\"\n"),
    )
    .expect("write cats");

    let launcher_file = dir.path().join("launcher_id.txt");
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "--cats-config",
            cats.to_str().expect("cats"),
            "--json",
            "combine-market-cat-dust",
            "--launcher-id-file",
            launcher_file.to_str().expect("launcher file"),
            "--dry-run",
        ],
        None,
        None,
    );
    let stdout = std::str::from_utf8(&output.stdout).expect("utf8 stdout");
    assert!(
        !stdout.trim().is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = parse_json_output(&output.stdout);
    assert!(payload.get("jobs").is_some());
    assert_eq!(payload.get("dry_run"), Some(&serde_json::json!(true)));
    assert_eq!(payload.get("list_only"), Some(&serde_json::json!(false)));
    assert_eq!(
        stdout.trim().find('{'),
        Some(0),
        "stdout should be a single JSON document"
    );
    serde_json::from_str::<serde_json::Value>(stdout.trim()).expect("exactly one json value");
}
