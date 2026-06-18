use std::path::Path;

use greenfloor_engine::config::{load_markets_config, load_program_config, parse_markets_config};
use serde_json::json;

#[test]
fn loads_repo_program_and_markets_configs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root");
    let program = root.join("config/program.yaml");
    let markets = root.join("config/markets.yaml");
    load_program_config(&program).expect("program config");
    load_markets_config(&markets).expect("markets config");
}

#[test]
fn parse_markets_config_requires_market_id() {
    let err =
        parse_markets_config(&json!({"markets": [{"enabled": true}]})).expect_err("missing id");
    assert!(err.to_string().contains("market id is required"));
}

#[test]
fn parse_markets_config_parses_cancel_move_threshold_bps() {
    let markets = parse_markets_config(&json!({
        "markets": [{
            "id": "m1",
            "enabled": true,
            "base_asset": "a1",
            "quote_asset": "xch",
            "receive_address": "xch1test",
            "pricing": {"cancel_move_threshold_bps": 250}
        }]
    }))
    .expect("markets");
    assert_eq!(markets.markets[0].cancel_move_threshold_bps, Some(250));
    assert!(markets.markets[0]
        .pricing
        .get("cancel_move_threshold_bps")
        .is_none());
}
