from __future__ import annotations

from datetime import UTC, datetime, timedelta
from typing import Any, cast

from greenfloor.core.strategy import PlannedAction
from greenfloor.daemon.testing import (
    POST_COOLDOWN_UNTIL,
    build_offer_for_action,
    drop_zero_repeat_strategy_actions,
    execute_strategy_actions,
    expand_planned_actions,
    inject_reseed_action_if_no_active_offers,
    strategy_config_from_market,
    strategy_dispatch,
)
from tests.helpers.config_fixtures import minimal_program_config
from tests.helpers.daemon_test_fixtures import (
    FakeDexie,
    FakeStore,
    execute_local_strategy_actions,
    market_config,
)


def test_execute_strategy_actions_dry_run_plans_without_posting() -> None:
    dexie = FakeDexie(post_result={"success": True, "id": "offer-1"})
    dexie.visible_offer_ids = {"offer-100", "offer-10", "offer-1"}
    store = FakeStore()
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

    result = execute_strategy_actions(
        market=market_config(),
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


def test_expand_planned_actions_preserves_strategy_order() -> None:
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="below_target",
        ),
        PlannedAction(
            size=10,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="below_target",
        ),
    ]

    expanded = expand_planned_actions(actions)

    assert [action.size for action in expanded] == [1, 1, 10, 10]


def test_execute_strategy_actions_skips_when_builder_skips(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()

    monkeypatch.setattr(
        strategy_dispatch,
        "build_offer_for_action",
        lambda **_kwargs: {"status": "skipped", "reason": "builder_not_ready", "offer": None},
    )
    dexie = FakeDexie(post_result={"success": True, "id": "offer-1"})
    dexie.visible_offer_ids = {"offer-100", "offer-10", "offer-1"}
    store = FakeStore()
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

    result = execute_local_strategy_actions(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=32.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        program=minimal_program_config(),
    )

    assert result["planned_count"] == 1
    assert result["executed_count"] == 0
    assert result["items"][0]["status"] == "skipped"
    assert result["items"][0]["reason"] == "builder_not_ready"
    assert dexie.posted == []
    assert store.offer_states == []


def test_execute_strategy_actions_posts_and_persists_offer_ids(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()

    monkeypatch.setattr(
        strategy_dispatch,
        "build_offer_for_action",
        lambda **_kwargs: {
            "status": "executed",
            "reason": "offer_builder_success",
            "offer": "offer1abc",
        },
    )
    dexie = FakeDexie(post_result={"success": True, "id": "offer-123"})
    store = FakeStore()
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

    result = execute_local_strategy_actions(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=32.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        program=minimal_program_config(),
    )

    assert result["planned_count"] == 2
    assert result["executed_count"] == 2
    assert len(dexie.posted) == 2
    assert len(store.offer_states) == 2
    assert all(s["offer_id"] == "offer-123" for s in store.offer_states)
    first_item = result["items"][0]
    assert isinstance(first_item.get("offer_create_ms"), int)
    assert isinstance(first_item.get("offer_publish_ms"), int)
    assert isinstance(first_item.get("offer_total_ms"), int)


def test_execute_strategy_actions_retries_then_succeeds(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "3")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", "10")

    monkeypatch.setattr(
        strategy_dispatch,
        "build_offer_for_action",
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
    store = FakeStore()
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
    result = execute_local_strategy_actions(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        program=minimal_program_config(),
    )
    assert result["executed_count"] == 1
    assert dexie.calls == 2
    assert result["items"][0]["attempts"] == 2


def test_execute_strategy_actions_applies_post_cooldown_after_retry_exhaust(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "2")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", "60")
    monkeypatch.setattr(
        strategy_dispatch,
        "build_offer_for_action",
        lambda **_kwargs: {"status": "executed", "reason": "ok", "offer": "offer1abc"},
    )

    dexie = FakeDexie(post_result={"success": False, "error": "down"})
    store = FakeStore()
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
    result = execute_local_strategy_actions(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        program=minimal_program_config(),
    )
    assert result["executed_count"] == 0
    assert dexie.calls == 2
    assert result["items"][0]["reason"].startswith("dexie_post_retry_exhausted:")
    assert result["items"][1]["reason"].startswith("post_cooldown_active:")


def test_build_offer_for_action_direct_builder_call(monkeypatch) -> None:
    captured = {}

    def _fake_build_offer(payload):
        captured["payload"] = payload
        return f"offer1direct-{payload['size_base_units']}"

    monkeypatch.setattr("greenfloor.offer_builder.build_offer", _fake_build_offer)
    action = PlannedAction(
        size=10,
        repeat=1,
        pair="xch",
        expiry_unit="minutes",
        expiry_value=65,
        cancel_after_create=True,
        reason="below_target",
    )

    built = build_offer_for_action(
        program=minimal_program_config(),
        market=market_config(),
        action=action,
        xch_price_usd=31.5,
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


def test_inject_reseed_action_when_no_active_offers() -> None:
    store = FakeStore()
    store.offer_states = [{"offer_id": "old-1", "market_id": "m1", "state": "expired"}]
    market = market_config()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=datetime.now(UTC),
    )

    assert [action.size for action in actions] == [1, 10, 100]
    assert [action.repeat for action in actions] == [5, 2, 1]
    assert all(action.reason == "offer_size_gap_reseed" for action in actions)


def test_inject_reseed_action_skips_when_offer_targets_are_satisfied() -> None:
    store = FakeStore()
    store.offer_states = [
        *[{"offer_id": f"one-{idx}", "market_id": "m1", "state": "open"} for idx in range(1, 6)],
        *[{"offer_id": f"ten-{idx}", "market_id": "m1", "state": "open"} for idx in range(1, 3)],
        {"offer_id": "hundred-1", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "payload": {
                "items": [
                    {"offer_id": f"one-{idx}", "size": 1, "status": "executed"}
                    for idx in range(1, 6)
                ]
                + [
                    {"offer_id": f"ten-{idx}", "size": 10, "status": "executed"}
                    for idx in range(1, 3)
                ]
                + [{"offer_id": "hundred-1", "size": 100, "status": "executed"}]
            },
        }
    ]
    market = market_config()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=datetime.now(UTC),
    )

    assert actions == []


def test_inject_reseed_action_fills_missing_sizes_when_recent_mempool_is_present() -> None:
    store = FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {
            "offer_id": "mempool-1",
            "market_id": "m1",
            "state": "mempool_observed",
            "updated_at": now.isoformat(),
        }
    ]
    market = market_config()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=now,
    )

    assert [action.size for action in actions] == [1, 10, 100]
    assert [action.repeat for action in actions] == [5, 2, 1]
    assert all(action.reason == "offer_size_gap_reseed" for action in actions)


