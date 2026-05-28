"""Structured market decision logging for the daemon."""

from __future__ import annotations

import logging
from typing import Any

from greenfloor.core.strategy_action_item import StrategyActionItem

_daemon_logger = logging.getLogger("greenfloor.daemon")


def _log_market_decision(market_id: str, decision: str, **fields: Any) -> None:
    extras = " ".join(f"{key}={fields[key]}" for key in sorted(fields))
    if extras:
        _daemon_logger.info(
            "market_decision market_id=%s decision=%s %s", market_id, decision, extras
        )
    else:
        _daemon_logger.info("market_decision market_id=%s decision=%s", market_id, decision)


def _log_offer_action_timing(market_id: str, item: StrategyActionItem) -> None:
    audit = item.to_audit_dict()
    if any(
        audit.get(k) is not None for k in ("offer_create_ms", "offer_publish_ms", "offer_total_ms")
    ):
        _log_market_decision(
            market_id,
            "offer_action_timing",
            size=int(audit.get("size", 0) or 0),
            side=str(audit.get("side", "sell")),
            status=str(audit.get("status", "")),
            reason=str(audit.get("reason", "")),
            offer_id=str(audit.get("offer_id", "") or ""),
            offer_create_ms=audit.get("offer_create_ms"),
            offer_publish_ms=audit.get("offer_publish_ms"),
            offer_total_ms=audit.get("offer_total_ms"),
            offer_create_phase_ms=audit.get("offer_create_phase_ms"),
            offer_artifact_wait_ms=audit.get("offer_artifact_wait_ms"),
        )
