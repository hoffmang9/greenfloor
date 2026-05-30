from __future__ import annotations

from dataclasses import dataclass, field

from greenfloor.config import io as config_io
from greenfloor.core.cycle import enqueue_immediate_requeue, select_market_batch
from greenfloor.daemon.testing import (
    match_watched_coin_ids,
    resolve_quote_asset_for_offer,
    set_watched_coin_ids_for_market,
)
from greenfloor.daemon.testing.watchlist import new_coin_watchlist_cache


@dataclass
class _DispatchState:
    cursor: int = 0
    immediate_requeue_ids: list[str] = field(default_factory=list)


def test_select_market_batch_prioritizes_immediate_requeue_then_round_robin() -> None:
    @dataclass
    class _Market:
        market_id: str
        enabled: bool = True

    markets = [_Market("m1"), _Market("m2"), _Market("m3"), _Market("m4")]
    enabled_ids = [market.market_id for market in markets]
    state = _DispatchState()
    state.immediate_requeue_ids = enqueue_immediate_requeue(list(state.immediate_requeue_ids), "m3")

    first = select_market_batch(
        enabled_market_ids=enabled_ids,
        slot_count=2,
        cursor=state.cursor,
        immediate_requeue_ids=list(state.immediate_requeue_ids),
    )
    state.cursor = first.cursor
    state.immediate_requeue_ids = list(first.immediate_requeue_ids)
    assert first.selected_market_ids == ["m3", "m1"]
    assert first.consumed_immediate_requeues == ["m3"]
    assert list(state.immediate_requeue_ids) == []

    second = select_market_batch(
        enabled_market_ids=enabled_ids,
        slot_count=2,
        cursor=state.cursor,
        immediate_requeue_ids=list(state.immediate_requeue_ids),
    )
    state.cursor = second.cursor
    assert second.selected_market_ids == ["m2", "m3"]
    assert second.consumed_immediate_requeues == []


def test_match_watched_coin_ids_returns_empty_without_overlap() -> None:
    coin_watchlist = new_coin_watchlist_cache()
    set_watched_coin_ids_for_market(
        coin_watchlist=coin_watchlist,
        market_id="m-empty",
        coin_ids={"c" * 64},
    )
    assert (
        match_watched_coin_ids(
            coin_watchlist=coin_watchlist,
            observed_coin_ids=["d" * 64],
        )
        == {}
    )


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
