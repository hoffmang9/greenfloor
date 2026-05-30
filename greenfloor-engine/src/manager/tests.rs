use std::io::Write;

use mockito::Matcher;
use serde_json::json;

use crate::adapters::{post_offer_phase_dexie, DexieClient};
use crate::config::{
    load_markets_config, load_program_config, require_signer_offer_path, resolve_market_for_build,
    resolve_offer_publish_settings, ManagerProgramConfig,
};

#[tokio::test]
async fn dexie_post_offer_phase_posts_and_verifies_visibility() {
    let mut server = mockito::Server::new_async().await;
    let offer_id = "offer-123";
    let _post = server
        .mock("POST", "/v1/offers")
        .with_status(200)
        .with_body(json!({"success": true, "id": offer_id}).to_string())
        .create_async()
        .await;
    let _get = server
        .mock("GET", Matcher::Regex(r"/v1/offers/.*".to_string()))
        .with_status(200)
        .with_body(
            json!({
                "offer": {
                    "id": offer_id,
                    "offered": [{"id": "basecat"}],
                    "requested": [{"code": "xch"}],
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let dexie = DexieClient::new(server.url());
    let result = post_offer_phase_dexie(
        &dexie,
        "offer1test",
        true,
        false,
        "basecat",
        "A1",
        "xch",
        "xch",
    )
    .await
    .expect("post");
    assert_eq!(result.get("success").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(result.get("id").and_then(|v| v.as_str()), Some(offer_id));
}

#[test]
fn manager_config_and_market_resolution() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program_path = dir.path().join("program.yaml");
    let markets_path = dir.path().join("markets.yaml");
    let mut program_file = std::fs::File::create(&program_path).expect("create");
    write!(
        program_file,
        r#"
app:
  network: mainnet
  home_dir: /tmp/gf
runtime:
  offer_bootstrap_wait_timeout_seconds: 120
venues:
  dexie:
    api_base: https://api.dexie.space
  splash:
    api_base: http://localhost:4000
  offer_publish:
    provider: dexie
coin_ops:
  minimum_fee_mojos: 10000000
signer:
  kms_key_id: arn:aws:kms:us-west-2:123:key/demo
vault:
  launcher_id: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  custody_threshold: 1
  recovery_threshold: 1
  recovery_clawback_timelock: 3600
  custody_keys:
    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"
      curve: SECP256R1
  recovery_keys:
    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"
      curve: BLS12_381
"#
    )
    .expect("write");
    std::fs::write(
        &markets_path,
        r#"
markets:
  - id: m1
    enabled: true
    base_asset: a1
    base_symbol: A1
    quote_asset: xch
    receive_address: xch1test
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
"#,
    )
    .expect("write markets");

    require_signer_offer_path(&program_path).expect("signer path");
    let program = load_program_config(&program_path).expect("program");
    assert_eq!(program.offer_publish_venue, "dexie");
    let markets = load_markets_config(&markets_path).expect("markets");
    let market = resolve_market_for_build(&markets, Some("m1"), None, "mainnet").expect("market");
    assert_eq!(market.market_id, "m1");
}

#[test]
fn resolve_offer_publish_settings_uses_program_defaults() {
    let program = ManagerProgramConfig {
        network: "mainnet".to_string(),
        home_dir: std::path::PathBuf::from("/tmp/gf"),
        app_log_level: "INFO".to_string(),
        app_log_level_was_missing: false,
        dexie_api_base: "https://api.dexie.space".to_string(),
        splash_api_base: "http://localhost:4000".to_string(),
        offer_publish_venue: "splash".to_string(),
        coin_ops_minimum_fee_mojos: 0,
        coin_ops_max_operations_per_run: 0,
        coin_ops_max_daily_fee_budget_mojos: 0,
        coin_ops_split_fee_mojos: 0,
        coin_ops_combine_fee_mojos: 0,
        runtime_offer_bootstrap_wait_timeout_seconds: 120,
        runtime_market_slot_count: 0,
        runtime_offer_parallelism_enabled: false,
        runtime_offer_parallelism_max_workers: 4,
        runtime_dry_run: false,
        runtime_loop_interval_seconds: 30,
        tx_block_trigger_mode: "websocket".to_string(),
        tx_block_websocket_url: String::new(),
        tx_block_websocket_reconnect_interval_seconds: 1,
        tx_block_fallback_poll_interval_seconds: 1,
    };
    let (venue, dexie_base, splash_base) =
        resolve_offer_publish_settings(&program, "mainnet", None, None, None).expect("settings");
    assert_eq!(venue, "splash");
    assert_eq!(dexie_base, "https://api.dexie.space");
    assert_eq!(splash_base, "http://localhost:4000");
}

#[test]
fn resolve_market_rejects_unknown_market_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let markets_path = dir.path().join("markets.yaml");
    std::fs::write(
        &markets_path,
        r#"
markets:
  - id: m1
    enabled: true
    base_asset: a1
    base_symbol: A1
    quote_asset: xch
    receive_address: xch1test
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
"#,
    )
    .expect("write");
    let markets = load_markets_config(&markets_path).expect("markets");
    let err = resolve_market_for_build(&markets, Some("missing"), None, "mainnet")
        .expect_err("missing market");
    assert!(err.to_string().contains("market_id not found"));
}

#[test]
fn resolve_market_rejects_ambiguous_pair() {
    let dir = tempfile::tempdir().expect("tempdir");
    let markets_path = dir.path().join("markets.yaml");
    std::fs::write(
        &markets_path,
        r#"
markets:
  - id: m1
    enabled: true
    base_asset: a1
    base_symbol: A1
    quote_asset: xch
    receive_address: xch1a
    pricing: { "side": "sell" }
  - id: m2
    enabled: true
    base_asset: a1
    base_symbol: A1
    quote_asset: xch
    receive_address: xch1b
    pricing: { "side": "sell" }
"#,
    )
    .expect("write");
    let markets = load_markets_config(&markets_path).expect("markets");
    let err =
        resolve_market_for_build(&markets, None, Some("a1:xch"), "mainnet").expect_err("ambiguous");
    assert!(err.to_string().contains("ambiguous"));
}
