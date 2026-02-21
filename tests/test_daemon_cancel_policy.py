from __future__ import annotations

from pathlib import Path
from typing import Any, cast

from greenfloor.config.models import MarketConfig, MarketInventoryConfig
from greenfloor.daemon.main import _execute_cancel_policy_for_market
from greenfloor.storage.sqlite import SqliteStore


class _FakeDexie:
    def __init__(self, result: dict):
        self.result = result
        self.cancelled: list[str] = []

    def cancel_offer(self, offer_id: str) -> dict:
        self.cancelled.append(offer_id)
        return dict(self.result)


class _FakeStore:
    def __init__(self) -> None:
        self.rows: list[dict] = []

    def upsert_offer_state(
        self, *, offer_id: str, market_id: str, state: str, last_seen_status: int | None
    ) -> None:
        self.rows.append(
            {
                "offer_id": offer_id,
                "market_id": market_id,
                "state": state,
                "last_seen_status": last_seen_status,
            }
        )


def _market(quote_asset_type: str, *, stable_vs_unstable: bool = False) -> MarketConfig:
    return MarketConfig(
        market_id="m1",
        enabled=True,
        base_asset="asset",
        base_symbol="BYC",
        quote_asset="xch",
        quote_asset_type=quote_asset_type,
        receive_address="xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        mode="sell_only",
        signer_key_id="key-main-1",
        inventory=MarketInventoryConfig(low_watermark_base_units=100),
        pricing={"cancel_policy_stable_vs_unstable": bool(stable_vs_unstable)},
    )


def test_cancel_policy_skips_non_unstable_market() -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._CANCEL_COOLDOWN_UNTIL.clear()
    out = _execute_cancel_policy_for_market(
        market=_market("stable"),
        offers=[{"id": "o1", "status": 0}],
        runtime_dry_run=False,
        current_xch_price_usd=30.0,
        previous_xch_price_usd=25.0,
        dexie=cast(Any, _FakeDexie({"success": True})),
        store=cast(Any, _FakeStore()),
    )
    assert out["eligible"] is False
    assert out["triggered"] is False
    assert out["reason"] == "not_unstable_leg_market"


def test_cancel_policy_requires_strong_price_move() -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._CANCEL_COOLDOWN_UNTIL.clear()
    out = _execute_cancel_policy_for_market(
        market=_market("unstable", stable_vs_unstable=True),
        offers=[{"id": "o1", "status": 0}],
        runtime_dry_run=False,
        current_xch_price_usd=30.2,
        previous_xch_price_usd=30.0,
        dexie=cast(Any, _FakeDexie({"success": True})),
        store=cast(Any, _FakeStore()),
    )
    assert out["eligible"] is True
    assert out["triggered"] is False
    assert out["reason"] == "price_move_below_threshold"


def test_cancel_policy_dry_run_marks_planned_only() -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._CANCEL_COOLDOWN_UNTIL.clear()
    dexie = _FakeDexie({"success": True})
    store = _FakeStore()
    out = _execute_cancel_policy_for_market(
        market=_market("unstable", stable_vs_unstable=True),
        offers=[{"id": "o1", "status": 0}, {"id": "o2", "status": 4}],
        runtime_dry_run=True,
        current_xch_price_usd=40.0,
        previous_xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )
    assert out["triggered"] is True
    assert out["planned_count"] == 1
    assert out["executed_count"] == 0
    assert out["items"][0]["status"] == "planned"
    assert dexie.cancelled == []
    assert store.rows == []


def test_cancel_policy_executes_and_persists_cancelled_state() -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._CANCEL_COOLDOWN_UNTIL.clear()
    dexie = _FakeDexie({"success": True})
    store = _FakeStore()
    out = _execute_cancel_policy_for_market(
        market=_market("unstable", stable_vs_unstable=True),
        offers=[{"id": "o1", "status": 0}],
        runtime_dry_run=False,
        current_xch_price_usd=45.0,
        previous_xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )
    assert out["triggered"] is True
    assert out["executed_count"] == 1
    assert dexie.cancelled == ["o1"]
    assert store.rows[0]["state"] == "cancelled"
    assert store.rows[0]["last_seen_status"] == 3
    assert out["items"][0]["attempts"] == 1


def test_cancel_policy_retry_exhaust_sets_cooldown(monkeypatch) -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._CANCEL_COOLDOWN_UNTIL.clear()
    monkeypatch.setenv("GREENFLOOR_OFFER_CANCEL_MAX_ATTEMPTS", "2")
    monkeypatch.setenv("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_CANCEL_COOLDOWN_SECONDS", "60")
    dexie = _FakeDexie({"success": False, "error": "dexie_down"})
    store = _FakeStore()
    out = _execute_cancel_policy_for_market(
        market=_market("unstable", stable_vs_unstable=True),
        offers=[{"id": "o1", "status": 0}, {"id": "o2", "status": 0}],
        runtime_dry_run=False,
        current_xch_price_usd=45.0,
        previous_xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )
    assert out["executed_count"] == 0
    assert dexie.cancelled == ["o1", "o1"]
    assert out["items"][0]["reason"].startswith("cancel_retry_exhausted:")
    assert out["items"][1]["reason"].startswith("cancel_cooldown_active:")


def test_cancel_policy_skips_when_market_not_flagged_stable_vs_unstable() -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._CANCEL_COOLDOWN_UNTIL.clear()
    out = _execute_cancel_policy_for_market(
        market=_market("unstable", stable_vs_unstable=False),
        offers=[{"id": "o1", "status": 0}],
        runtime_dry_run=False,
        current_xch_price_usd=45.0,
        previous_xch_price_usd=30.0,
        dexie=cast(Any, _FakeDexie({"success": True})),
        store=cast(Any, _FakeStore()),
    )
    assert out["eligible"] is False
    assert out["triggered"] is False
    assert out["reason"] == "not_stable_vs_unstable_market"


def test_sqlite_store_reads_latest_xch_snapshot(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "state.sqlite")
    try:
        assert store.get_latest_xch_price_snapshot() is None
        store.add_audit_event("xch_price_snapshot", {"price_usd": 31.5})
        store.add_audit_event("xch_price_snapshot", {"price_usd": 33.25})
        assert store.get_latest_xch_price_snapshot() == 33.25
    finally:
        store.close()
