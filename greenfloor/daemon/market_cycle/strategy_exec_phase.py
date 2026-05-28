"""Strategy execution: audit, dispatch, health reporting."""

from __future__ import annotations

from datetime import datetime
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.core.strategy import PlannedAction
from greenfloor.daemon.cooldowns import _managed_offer_market_health_payload
from greenfloor.daemon.market_helpers import _normalize_offer_side
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.daemon.market_cycle.result import MarketCycleResult
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.strategy_dispatch import _execute_strategy_actions
from greenfloor.storage.sqlite import SqliteStore


def execute_strategy_for_market(
    *,
    market: Any,
    program: Any,
    strategy_actions: list[PlannedAction],
    dexie: DexieAdapter,
    splash: SplashAdapter,
    store: SqliteStore,
    xch_price_usd: float | None,
    now: datetime,
    result: MarketCycleResult,
    reservation_coordinator: AssetReservationCoordinator | None = None,
) -> dict[str, Any]:
    store.add_audit_event(
        "strategy_actions_planned",
        {
            "market_id": market.market_id,
            "xch_price_usd": xch_price_usd,
            "actions": [
                {
                    "size": action.size,
                    "repeat": action.repeat,
                    "pair": action.pair,
                    "expiry_unit": action.expiry_unit,
                    "expiry_value": action.expiry_value,
                    "cancel_after_create": action.cancel_after_create,
                    "reason": action.reason,
                    "target_spread_bps": action.target_spread_bps,
                    "side": _normalize_offer_side(getattr(action, "side", "sell")),
                }
                for action in strategy_actions
            ],
        },
        market_id=market.market_id,
    )
    offer_execution = _execute_strategy_actions(
        market=market,
        strategy_actions=strategy_actions,
        runtime_dry_run=program.runtime_dry_run,
        xch_price_usd=xch_price_usd,
        dexie=dexie,
        splash=splash,
        publish_venue=program.offer_publish_venue,
        store=store,
        app_network=program.app_network,
        signer_key_registry=program.signer_key_registry,
        program=program,
        reservation_coordinator=reservation_coordinator,
    )
    result.merge_strategy_execution(
        planned=int(offer_execution["planned_count"]),
        executed=int(offer_execution["executed_count"]),
    )
    _log_market_decision(
        market.market_id,
        "strategy_executed",
        planned_count=offer_execution["planned_count"],
        executed_count=offer_execution["executed_count"],
    )
    store.add_audit_event(
        "strategy_offer_execution",
        {
            "market_id": market.market_id,
            "planned_count": offer_execution["planned_count"],
            "executed_count": offer_execution["executed_count"],
            "items": offer_execution["items"],
        },
        market_id=market.market_id,
    )
    health_payload = _managed_offer_market_health_payload(
        market_id=str(market.market_id),
        current_items=list(offer_execution["items"]),
        now=now,
    )
    store.add_audit_event(
        "managed_offer_market_health",
        health_payload,
        market_id=market.market_id,
    )
    return offer_execution
