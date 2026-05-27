from __future__ import annotations

from typing import Any, cast

from greenfloor.config.models import ProgramConfig
from greenfloor.core.strategy import PlannedAction
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.testing import (
    POST_COOLDOWN_UNTIL,
    coinset_spendable_base_unit_coin_amounts,
    cooldown_remaining_ms,
    execute_strategy_actions,
    inventory_scan,
    single_input_preferred_skip_reason,
    strategy_dispatch,
)
from greenfloor.runtime.coin_ops.planning import select_spendable_coins_for_target_amount
from tests.helpers.daemon_test_fixtures import (
    FakeDexie,
    FakeStore,
    market_config,
    signer_program_config,
)


def test_execute_strategy_actions_parallel_sets_post_cooldown_on_transient_worker_failures(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "1")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_BACKOFF_MS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", "60")

    class _FakeManagedSigner:
        def list_coins(self, *, asset_id: str | None = None, include_pending: bool = True):
            _ = asset_id, include_pending
            return [{"amount": 20_000, "state": "SETTLED", "asset": {"id": "asset_global"}}]

    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"amount": 5000, "state": "CONFIRMED", "id": "coin-base", "name": "coin-base"},
            {"amount": 10_000_000, "state": "CONFIRMED", "id": "coin-xch", "name": "coin-xch"},
        ],
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_resolve_signer_offer_asset_ids_for_reservation",
        lambda **_kwargs: ("asset_global", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_execute_single_managed_action",
        lambda **_kwargs: (_ for _ in ()).throw(TimeoutError("The read operation timed out")),
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config(runtime_offer_parallelism_enabled=True)

    market = market_config()
    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = FakeDexie(post_result={"success": True, "id": "unused"})
    store = FakeStore()
    actions = [
        PlannedAction(
            size=1,
            repeat=2,
            pair="usdc",
            expiry_unit="minutes",
            expiry_value=10,
            cancel_after_create=True,
            reason="no_active_offer_reseed",
            side="sell",
        )
    ]
    result = execute_strategy_actions(
        market=market,
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=program_factory(),
        reservation_coordinator=coordinator,
    )
    assert result["executed_count"] == 0
    assert all(item.get("transient_upstream") is True for item in result["items"])
    remaining_ms = cooldown_remaining_ms(
        POST_COOLDOWN_UNTIL,
        f"dexie:{market.market_id}",
    )
    assert remaining_ms > 0


def test_execute_strategy_actions_signer_managed_nonparallel_converts_worker_exception_to_skip(
    monkeypatch,
) -> None:
    def program_factory() -> ProgramConfig:
        return signer_program_config(runtime_offer_parallelism_enabled=False)

    dexie = FakeDexie(post_result={"success": True, "id": "unused"})
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
            side="sell",
        )
    ]
    monkeypatch.setattr(
        strategy_dispatch,
        "_execute_single_managed_action",
        lambda **_kwargs: (_ for _ in ()).throw(TimeoutError("The read operation timed out")),
    )

    result = execute_strategy_actions(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=program_factory(),
        reservation_coordinator=None,
    )
    assert result["executed_count"] == 0
    assert len(result["items"]) == 1
    assert str(result["items"][0]["reason"]).startswith("managed_action_error:")


def test_execute_strategy_actions_parallel_prefers_single_input_offer(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(
            self, *, asset_id: str | None = None, include_pending: bool = True
        ) -> list[dict[str, Any]]:
            _ = include_pending
            if asset_id == "asset_global":
                return [
                    {
                        "id": "c1",
                        "amount": 600,
                        "state": "SETTLED",
                        "asset": {"id": "asset_global"},
                    },
                    {
                        "id": "c2",
                        "amount": 600,
                        "state": "SETTLED",
                        "asset": {"id": "asset_global"},
                    },
                ]
            if asset_id == "xch_asset":
                return [
                    {"id": "x1", "amount": 1000, "state": "SETTLED", "asset": {"id": "xch_asset"}}
                ]
            return []

    monkeypatch.setattr(
        strategy_dispatch,
        "_resolve_signer_offer_asset_ids_for_reservation",
        lambda **_kwargs: ("asset_global", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-should-not-post"},
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config(runtime_offer_parallelism_enabled=True)

    market = market_config()
    market.base_asset = "asset-local-only"
    market.pricing = {"fixed_quote_per_base": 1.0, "base_unit_mojo_multiplier": 1000}
    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = FakeDexie(post_result={"success": True, "id": "offer-should-not-post"})
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
            side="sell",
        )
    ]

    def _list_unspent_coins(**kwargs: Any) -> list[dict[str, Any]]:
        asset_id = str(kwargs.get("asset_id", "")).strip()
        if asset_id == "asset_global":
            return [
                {"amount": 600, "state": "CONFIRMED", "id": "c1", "name": "c1"},
                {"amount": 600, "state": "CONFIRMED", "id": "c2", "name": "c2"},
            ]
        if asset_id == "xch_asset":
            return [{"amount": 1000, "state": "CONFIRMED", "id": "x1", "name": "x1"}]
        return []

    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        _list_unspent_coins,
    )
    result = execute_strategy_actions(
        market=market,
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=program_factory(),
        reservation_coordinator=coordinator,
    )
    assert result["executed_count"] == 0
    assert any(
        "single_input_preferred_requires_combine" in str(item["reason"]) for item in result["items"]
    )


def test_coinset_spendable_base_unit_coin_amounts_filters_and_converts(monkeypatch) -> None:
    monkeypatch.setattr(
        inventory_scan,
        "list_unspent_coins_by_receive_address",
        lambda **_kwargs: [
            {"amount": 10000, "state": "CONFIRMED"},
            {"amount": 999, "state": "CONFIRMED"},
            {"amount": 20000, "state": "PENDING"},
        ],
    )
    got = coinset_spendable_base_unit_coin_amounts(
        program=signer_program_config(),
        market=market_config(),
        resolved_asset_id="Asset_byc",
        base_unit_mojo_multiplier=1000,
    )
    assert got == [10]


def test_single_input_preferred_skip_reason_ignores_unknown_max_single() -> None:
    reason = single_input_preferred_skip_reason(
        requested_amounts={"Asset_byc": 10000},
        spendable_profiles={
            "Asset_byc": {
                "total": 77000,
                "max_single": 0,
                "coin_count": 0,
                "max_single_known": 0,
            }
        },
    )
    assert reason is None


def test_select_spendable_coins_for_target_amount_prefers_exact() -> None:
    coins = [
        {"id": "c5", "amount": 5000},
        {"id": "c3", "amount": 3000},
        {"id": "c2", "amount": 2000},
        {"id": "c3b", "amount": 3000},
    ]
    coin_ids, total, exact = select_spendable_coins_for_target_amount(
        coins=coins,
        target_amount=10_000,
    )
    assert exact is True
    assert total == 10_000
    assert set(coin_ids) == {"c5", "c3", "c2"}


def test_select_spendable_coins_for_target_amount_uses_change_when_needed() -> None:
    coins = [
        {"id": "c5", "amount": 5000},
        {"id": "c3a", "amount": 3000},
        {"id": "c3b", "amount": 3000},
    ]
    coin_ids, total, exact = select_spendable_coins_for_target_amount(
        coins=coins,
        target_amount=10_000,
    )
    assert exact is False
    assert total == 11_000
    assert set(coin_ids) == {"c5", "c3a", "c3b"}
