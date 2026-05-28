"""Market cycle setup: signer selection, alerts, policy snapshot."""

from __future__ import annotations

import logging
from datetime import datetime
from typing import Any

from greenfloor.core.notifications import AlertState, evaluate_low_inventory_alert
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.daemon.market_cycle.result import log_daemon_event
from greenfloor.keys.router import resolve_market_key
from greenfloor.notify.pushover import send_pushover_alert
from greenfloor.storage.sqlite import SqliteStore, StoredAlertState


def run_market_cycle_setup(
    *,
    market: Any,
    program: Any,
    allowed_keys: set[str] | None,
    store: SqliteStore,
    now: datetime,
) -> Any:
    _log_market_decision(
        market.market_id,
        "cycle_start",
        mode=str(getattr(market, "mode", "")),
        quote_asset=str(getattr(market, "quote_asset", "")),
    )
    signer_selection = resolve_market_key(
        market,
        allowed_keys,
        signer_key_registry=program.signer_key_registry,
        required_network=program.app_network,
    )
    _log_market_decision(
        market.market_id,
        "signer_selected",
        key_id=signer_selection.key_id,
        network=program.app_network,
    )
    store.add_price_policy_snapshot(
        market.market_id,
        {
            "mode": market.mode,
            "base_asset": market.base_asset,
            "quote_asset": market.quote_asset,
            "quote_asset_type": market.quote_asset_type,
        },
        source="startup",
    )
    persisted = store.get_alert_state(market.market_id)
    state, event = evaluate_low_inventory_alert(
        now=now,
        program=program,
        market=market,
        state=AlertState(
            is_low=persisted.is_low,
            last_alert_at=persisted.last_alert_at,
        ),
    )
    store.upsert_alert_state(
        StoredAlertState(
            market_id=market.market_id,
            is_low=state.is_low,
            last_alert_at=state.last_alert_at,
        )
    )
    if event:
        payload = {
            "event": "low_inventory_alert",
            "market_id": event.market_id,
            "ticker": event.ticker,
            "remaining_amount": event.remaining_amount,
            "receive_address": event.receive_address,
            "reason": event.reason,
        }
        log_daemon_event(level=logging.INFO, payload=payload)
        store.add_audit_event("low_inventory_alert", payload, market_id=market.market_id)
        send_pushover_alert(program, event)
    return signer_selection
