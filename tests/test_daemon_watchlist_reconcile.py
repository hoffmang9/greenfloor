from __future__ import annotations

from tests.helpers.daemon_test_fixtures import *  # noqa: F403

def test_reconcile_offer_states_expires_watched_offer_on_direct_dexie_404(tmp_path: Path) -> None:
    db_path = tmp_path / "state.sqlite"
    store = SqliteStore(db_path)
    market = _market()
    now = datetime.now(UTC)
    try:
        store.upsert_offer_state(
            offer_id="offer-50",
            market_id=market.market_id,
            state="open",
            last_seen_status=0,
        )
        store.add_audit_event(
            "strategy_offer_execution",
            {
                "market_id": market.market_id,
                "planned_count": 1,
                "executed_count": 1,
                "items": [
                    {
                        "offer_id": "offer-50",
                        "size": 50,
                        "side": "sell",
                        "status": "executed",
                        "reason": "dexie_post_success",
                    }
                ],
            },
            market_id=market.market_id,
        )

        class _FakeDexie:
            def get_offers(self, offered: str, requested: str) -> list[dict[str, Any]]:
                _ = offered, requested
                return []

            def get_offer(self, offer_id: str, *, timeout: int = 20) -> dict[str, Any]:
                _ = offer_id, timeout
                raise RuntimeError("HTTP Error 404: Not Found")

        result = MarketCycleResult()
        reconcile_market_cycle_offers(
            market=market,
            network="mainnet",
            dexie=cast(Any, _FakeDexie()),
            store=store,
            now=now,
            result=result,
        )

        rows = {
            r["offer_id"]: r for r in store.list_offer_states(market_id=market.market_id, limit=20)
        }
        transitions = store.list_recent_audit_events(
            event_types=["offer_lifecycle_transition"],
            market_id=market.market_id,
            limit=20,
        )
    finally:
        store.close()

    assert rows["offer-50"]["state"] == "expired"
    assert rows["offer-50"]["last_seen_status"] is None
    assert transitions[0]["payload"]["offer_id"] == "offer-50"
    assert transitions[0]["payload"]["signal_source"] == "dexie_get_offer_404"
    assert transitions[0]["payload"]["dexie_error"] == "HTTP Error 404: Not Found"
    assert result.immediate_requeue_requested is True
    assert "expired" in result.immediate_requeue_signals


def test_reconcile_offer_states_requests_immediate_requeue_on_tx_confirmed(
    tmp_path: Path,
) -> None:
    db_path = tmp_path / "state.sqlite"
    store = SqliteStore(db_path)
    market = _market()
    try:
        store.upsert_offer_state(
            offer_id="offer-confirmed",
            market_id=market.market_id,
            state="open",
            last_seen_status=0,
        )

        class _FakeDexie:
            def get_offers(self, offered: str, requested: str) -> list[dict[str, Any]]:
                _ = offered, requested
                return [{"id": "offer-confirmed", "status": 4}]

            def get_offer(self, offer_id: str, *, timeout: int = 20) -> dict[str, Any]:
                _ = offer_id, timeout
                raise RuntimeError("unexpected_get_offer_call")

        result = MarketCycleResult()
        reconcile_market_cycle_offers(
            market=market,
            network="mainnet",
            dexie=cast(Any, _FakeDexie()),
            store=store,
            now=datetime.now(UTC),
            result=result,
        )

        rows = {
            r["offer_id"]: r for r in store.list_offer_states(market_id=market.market_id, limit=20)
        }
    finally:
        store.close()

    assert rows["offer-confirmed"]["state"] == "tx_block_confirmed"
    assert rows["offer-confirmed"]["last_seen_status"] == 4
    assert result.immediate_requeue_requested is True
    assert "tx_confirmed" in result.immediate_requeue_signals


def test_reconcile_offer_states_resolves_quote_asset_before_dexie_fetch(
    monkeypatch, tmp_path: Path
) -> None:
    store = SqliteStore(tmp_path / "state.db")
    market = _market()
    market.quote_asset = "wUSDC.b"
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
    captured: dict[str, str] = {}

    class _FakeDexie:
        def get_offers(self, offered: str, requested: str) -> list[dict[str, Any]]:
            captured["offered"] = offered
            captured["requested"] = requested
            return []

    try:
        result = MarketCycleResult()
        reconcile_market_cycle_offers(
            market=market,
            network="mainnet",
            dexie=cast(Any, _FakeDexie()),
            store=store,
            now=datetime.now(UTC),
            result=result,
        )
    finally:
        store.close()

    assert captured == {
        "offered": "asset",
        "requested": "fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d",
    }


def test_reconcile_offer_states_resolves_base_asset_before_dexie_fetch(
    monkeypatch,
    tmp_path: Path,
) -> None:
    store = SqliteStore(tmp_path / "state.db")
    market = _market()
    market.base_asset = "BYC"
    hex_id = "a" * 64
    cats = tmp_path / "cats.yaml"
    cats.write_text(
        "\n".join(
            [
                "cats:",
                "  - base_symbol: BYC",
                f"    asset_id: {hex_id}",
            ]
        ),
        encoding="utf-8",
    )
    monkeypatch.setattr(config_io, "default_cats_config_path", lambda: cats)
    captured: dict[str, str] = {}

    class _FakeDexie:
        def get_offers(self, offered: str, requested: str) -> list[dict[str, Any]]:
            captured["offered"] = offered
            captured["requested"] = requested
            return []

    try:
        result = MarketCycleResult()
        reconcile_market_cycle_offers(
            market=market,
            network="mainnet",
            dexie=cast(Any, _FakeDexie()),
            store=store,
            now=datetime.now(UTC),
            result=result,
        )
    finally:
        store.close()

    assert captured == {"offered": hex_id, "requested": "xch"}


def test_reconcile_offer_states_dexie_fallback_status_does_not_mark_mempool(
    tmp_path: Path,
) -> None:
    db_path = tmp_path / "state.sqlite"
    store = SqliteStore(db_path)
    market = _market()
    try:
        store.upsert_offer_state(
            offer_id="offer-open",
            market_id=market.market_id,
            state="open",
            last_seen_status=0,
        )

        class _FakeDexie:
            def get_offers(self, offered: str, requested: str) -> list[dict[str, Any]]:
                _ = offered, requested
                return [{"id": "offer-open", "status": 5}]

            def get_offer(self, offer_id: str, *, timeout: int = 20) -> dict[str, Any]:
                _ = offer_id, timeout
                raise RuntimeError("unexpected_get_offer_call")

        result = MarketCycleResult()
        reconcile_market_cycle_offers(
            market=market,
            network="mainnet",
            dexie=cast(Any, _FakeDexie()),
            store=store,
            now=datetime.now(UTC),
            result=result,
        )

        rows = {
            r["offer_id"]: r for r in store.list_offer_states(market_id=market.market_id, limit=20)
        }
    finally:
        store.close()

    assert rows["offer-open"]["state"] == "open"
    assert rows["offer-open"]["last_seen_status"] == 5


