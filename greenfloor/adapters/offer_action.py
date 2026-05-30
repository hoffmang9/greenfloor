"""Rust-engine IO for unified offer-action build."""

from __future__ import annotations

from greenfloor.core.engine_bridge import import_engine
from greenfloor.core.offer_action import (
    OfferActionRequest,
    OfferActionResult,
    parse_action_result,
)

__all__ = [
    "build_signer_offer_for_action",
]


def build_signer_offer_for_action(
    config_path: str,
    request: OfferActionRequest,
) -> OfferActionResult:
    engine = import_engine()
    result = engine.build_signer_offer_for_action(str(config_path), dict(request))
    return parse_action_result(result)
