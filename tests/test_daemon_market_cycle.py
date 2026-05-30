from __future__ import annotations

from dataclasses import dataclass

from greenfloor.config import io as config_io
from greenfloor.daemon.testing import (
    MarketDispatchState,
    enqueue_immediate_requeue_market,
    match_watched_coin_ids,
    resolve_quote_asset_for_offer,
    select_market_batch,
    set_watched_coin_ids_for_market,
)


def test_select_market_batch_prioritizes_immediate_requeue_then_round_robin() -> None:
    @dataclass
    class _Market:
        market_id: str
        enabled: bool = True

    markets = [_Market("m1"), _Market("m2"), _Market("m3"), _Market("m4")]
    state = MarketDispatchState()
    enqueue_immediate_requeue_market(state, "m3")

    selected, consumed = select_market_batch(
        enabled_markets=markets,
        slot_count=2,
        dispatch_state=state,
    )
    assert [m.market_id for m in selected] == ["m3", "m1"]
    assert consumed == ["m3"]
    assert list(state.immediate_requeue_ids) == []

    selected_next, consumed_next = select_market_batch(
        enabled_markets=markets,
        slot_count=2,
        dispatch_state=state,
    )
    assert [m.market_id for m in selected_next] == ["m2", "m3"]
    assert consumed_next == []


def test_match_watched_coin_ids_returns_empty_without_overlap() -> None:
    set_watched_coin_ids_for_market(market_id="m-empty", coin_ids={"c" * 64})
    assert match_watched_coin_ids(observed_coin_ids=["d" * 64]) == {}


def test_resolve_quote_asset_for_offer_maps_symbol_from_cats(monkeypatch, tmp_path) -> None:
    cats = tmp_path / "cats.yaml"
    cats.write_text(
        "\n".join(
            [
                "cats:",
                "  - base_symbol: wUSDC.b",
                "    asset_id: fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d",
            ]
        ),
        encoding="utf-8",
    )
    monkeypatch.setattr(config_io, "default_cats_config_path", lambda: cats)

    resolved = resolve_quote_asset_for_offer(
        quote_asset="wUSDC.b",
        network="mainnet",
    )
    assert resolved == "fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d"
