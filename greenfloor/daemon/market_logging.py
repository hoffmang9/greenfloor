"""Structured market decision logging for the daemon."""

from __future__ import annotations

import logging
from typing import Any

_daemon_logger = logging.getLogger("greenfloor.daemon")


def _log_market_decision(market_id: str, decision: str, **fields: Any) -> None:
    extras = " ".join(f"{key}={fields[key]}" for key in sorted(fields))
    if extras:
        _daemon_logger.info(
            "market_decision market_id=%s decision=%s %s", market_id, decision, extras
        )
    else:
        _daemon_logger.info("market_decision market_id=%s decision=%s", market_id, decision)


def _log_offer_action_timing(market_id: str, item: dict[str, Any]) -> None:
    if any(
        item.get(k) is not None for k in ("offer_create_ms", "offer_publish_ms", "offer_total_ms")
    ):
        _log_market_decision(
            market_id,
            "offer_action_timing",
            size=int(item.get("size", 0) or 0),
            side=str(item.get("side", "sell")),
            status=str(item.get("status", "")),
            reason=str(item.get("reason", "")),
            offer_id=str(item.get("offer_id", "") or ""),
            offer_create_ms=item.get("offer_create_ms"),
            offer_publish_ms=item.get("offer_publish_ms"),
            offer_total_ms=item.get("offer_total_ms"),
            offer_create_phase_ms=item.get("offer_create_phase_ms"),
            offer_artifact_wait_ms=item.get("offer_artifact_wait_ms"),
        )
