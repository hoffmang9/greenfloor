use super::fixtures::{
    parse_json_output, run_manager, write_minimal_program, MinimalProgramParams,
};
use serde_json::json;

fn write_non_cat_markets(path: &std::path::Path) {
    let yaml = r#"markets:
  - id: m1
    enabled: true
    base_asset: "not_in_cats_catalog"
    base_symbol: "NOPE"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 10
"#;
    std::fs::write(path, yaml).expect("write markets");
}

fn write_cats(path: &std::path::Path, cat_asset_id: &str) {
    std::fs::write(
        path,
        format!("cats:\n  - base_symbol: DUST\n    asset_id: \"{cat_asset_id}\"\n"),
    )
    .expect("write cats");
}

#[test]
fn combine_market_cat_dust_dry_run_json_reports_no_enabled_cat_markets() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("create home");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    let cats = dir.path().join("cats.yaml");
    write_minimal_program(
        &program,
        MinimalProgramParams {
            home_dir: &home,
            ..Default::default()
        },
    );
    write_non_cat_markets(&markets);
    write_cats(&cats, &"f".repeat(64));

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
            "--dry-run",
        ],
        None,
        None,
    );
    assert_eq!(output.status.code(), Some(0), "stderr: {}", {
        String::from_utf8_lossy(&output.stderr)
    });
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("status"), Some(&json!("ok")));
    assert_eq!(
        payload.get("message"),
        Some(&json!("no_enabled_cat_markets"))
    );
    assert_eq!(payload.get("jobs"), Some(&json!([])));
}

#[test]
fn combine_market_cat_dust_json_reports_missing_launcher() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("create home");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    let cats = dir.path().join("cats.yaml");
    let cat_hex = "f".repeat(64);
    write_minimal_program(
        &program,
        MinimalProgramParams {
            home_dir: &home,
            ..Default::default()
        },
    );
    std::fs::write(
        &markets,
        format!(
            r#"markets:
  - id: dust_m
    enabled: true
    base_asset: "{cat_hex}"
    base_symbol: DUST
    quote_asset: xch
    quote_asset_type: unstable
    signer_key_id: key-main-1
    receive_address: xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h
    mode: sell_only
    inventory:
      low_watermark_base_units: 100
"#
        ),
    )
    .expect("write markets");
    write_cats(&cats, &cat_hex);

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
            "--dry-run",
            "--launcher-id-file",
            dir.path()
                .join("missing_launcher_cache.txt")
                .to_str()
                .expect("launcher cache"),
        ],
        None,
        None,
    );
    assert_eq!(output.status.code(), Some(1), "stderr: {}", {
        String::from_utf8_lossy(&output.stderr)
    });
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("status"), Some(&json!("error")));
    assert_eq!(
        payload.get("reason"),
        Some(&json!("launcher_id_resolution_failed"))
    );
}
