from __future__ import annotations

import pytest

from greenfloor.config.models import parse_markets_config


def _base_market_row() -> dict:
    return {
        "id": "m1",
        "enabled": True,
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
    }


def test_parse_markets_config_rejects_invalid_strategy_spread() -> None:
    row = _base_market_row()
    row["pricing"] = {"strategy_target_spread_bps": 0}
    with pytest.raises(ValueError, match="strategy_target_spread_bps"):
        parse_markets_config({"markets": [row]})


def test_parse_markets_config_rejects_invalid_strategy_price_band() -> None:
    row = _base_market_row()
    row["pricing"] = {
        "strategy_min_xch_price_usd": 50.0,
        "strategy_max_xch_price_usd": 40.0,
    }
    with pytest.raises(
        ValueError, match="strategy_min_xch_price_usd must be <= strategy_max_xch_price_usd"
    ):
        parse_markets_config({"markets": [row]})


def test_parse_markets_config_accepts_valid_strategy_controls() -> None:
    row = _base_market_row()
    row["pricing"] = {
        "strategy_target_spread_bps": 120,
        "strategy_min_xch_price_usd": 20.0,
        "strategy_max_xch_price_usd": 60.0,
    }
    out = parse_markets_config({"markets": [row]})
    assert len(out.markets) == 1
    assert out.markets[0].pricing["strategy_target_spread_bps"] == 120
