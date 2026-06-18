use greenfloor_engine::config::{
    load_markets_config_with_overlay, parse_markets_config, resolve_market_for_build, MarketConfig,
    MarketsConfig,
};
use greenfloor_engine::error::SignerResult;
use serde_json::{json, Value};

fn base_market_row() -> Value {
    json!({
        "id": "m1",
        "enabled": true,
        "base_asset": "asset1",
        "base_symbol": "AS1",
        "quote_asset": "xch",
        "quote_asset_type": "unstable",
        "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        "mode": "sell_only",
        "signer_key_id": "key-main-1",
        "inventory": {"low_watermark_base_units": 1},
        "ladders": {"sell": [{"size_base_units": 1, "target_count": 1}]},
        "pricing": {},
    })
}

fn parse_single_market(row: Value) -> SignerResult<MarketConfig> {
    parse_markets_config(&json!({"markets": [row]})).map(|cfg| {
        cfg.markets
            .into_iter()
            .next()
            .expect("single market expected")
    })
}

fn sample_markets() -> MarketsConfig {
    parse_markets_config(&json!({
        "markets": [{
            "id": "m1",
            "enabled": true,
            "base_asset": "a1",
            "base_symbol": "A1",
            "quote_asset": "xch",
            "receive_address": "xch1test",
            "pricing": {"min_price_quote_per_base": 0.0031}
        }]
    }))
    .expect("markets")
}

#[test]
fn parse_markets_config_rejects_invalid_strategy_spread() {
    let mut row = base_market_row();
    row["pricing"] = json!({"strategy_target_spread_bps": 0});
    let err = parse_single_market(row).expect_err("invalid spread");
    assert!(err.to_string().contains("strategy_target_spread_bps"));
}

#[test]
fn parse_markets_config_rejects_invalid_strategy_price_band() {
    let mut row = base_market_row();
    row["pricing"] = json!({
        "strategy_min_xch_price_usd": 50.0,
        "strategy_max_xch_price_usd": 40.0,
    });
    let err = parse_single_market(row).expect_err("invalid price band");
    assert!(err
        .to_string()
        .contains("strategy_min_xch_price_usd must be <= strategy_max_xch_price_usd"));
}

#[test]
fn parse_markets_config_accepts_valid_strategy_controls() {
    let mut row = base_market_row();
    row["pricing"] = json!({
        "strategy_target_spread_bps": 120,
        "strategy_min_xch_price_usd": 20.0,
        "strategy_max_xch_price_usd": 60.0,
    });
    let market = parse_single_market(row).expect("valid strategy controls");
    assert_eq!(
        market.pricing["strategy_target_spread_bps"].as_i64(),
        Some(120)
    );
}

#[test]
fn parse_markets_config_rejects_legacy_strategy_expiry_fields() {
    let mut row = base_market_row();
    row["pricing"] = json!({
        "strategy_offer_expiry_unit": "hours",
        "strategy_offer_expiry_value": 2,
    });
    let err = parse_single_market(row).expect_err("legacy expiry fields");
    assert!(err
        .to_string()
        .contains("strategy_offer_expiry_unit/value are no longer supported"));
}

#[test]
fn parse_markets_config_rejects_invalid_strategy_expiry_minutes_type() {
    let mut row = base_market_row();
    row["pricing"] = json!({"strategy_offer_expiry_minutes": "abc"});
    let err = parse_single_market(row).expect_err("invalid expiry minutes type");
    assert!(err
        .to_string()
        .contains("strategy_offer_expiry_minutes must be an integer"));
}

#[test]
fn parse_markets_config_accepts_strategy_expiry_override() {
    let mut row = base_market_row();
    row["quote_asset_type"] = json!("stable");
    row["pricing"] = json!({"strategy_offer_expiry_minutes": 120});
    let market = parse_single_market(row).expect("strategy expiry override");
    assert_eq!(
        market.pricing["strategy_offer_expiry_minutes"].as_i64(),
        Some(120)
    );
}

#[test]
fn parse_markets_config_accepts_unstable_expiry_above_15_minutes() {
    let mut row = base_market_row();
    row["pricing"] = json!({"strategy_offer_expiry_minutes": 30});
    let market = parse_single_market(row).expect("unstable expiry above 15 minutes");
    assert_eq!(
        market.pricing["strategy_offer_expiry_minutes"].as_i64(),
        Some(30)
    );
}

#[test]
fn parse_markets_config_rejects_legacy_reference_fields() {
    let mut row = base_market_row();
    row["pricing"] = json!({
        "reference_source": "coingecko",
        "reference_pair": "xch_usd",
    });
    let err = parse_single_market(row).expect_err("legacy reference fields");
    assert!(err
        .to_string()
        .contains("reference_source is no longer supported"));
}

