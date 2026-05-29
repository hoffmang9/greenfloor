"""Rust-kernel IO for unified offer-action build."""

from __future__ import annotations

from greenfloor.core.kernel_bridge import import_kernel
from greenfloor.core.offer_action import (
    OfferActionRequest,
    OfferActionResult,
    parse_action_result,
)

__all__ = [
    "build_bls_offer_for_action",
    "build_signer_offer_for_action",
]


def build_signer_offer_for_action(
    config_path: str,
    request: OfferActionRequest,
) -> OfferActionResult:
    kernel = import_kernel()
    result = kernel.build_signer_offer_for_action(str(config_path), dict(request))
    return parse_action_result(result)


def build_bls_offer_for_action(
    *,
    network: str,
    key_id: str,
    request: OfferActionRequest,
) -> OfferActionResult:
    kernel = import_kernel()
    result = kernel.build_bls_offer_for_action_key(str(network), str(key_id), dict(request))
    return parse_action_result(result)
