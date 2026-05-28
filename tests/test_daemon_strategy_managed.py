from __future__ import annotations

from typing import Any, cast

from greenfloor.config.models import ProgramConfig
from greenfloor.core.strategy import PlannedAction
from greenfloor.daemon.testing import (
    POST_COOLDOWN_UNTIL,
    execute_strategy_dispatch,
    strategy_dispatch,
)
from tests.helpers.daemon_test_fixtures import (
    FakeDexie,
    FakeStore,
    market_config,
    signer_program_config,
)


def test_execute_strategy_dispatch_uses_signer_managed_path_when_configured(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setattr(
        strategy_dispatch,
        "managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-fallback-1"},
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config()

    dexie = FakeDexie(post_result={"success": True, "id": "offer-1"})
    dexie.visible_offer_ids = {"offer-fallback-1"}
    store = FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
        )
    ]

    result = execute_strategy_dispatch(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=program_factory(),
    )

    assert result.executed_count == 1
    assert result.action_items[0].reason == "managed_offer_post_success"


def test_execute_strategy_dispatch_signer_managed_requires_dexie_visibility(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setattr("time.sleep", lambda _seconds: None)
    monkeypatch.setattr(
        strategy_dispatch,
        "managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-fallback-missing"},
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config()

    class _DexieNon404:
        def get_offer(self, offer_id: str) -> dict[str, Any]:
            _ = offer_id
            raise RuntimeError("dexie_http_error:500")

    store = FakeStore()
    actions = [
        PlannedAction(
            size=100,
            repeat=1,
            pair="usdc",
            expiry_unit="hours",
            expiry_value=8,
            cancel_after_create=True,
            reason="offer_size_gap_reseed",
        )
    ]

    result = execute_strategy_dispatch(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, _DexieNon404()),
        store=cast(Any, store),
        publish_venue="dexie",
        program=program_factory(),
    )

    assert result.executed_count == 0
    assert result.action_items[0].status == "skipped"
    assert "managed_offer_post_not_visible_on_dexie" in result.action_items[0].reason


def test_execute_strategy_dispatch_signer_managed_accepts_transient_dexie_http_404(
    monkeypatch,
) -> None:
    """A transient 404 from Dexie is treated as pending-visibility, not a hard failure.

    The offer is counted as executed so the active-offer reader keeps it in scope
    until the grace period expires.
    """
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setattr("time.sleep", lambda _seconds: None)
    monkeypatch.setattr(
        strategy_dispatch,
        "managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-fallback-pending"},
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config()

    class _Dexie404:
        def get_offer(self, offer_id: str) -> dict[str, Any]:
            _ = offer_id
            raise RuntimeError("HTTP Error 404: Not Found")

    store = FakeStore()
    actions = [
        PlannedAction(
            size=50,
            repeat=1,
            pair="usdc",
            expiry_unit="hours",
            expiry_value=8,
            cancel_after_create=True,
            reason="offer_size_gap_reseed",
        )
    ]

    result = execute_strategy_dispatch(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, _Dexie404()),
        store=cast(Any, store),
        publish_venue="dexie",
        program=program_factory(),
    )

    assert result.executed_count == 1
    assert result.action_items[0].status == "pending_visibility"
    assert result.action_items[0].reason == "managed_offer_post_success"
    assert result.action_items[0].offer_id == "offer-fallback-pending"


def test_execute_strategy_dispatch_preserves_planned_size_order(monkeypatch) -> None:
    POST_COOLDOWN_UNTIL.clear()
    seen_sizes: list[int] = []

    def _fakemanaged_offer_post(**kwargs: Any) -> dict[str, Any]:
        seen_sizes.append(int(kwargs["size_base_units"]))
        size = int(kwargs["size_base_units"])
        return {"success": True, "offer_id": f"offer-{size}"}

    monkeypatch.setattr(strategy_dispatch, "managed_offer_post", _fakemanaged_offer_post)

    def program_factory() -> ProgramConfig:
        return signer_program_config()

    dexie = FakeDexie(post_result={"success": True, "id": "offer-1"})
    dexie.visible_offer_ids = {"offer-100", "offer-10", "offer-1"}
    store = FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="hours",
            expiry_value=8,
            cancel_after_create=True,
            reason="offer_size_gap_reseed",
        ),
        PlannedAction(
            size=10,
            repeat=1,
            pair="usdc",
            expiry_unit="hours",
            expiry_value=8,
            cancel_after_create=True,
            reason="offer_size_gap_reseed",
        ),
        PlannedAction(
            size=100,
            repeat=1,
            pair="usdc",
            expiry_unit="hours",
            expiry_value=8,
            cancel_after_create=True,
            reason="offer_size_gap_reseed",
        ),
    ]

    result = execute_strategy_dispatch(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=program_factory(),
    )

    assert result.executed_count == 3
    assert seen_sizes == [1, 10, 100]


def test_execute_strategy_dispatch_signer_managed_failure_skips_without_builder(
    monkeypatch,
) -> None:
    POST_COOLDOWN_UNTIL.clear()
    calls = {"builder": 0}

    def _unexpected_builder(**_kwargs):
        calls["builder"] += 1
        return {"status": "executed", "reason": "offer_builder_success", "offer": "offer1unused"}

    monkeypatch.setattr(strategy_dispatch, "build_offer_for_action", _unexpected_builder)
    monkeypatch.setattr(
        strategy_dispatch,
        "managed_offer_post",
        lambda **_kwargs: {"success": False, "error": "vault_signing_unavailable"},
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config()

    dexie = FakeDexie(post_result={"success": True, "id": "offer-1"})
    store = FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
        )
    ]

    result = execute_strategy_dispatch(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=program_factory(),
    )

    assert result.executed_count == 0
    assert result.action_items[0].status == "skipped"
    assert result.action_items[0].reason == "managed_offer_post_failed:vault_signing_unavailable"
    assert calls["builder"] == 0
