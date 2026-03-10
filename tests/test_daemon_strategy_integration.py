from __future__ import annotations

from datetime import UTC, datetime

from greenfloor.config.models import MarketConfig, MarketInventoryConfig, MarketLadderEntry
from greenfloor.daemon.main import (
    _effective_sell_bucket_counts_for_coin_ops,
    _evaluate_two_sided_market_actions,
    _executed_sell_offer_counts_by_size,
    _normalize_strategy_pair,
    _strategy_config_from_market,
    _strategy_state_from_bucket_counts,
)


def _market_with_quote(quote_asset: str) -> MarketConfig:
    return MarketConfig(
        market_id="m1",
        enabled=True,
        base_asset="asset",
        base_symbol="BYC",
        quote_asset=quote_asset,
        quote_asset_type="unstable",
        receive_address="xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        mode="sell_only",
        signer_key_id="key-main-1",
        inventory=MarketInventoryConfig(low_watermark_base_units=100),
        ladders={
            "sell": [
                MarketLadderEntry(
                    size_base_units=1,
                    target_count=7,
                    split_buffer_count=1,
                    combine_when_excess_factor=2.0,
                ),
                MarketLadderEntry(
                    size_base_units=10,
                    target_count=3,
                    split_buffer_count=1,
                    combine_when_excess_factor=2.0,
                ),
                MarketLadderEntry(
                    size_base_units=100,
                    target_count=2,
                    split_buffer_count=0,
                    combine_when_excess_factor=2.0,
                ),
            ]
        },
    )


def test_normalize_strategy_pair_handles_xch_and_usdc_aliases() -> None:
    assert _normalize_strategy_pair("xch") == "xch"
    assert _normalize_strategy_pair("wUSDC.b") == "usdc"
    assert _normalize_strategy_pair("USDC") == "usdc"


def test_strategy_config_from_market_uses_sell_ladder_targets() -> None:
    cfg = _strategy_config_from_market(_market_with_quote("xch"))
    assert cfg.pair == "xch"
    assert cfg.ones_target == 7
    assert cfg.tens_target == 3
    assert cfg.hundreds_target == 2


def test_strategy_config_from_market_reads_configurable_price_bands_and_spread() -> None:
    market = _market_with_quote("xch")
    market.pricing = {
        "strategy_target_spread_bps": 140,
        "strategy_min_xch_price_usd": 26.5,
        "strategy_max_xch_price_usd": 39.0,
    }
    cfg = _strategy_config_from_market(market)
    assert cfg.target_spread_bps == 140
    assert cfg.min_xch_price_usd == 26.5
    assert cfg.max_xch_price_usd == 39.0


def test_strategy_state_from_bucket_counts_includes_xch_price() -> None:
    state = _strategy_state_from_bucket_counts(
        {1: 2, 10: 1, 100: 0},
        xch_price_usd=32.5,
    )
    assert state.ones == 2
    assert state.tens == 1
    assert state.hundreds == 0
    assert state.xch_price_usd == 32.5


def test_effective_sell_bucket_counts_for_coin_ops_counts_live_sells_toward_target_only() -> None:
    market = _market_with_quote("wUSDC.b")
    market.mode = "two_sided"
    market.quote_asset_type = "stable"
    market.ladders = {
        "buy": [
            MarketLadderEntry(
                size_base_units=10,
                target_count=1,
                split_buffer_count=1,
                combine_when_excess_factor=2.0,
            )
        ],
        "sell": [
            MarketLadderEntry(
                size_base_units=10,
                target_count=3,
                split_buffer_count=1,
                combine_when_excess_factor=2.0,
            )
        ],
    }
    effective = _effective_sell_bucket_counts_for_coin_ops(
        sell_ladder=market.ladders["sell"],
        wallet_bucket_counts={10: 0},
        active_sell_offer_counts_by_size={10: 3},
    )
    assert effective[10] == 3


def test_effective_sell_bucket_counts_for_coin_ops_caps_live_sell_credit_at_target() -> None:
    market = _market_with_quote("wUSDC.b")
    market.mode = "two_sided"
    market.quote_asset_type = "stable"
    market.ladders = {
        "buy": [
            MarketLadderEntry(
                size_base_units=10,
                target_count=1,
                split_buffer_count=1,
                combine_when_excess_factor=2.0,
            )
        ],
        "sell": [
            MarketLadderEntry(
                size_base_units=10,
                target_count=3,
                split_buffer_count=1,
                combine_when_excess_factor=2.0,
            )
        ],
    }
    effective = _effective_sell_bucket_counts_for_coin_ops(
        sell_ladder=market.ladders["sell"],
        wallet_bucket_counts={10: 0},
        active_sell_offer_counts_by_size={10: 4},
    )
    assert effective[10] == 3


def test_effective_sell_bucket_counts_for_coin_ops_accounts_for_new_sell_posts_in_cycle() -> None:
    market = _market_with_quote("wUSDC.b")
    market.mode = "sell_only"
    market.quote_asset_type = "stable"
    market.ladders = {
        "sell": [
            MarketLadderEntry(
                size_base_units=10,
                target_count=2,
                split_buffer_count=1,
                combine_when_excess_factor=2.0,
            )
        ]
    }
    effective = _effective_sell_bucket_counts_for_coin_ops(
        sell_ladder=market.ladders["sell"],
        wallet_bucket_counts={10: 2},
        active_sell_offer_counts_by_size={10: 0},
        newly_executed_sell_offer_counts_by_size={10: 2},
    )
    assert effective[10] == 2


def test_executed_sell_offer_counts_by_size_counts_only_executed_sell_items() -> None:
    counts = _executed_sell_offer_counts_by_size(
        {
            "items": [
                {"status": "executed", "side": "sell", "size": 10},
                {"status": "executed", "side": "sell", "size": 10},
                {"status": "executed", "side": "buy", "size": 10},
                {"status": "skipped", "side": "sell", "size": 10},
                {"status": "executed", "side": "sell", "size": 1},
            ]
        }
    )
    assert counts == {10: 2, 1: 1}


def test_evaluate_two_sided_market_actions_uses_side_targets_from_ladders() -> None:
    market = _market_with_quote("wUSDC.b")
    market.mode = "two_sided"
    market.quote_asset_type = "stable"
    market.ladders = {
        "buy": [
            MarketLadderEntry(
                size_base_units=10,
                target_count=1,
                split_buffer_count=0,
                combine_when_excess_factor=2.0,
            )
        ],
        "sell": [
            MarketLadderEntry(
                size_base_units=10,
                target_count=3,
                split_buffer_count=0,
                combine_when_excess_factor=2.0,
            )
        ],
    }
    actions = _evaluate_two_sided_market_actions(
        market=market,
        counts_by_side={"buy": {10: 0}, "sell": {10: 1}},
        xch_price_usd=None,
        now=datetime.now(UTC),
    )
    by_side = {(a.side, a.size): int(a.repeat) for a in actions}
    assert by_side[("buy", 10)] == 1
    assert by_side[("sell", 10)] == 2