def test_inject_reseed_action_when_only_mempool_offer_is_stale() -> None:
    store = FakeStore()
    now = datetime.now(UTC)
    stale = now - timedelta(minutes=31)
    store.offer_states = [
        {
            "offer_id": "mempool-old-1",
            "market_id": "m1",
            "state": "mempool_observed",
            "updated_at": stale.isoformat(),
        }
    ]
    market = market_config()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=now,
    )

    assert [action.size for action in actions] == [1, 10, 100]
    assert [action.repeat for action in actions] == [5, 2, 1]
    assert all(action.reason == "offer_size_gap_reseed" for action in actions)


def test_inject_reseed_action_refills_missing_same_size_offers_immediately() -> None:
    store = FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {"offer_id": "one-1", "market_id": "m1", "state": "open"},
        {"offer_id": "one-2", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "created_at": (now - timedelta(seconds=60)).isoformat(),
            "payload": {
                "items": [
                    {"offer_id": "recent-one", "size": 1, "status": "executed"},
                    {"offer_id": "one-1", "size": 1, "status": "executed"},
                    {"offer_id": "one-2", "size": 1, "status": "executed"},
                ]
            },
        }
    ]
    market = market_config()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=now,
    )

    assert [action.size for action in actions] == [1, 10, 100]
    assert [action.repeat for action in actions] == [3, 2, 1]


def test_inject_reseed_action_is_not_limited_by_old_cadence_window() -> None:
    store = FakeStore()
    now = datetime.now(UTC)
    store.offer_states = [
        {"offer_id": "one-1", "market_id": "m1", "state": "open"},
        {"offer_id": "one-2", "market_id": "m1", "state": "open"},
    ]
    store.audit_events = [
        {
            "event_type": "strategy_offer_execution",
            "market_id": "m1",
            "created_at": (now - timedelta(minutes=4)).isoformat(),
            "payload": {
                "items": [
                    {"offer_id": "stale-one", "size": 1, "status": "executed"},
                    {"offer_id": "one-1", "size": 1, "status": "executed"},
                    {"offer_id": "one-2", "size": 1, "status": "executed"},
                ]
            },
        }
    ]
    market = market_config()
    strategy_config = strategy_config_from_market(market)

    actions = inject_reseed_action_if_no_active_offers(
        strategy_actions=[],
        strategy_config=strategy_config,
        market=market,
        store=cast(Any, store),
        xch_price_usd=30.0,
        clock=now,
    )

    assert [action.size for action in actions] == [1, 10, 100]
    assert [action.repeat for action in actions] == [3, 2, 1]


def test_drop_zero_repeat_strategy_actions_preserves_positive_repeats() -> None:
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="below_target",
            side="sell",
        )
    ]

    filtered = drop_zero_repeat_strategy_actions(actions)

    assert filtered == actions


def test_drop_zero_repeat_strategy_actions_filters_zero_repeat_actions() -> None:
    actions = [
        PlannedAction(
            size=1,
            repeat=0,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="below_target",
            side="sell",
        ),
        PlannedAction(
            size=1,
            repeat=1,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="below_target",
            side="sell",
        ),
    ]

    filtered = drop_zero_repeat_strategy_actions(actions)

    assert len(filtered) == 1
    assert filtered[0].repeat == 1
