from __future__ import annotations

import threading
from dataclasses import replace
from datetime import UTC, datetime, timedelta
from pathlib import Path
from types import SimpleNamespace
from typing import Any, cast

import pytest

from greenfloor.config.models import ProgramConfig
from greenfloor.core.strategy import PlannedAction
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.testing import (
    POST_COOLDOWN_UNTIL,
    ReservationContentionError,
    ReservationStorageError,
    execute_strategy_actions,
    inventory_scan,
    strategy_dispatch,
)
from greenfloor.storage.sqlite import SqliteStore
from tests.helpers.daemon_test_fixtures import (
    FakeDexie,
    FakeStore,
    market_config,
    signer_program_config,
)

def test_execute_strategy_actions_parallel_signer_managed_reservation_contention(
    monkeypatch,
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(self, *, include_pending: bool = True):
            _ = include_pending
            return [
                {
                    "amount": 1500,
                    "state": "SPENDABLE",
                    "asset": {"id": "asset"},
                },
                {
                    "amount": 10_000_000,
                    "state": "SPENDABLE",
                    "asset": {"id": "xch_asset"},
                },
            ]

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
        lambda **_kwargs: ("asset", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-parallel"},
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config(runtime_offer_parallelism_enabled=True)

    class _DeterministicContentionCoordinator:
        def __init__(self) -> None:
            self._lock = threading.Lock()
            self.non_empty_acquire_calls = 0
            self.released: list[tuple[str, str]] = []

        def try_acquire(self, **kwargs):
            requested = dict(kwargs.get("requested_amounts", {}) or {})
            if not requested:
                # Daemon health-check path.
                return SimpleNamespace(ok=True, reservation_id="res-health", error=None)
            with self._lock:
                self.non_empty_acquire_calls += 1
                if self.non_empty_acquire_calls == 1:
                    return SimpleNamespace(ok=True, reservation_id="res-1", error=None)
                return SimpleNamespace(
                    ok=False,
                    reservation_id=None,
                    error="reservation_insufficient_asset",
                )

        def release(self, *, reservation_id: str, status: str) -> None:
            self.released.append((str(reservation_id), str(status)))

        def probe_storage(self) -> None:
            return None

    coordinator = _DeterministicContentionCoordinator()
    dexie = FakeDexie(post_result={"success": True, "id": "offer-parallel"})
    dexie.visible_offer_ids = {"offer-parallel"}
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
        )
    ]
    result = execute_strategy_actions(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=program_factory(),
        reservation_coordinator=cast(Any, coordinator),
    )
    assert result["planned_count"] == 2
    assert result["executed_count"] == 1
    assert any("reservation_insufficient_asset" in str(item["reason"]) for item in result["items"])
    assert coordinator.released == [("res-1", "released_success")]


