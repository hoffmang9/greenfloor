from __future__ import annotations

from tests.helpers.daemon_test_fixtures import *  # noqa: F403

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


def test_detect_stale_open_offers_for_requeue_marks_expired_status(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "state.db")
    try:
        store.upsert_offer_state(
            offer_id="offer-expired",
            market_id="m1",
            state="open",
            last_seen_status=0,
        )

        class _FakeDexie:
            @staticmethod
            def get_offer(offer_id: str, *, timeout: int = 5) -> dict[str, Any]:
                _ = timeout
                assert offer_id == "offer-expired"
                return {"offer": {"id": offer_id, "status": 6}}

        payload = detect_stale_open_offers_for_requeue(
            store=store,
            dexie=cast(Any, _FakeDexie()),
            enabled_market_ids={"m1"},
        )
    finally:
        store.close()

    assert payload["checked_offer_count"] == 1
    assert payload["requeue_market_ids"] == ["m1"]
    assert payload["hits"][0]["reason"] == "offer_expired"


def test_detect_stale_open_offers_for_requeue_marks_missing_404(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "state.db")
    try:
        store.upsert_offer_state(
            offer_id="offer-missing",
            market_id="m2",
            state="open",
            last_seen_status=0,
        )

        class _FakeDexie:
            @staticmethod
            def get_offer(offer_id: str, *, timeout: int = 5) -> dict[str, Any]:
                _ = offer_id, timeout
                raise RuntimeError("HTTP Error 404: Not Found")

        payload = detect_stale_open_offers_for_requeue(
            store=store,
            dexie=cast(Any, _FakeDexie()),
            enabled_market_ids={"m2"},
        )
    finally:
        store.close()

    assert payload["checked_offer_count"] == 1
    assert payload["requeue_market_ids"] == ["m2"]
    assert payload["hits"][0]["reason"] == "offer_missing_404"


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