#[test]
fn parse_markets_config_rejects_invalid_cancel_move_threshold_bps() {
    let mut row = base_market_row();
    row["pricing"] = json!({"cancel_move_threshold_bps": 0});
    let err = parse_single_market(row).expect_err("invalid cancel threshold");
    assert!(err
        .to_string()
        .contains("cancel_move_threshold_bps must be positive"));
}

#[test]
fn parse_markets_config_accepts_cancel_move_threshold_bps() {
    let mut row = base_market_row();
    row["pricing"] = json!({"cancel_move_threshold_bps": 250});
    let market = parse_single_market(row).expect("cancel threshold");
    assert!(market.pricing.get("cancel_move_threshold_bps").is_none());
    assert_eq!(market.cancel_move_threshold_bps, Some(250));
}

#[test]
fn parse_markets_config_stable_quote_validates_present_strategy_fields() {
    let mut row = base_market_row();
    row["quote_asset_type"] = json!("stable");
    row["pricing"] = json!({
        "strategy_target_spread_bps": 0,
        "strategy_min_xch_price_usd": -1,
        "strategy_max_xch_price_usd": "invalid",
    });
    let err = parse_single_market(row).expect_err("stable strategy validation");
    assert!(err.to_string().contains("strategy_target_spread_bps"));
}

#[test]
fn parse_markets_config_defaults_cat_unit_multipliers_to_1000() {
    let mut row = base_market_row();
    row["base_asset"] = json!("BYC");
    row["quote_asset"] = json!("wUSDC.b");
    let market = parse_single_market(row).expect("cat multipliers");
    assert_eq!(
        market.pricing["base_unit_mojo_multiplier"].as_i64(),
        Some(1000)
    );
    assert_eq!(
        market.pricing["quote_unit_mojo_multiplier"].as_i64(),
        Some(1000)
    );
}

#[test]
fn parse_markets_config_defaults_xch_quote_multiplier_to_one_trillion() {
    let row = base_market_row();
    let market = parse_single_market(row).expect("xch quote multiplier");
    assert_eq!(
        market.pricing["base_unit_mojo_multiplier"].as_i64(),
        Some(1000)
    );
    assert_eq!(
        market.pricing["quote_unit_mojo_multiplier"].as_i64(),
        Some(1_000_000_000_000)
    );
}

#[test]
fn parse_markets_config_rejects_noncanonical_cat_base_multiplier() {
    let mut row = base_market_row();
    row["base_asset"] = json!("BYC");
    row["pricing"] = json!({"base_unit_mojo_multiplier": 10});
    let err = parse_single_market(row).expect_err("noncanonical base multiplier");
    assert!(err
        .to_string()
        .contains("base_unit_mojo_multiplier must be 1000 for CAT assets"));
}

#[test]
fn parse_markets_config_rejects_noncanonical_cat_quote_multiplier() {
    let mut row = base_market_row();
    row["quote_asset"] = json!("wUSDC.b");
    row["pricing"] = json!({"quote_unit_mojo_multiplier": 10});
    let err = parse_single_market(row).expect_err("noncanonical quote multiplier");
    assert!(err
        .to_string()
        .contains("quote_unit_mojo_multiplier must be 1000 for CAT assets"));
}

#[test]
fn parse_markets_config_preserves_xch_multiplier_override() {
    let mut row = base_market_row();
    row["pricing"] = json!({"quote_unit_mojo_multiplier": 1_000_000_000_000_i64});
    let market = parse_single_market(row).expect("xch multiplier override");
    assert_eq!(
        market.pricing["quote_unit_mojo_multiplier"].as_i64(),
        Some(1_000_000_000_000)
    );
}

#[test]
fn rejects_testnet_receive_address_in_base_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("markets.yaml");
    std::fs::write(
        &path,
        r#"markets:
  - id: bad_base
    enabled: true
    base_asset: "a1"
    base_symbol: "A1"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "txch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 100
"#,
    )
    .expect("write markets");
    let err = load_markets_config_with_overlay(&path, None).expect_err("txch in base");
    assert!(err.to_string().contains("testnet receive_address"));
}

#[test]
fn resolves_market_by_id() {
    let markets = sample_markets();
    let market = resolve_market_for_build(&markets, Some("m1"), None, "mainnet").expect("market");
    assert_eq!(market.market_id, "m1");
}

#[test]
fn resolves_market_by_pair() {
    let markets = sample_markets();
    let market =
        resolve_market_for_build(&markets, None, Some("A1:xch"), "mainnet").expect("market");
    assert_eq!(market.market_id, "m1");
}