def test_execute_strategy_actions_parallel_releases_reservation_on_failure(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(self, *, include_pending: bool = True):
            _ = include_pending
            return [
                {
                    "amount": 5000,
                    "state": "SPENDABLE",
                    "asset": {"id": "asset"},
                },
                {
                    "amount": 10_000_000,
                    "state": "SPENDABLE",
                    "asset": {"id": "xch_asset"},
                },
            ]

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
        lambda **_kwargs: ("asset", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": False, "error": "vault_unavailable"},
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config(runtime_offer_parallelism_enabled=True)

    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = FakeDexie(post_result={"success": True, "id": "offer-parallel"})
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
    result = execute_strategy_actions(
        market=market_config(),
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
    sqlite_store = SqliteStore(db_path)
    try:
        rows = sqlite_store.list_offer_reservation_leases()
        assert len(rows) == 1
        assert rows[0]["status"] == "released_failed"
    finally:
        sqlite_store.close()


def test_reservation_coordinator_expires_stale_leases(tmp_path) -> None:
    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=30)
    store = SqliteStore(db_path)
    try:
        store.add_offer_reservation_lease(
            reservation_id="res-stale",
            market_id="m1",
            wallet_id="Wallet_abc",
            asset_amounts={"asset": 1000},
            lease_seconds=30,
        )
        rows = store.list_offer_reservation_leases(reservation_id="res-stale")
        assert rows[0]["status"] == "active"
    finally:
        store.close()
    store = SqliteStore(db_path)
    try:
        store.expire_offer_reservation_leases(now=datetime.now(UTC) + timedelta(hours=1))
    finally:
        store.close()
    assert coordinator.expire_stale() == 0
    store = SqliteStore(db_path)
    try:
        rows = store.list_offer_reservation_leases(reservation_id="res-stale")
        assert rows[0]["status"] == "expired"
    finally:
        store.close()


def test_execute_strategy_actions_parallel_does_not_reserve_coin_ops_min_fee(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(self, *, include_pending: bool = True):
            _ = include_pending
            return [
                {"amount": 5000, "state": "SPENDABLE", "asset": {"id": "asset"}},
                {"amount": 10, "state": "SPENDABLE", "asset": {"id": "xch_asset"}},
            ]

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
        lambda **_kwargs: ("asset", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-parallel"},
    )

    def program_factory() -> ProgramConfig:
        return replace(
            signer_program_config(runtime_offer_parallelism_enabled=True),
            coin_ops_minimum_fee_mojos=10,
        )

    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = FakeDexie(post_result={"success": True, "id": "offer-parallel"})
    dexie.visible_offer_ids = {"offer-parallel"}
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
        )
    ]
    result = execute_strategy_actions(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=program_factory(),
        reservation_coordinator=coordinator,
    )
    assert result["executed_count"] == 2
    assert all(
        "reservation_insufficient_xch_asset" not in str(item["reason"]) for item in result["items"]
    )


def test_execute_strategy_actions_parallel_falls_back_to_sequential_on_transient_reservation_error(
    monkeypatch,
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _ContentionCoordinator:
        def probe_storage(self) -> None:
            raise ReservationContentionError("reservation contention on asset lock")

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
        lambda **_kwargs: ("asset", "quote_asset", "xch_asset"),
    )
    monkeypatch.setattr(
        strategy_dispatch,
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-fallback"},
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config(runtime_offer_parallelism_enabled=True)

    dexie = FakeDexie(post_result={"success": True, "id": "offer-fallback"})
    dexie.visible_offer_ids = {"offer-fallback"}
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
    result = execute_strategy_actions(
        market=market_config(),
        strategy_actions=actions,
        runtime_dry_run=False,
        xch_price_usd=30.0,
        dexie=cast(Any, dexie),
        store=cast(Any, store),
        publish_venue="dexie",
        program=program_factory(),
        reservation_coordinator=cast(Any, _ContentionCoordinator()),
    )
    assert result["executed_count"] == 1
    assert any(event["event_type"] == "offer_parallel_fallback" for event in store.audit_events)


def test_execute_strategy_actions_parallel_raises_on_non_transient_reservation_error(
    monkeypatch,
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _BrokenCoordinator:
        def probe_storage(self) -> None:
            raise ReservationStorageError("reservation_storage_down")

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
        lambda **_kwargs: ("asset", "quote_asset", "xch_asset"),
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config(runtime_offer_parallelism_enabled=True)

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
    with pytest.raises(ReservationStorageError, match="reservation_storage_down"):
        execute_strategy_actions(
            market=market_config(),
            strategy_actions=actions,
            runtime_dry_run=False,
            xch_price_usd=30.0,
            dexie=cast(Any, FakeDexie(post_result={"success": True, "id": "offer-fallback"})),
            store=cast(Any, FakeStore()),
            publish_venue="dexie",
            program=program_factory(),
            reservation_coordinator=cast(Any, _BrokenCoordinator()),
        )


def test_execute_strategy_actions_parallel_uses_resolved_asset_ids_for_reservation(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(self, *, include_pending: bool = True):
            _ = include_pending
            return [
                {"amount": 1500, "state": "SPENDABLE", "asset": {"id": "asset_global"}},
                {"amount": 10_000_000, "state": "SPENDABLE", "asset": {"id": "xch_asset"}},
            ]

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
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-resolved-asset"},
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config(runtime_offer_parallelism_enabled=True)

    market = market_config()
    market.base_asset = "asset-local-only"
    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = FakeDexie(post_result={"success": True, "id": "offer-resolved-asset"})
    dexie.visible_offer_ids = {"offer-resolved-asset"}
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
    assert result["executed_count"] == 1


def test_execute_strategy_actions_parallel_uses_asset_scoped_coin_inventory(
    monkeypatch, tmp_path
) -> None:
    POST_COOLDOWN_UNTIL.clear()

    class _FakeManagedSigner:
        def list_coins(
            self, *, asset_id: str | None = None, include_pending: bool = True
        ) -> list[dict[str, Any]]:
            _ = include_pending
            # Simulate the wallet behavior that motivated asset-scoped filtering:
            # a broad unfiltered query reports pending-only inventory.
            if not asset_id:
                return [
                    {
                        "amount": 9_999_999_999_000,
                        "state": "PENDING",
                        "asset": {"id": "xch_asset"},
                    }
                ]
            if asset_id == "asset_global":
                return [{"amount": 1500, "state": "SETTLED", "asset": {"id": "asset_global"}}]
            if asset_id == "xch_asset":
                return [{"amount": 1000, "state": "SETTLED", "asset": {"id": "xch_asset"}}]
            return []

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
        "_managed_offer_post",
        lambda **_kwargs: {"success": True, "offer_id": "offer-scoped"},
    )

    def program_factory() -> ProgramConfig:
        return signer_program_config(runtime_offer_parallelism_enabled=True)

    market = market_config()
    market.base_asset = "asset-local-only"
    db_path = tmp_path / "reservations.sqlite"
    coordinator = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    dexie = FakeDexie(post_result={"success": True, "id": "offer-scoped"})
    dexie.visible_offer_ids = {"offer-scoped"}
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
    assert result["executed_count"] == 1
    assert not any(
        "reservation_insufficient_asset" in str(item["reason"]) for item in result["items"]
    )


def test_reservation_coordinator_cross_instance_contention_allows_single_winner(
    tmp_path,
) -> None:
    db_path = tmp_path / "reservations.sqlite"
    coordinator_a = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    coordinator_b = AssetReservationCoordinator(db_path=db_path, lease_seconds=300)
    barrier = threading.Barrier(2)
    results: list[tuple[bool, str | None]] = []
    results_lock = threading.Lock()

    def _attempt(coordinator: AssetReservationCoordinator) -> None:
        barrier.wait()
        acquired = coordinator.try_acquire(
            market_id="m1",
            wallet_id="wallet-1",
            requested_amounts={"asset": 100},
            available_amounts={"asset": 100},
        )
        with results_lock:
            results.append((acquired.ok, acquired.error))

    thread_a = threading.Thread(target=_attempt, args=(coordinator_a,))
    thread_b = threading.Thread(target=_attempt, args=(coordinator_b,))
    thread_a.start()
    thread_b.start()
    thread_a.join()
    thread_b.join()

    assert len(results) == 2
    success_count = sum(1 for ok, _ in results if ok)
    failure_count = sum(1 for ok, _ in results if not ok)
    assert success_count == 1
    assert failure_count == 1
    assert any("reservation_insufficient_asset" in str(error) for ok, error in results if not ok)
