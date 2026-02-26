from __future__ import annotations

from typing import Any, cast

from greenfloor.config.models import MarketConfig, MarketInventoryConfig
from greenfloor.core.strategy import PlannedAction
from greenfloor.daemon import main as daemon_main
from greenfloor.daemon.main import _execute_strategy_actions


class _FakeDexie:
    def __init__(self, post_result: dict):
        self.post_result = post_result
        self.posted: list[str] = []
        self.calls = 0

    def post_offer(self, offer: str) -> dict:
        self.posted.append(offer)
        self.calls += 1
        return dict(self.post_result)


class _FakeStore:
    def __init__(self) -> None:
        self.offer_states: list[dict] = []

    def upsert_offer_state(
        self, *, offer_id: str, market_id: str, state: str, last_seen_status: int | None
    ) -> None:
        self.offer_states.append(
            {
                "offer_id": offer_id,
                "market_id": market_id,
                "state": state,
                "last_seen_status": last_seen_status,
            }
        )


def _market() -> MarketConfig:
    return MarketConfig(
        market_id="m1",
        enabled=True,
        base_asset="asset",
        base_symbol="BYC",
        quote_asset="xch",
        quote_asset_type="unstable",
        receive_address="xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        mode="sell_only",
        signer_key_id="key-main-1",
        inventory=MarketInventoryConfig(low_watermark_base_units=100),
        pricing={
            "fixed_quote_per_base": 0.5,
            "base_unit_mojo_multiplier": 1000,
            "quote_unit_mojo_multiplier": 1000,
        },
    )


def test_execute_strategy_actions_dry_run_plans_without_posting() -> None:
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-1"})
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        )
    ]

    result = _execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=True,
        xch_price_usd=32.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )

    assert result["planned_count"] == 2
    assert result["executed_count"] == 0
    assert len(result["items"]) == 2
    assert dexie.posted == []
    assert store.offer_states == []


def test_execute_strategy_actions_skips_when_builder_skips(monkeypatch) -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._POST_COOLDOWN_UNTIL.clear()

    monkeypatch.setattr(
        daemon_main,
        "_build_offer_for_action",
        lambda **_kwargs: {"status": "skipped", "reason": "builder_not_ready", "offer": None},
    )
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-1"})
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        )
    ]

    result = _execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=32.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )

    assert result["planned_count"] == 1
    assert result["executed_count"] == 0
    assert result["items"][0]["status"] == "skipped"
    assert result["items"][0]["reason"] == "builder_not_ready"
    assert dexie.posted == []
    assert store.offer_states == []


def test_execute_strategy_actions_posts_and_persists_offer_ids(monkeypatch) -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._POST_COOLDOWN_UNTIL.clear()

    monkeypatch.setattr(
        daemon_main,
        "_build_offer_for_action",
        lambda **_kwargs: {
            "status": "executed",
            "reason": "offer_builder_success",
            "offer": "offer1abc",
        },
    )
    dexie = _FakeDexie(post_result={"success": True, "id": "offer-123"})
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=10,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        )
    ]

    result = _execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=32.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )

    assert result["planned_count"] == 2
    assert result["executed_count"] == 2
    assert len(dexie.posted) == 2
    assert len(store.offer_states) == 2
    assert all(s["offer_id"] == "offer-123" for s in store.offer_states)


def test_execute_strategy_actions_retries_then_succeeds(monkeypatch) -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "3")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", "10")

    monkeypatch.setattr(
        daemon_main,
        "_build_offer_for_action",
        lambda **_kwargs: {"status": "executed", "reason": "ok", "offer": "offer1abc"},
    )

    class _FlakyDexie:
        def __init__(self) -> None:
            self.calls = 0

        def post_offer(self, _offer: str) -> dict:
            self.calls += 1
            if self.calls < 2:
                return {"success": False, "error": "temporary"}
            return {"success": True, "id": "offer-retry"}

    dexie = _FlakyDexie()
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        )
    ]
    result = _execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )
    assert result["executed_count"] == 1
    assert dexie.calls == 2
    assert result["items"][0]["attempts"] == 2


def test_execute_strategy_actions_applies_post_cooldown_after_retry_exhaust(monkeypatch) -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "2")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", "60")
    monkeypatch.setattr(
        daemon_main,
        "_build_offer_for_action",
        lambda **_kwargs: {"status": "executed", "reason": "ok", "offer": "offer1abc"},
    )

    dexie = _FakeDexie(post_result={"success": False, "error": "down"})
    store = _FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        )
    ]
    result = _execute_strategy_actions(
        market=_market(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
    )
    assert result["executed_count"] == 0
    assert dexie.calls == 2
    assert result["items"][0]["reason"].startswith("dexie_post_retry_exhausted:")
    assert result["items"][1]["reason"].startswith("post_cooldown_active:")


def test_build_offer_for_action_direct_builder_call(monkeypatch) -> None:
    monkeypatch.delenv("GREENFLOOR_OFFER_BUILDER_CMD", raising=False)
    captured = {}

    def _fake_build_offer(payload):
        captured["payload"] = payload
        return f"offer1direct-{payload['size_base_units']}"

    monkeypatch.setattr(
        "greenfloor.cli.offer_builder_sdk.build_offer",
        _fake_build_offer,
    )
    action = PlannedAction(
        size=10,
        repeat=1,
        pair="xch",
        expiry_unit="minutes",
        expiry_value=65,
        cancel_after_create=True,
        reason="below_target",
    )

    built = daemon_main._build_offer_for_action(
        market=_market(),
        action=action,
        xch_price_usd=31.5,
        network="mainnet",
        keyring_yaml_path="/tmp/keyring.yaml",
    )

    assert built["status"] == "executed"
    assert built["reason"] == "offer_builder_success"
    assert built["offer"] == "offer1direct-10"
    assert captured["payload"]["quote_price_quote_per_base"] == 0.5
    assert captured["payload"]["base_unit_mojo_multiplier"] == 1000
    assert captured["payload"]["quote_unit_mojo_multiplier"] == 1000
    assert captured["payload"]["key_id"] == "key-main-1"
    assert captured["payload"]["network"] == "mainnet"
    assert captured["payload"]["keyring_yaml_path"] == "/tmp/keyring.yaml"
