"""Rust-backed signer offer-request leg math (canonical Python bridge)."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from greenfloor.core.signer_offer_request import SignerOfferLegAmounts

from greenfloor.core import kernel_bridge


def _require_offer_request_method(method_name: str):
    method = getattr(kernel_bridge.policy_kernel(), method_name, None)
    if method is None:
        hint = kernel_bridge.kernel_rebuild_hint(missing="offer-request")
        raise RuntimeError(f"{hint} Missing symbol: {method_name}")
    return method


def _coerce_signer_offer_leg_amounts(payload: object):
    from greenfloor.core.signer_offer_request import SignerOfferLegAmounts

    if isinstance(payload, SignerOfferLegAmounts):
        return payload
    raise TypeError("compute_signer_offer_leg_amounts must return SignerOfferLegAmounts")


def normalize_offer_side(action_side: str) -> str:
    """Normalize to ``buy`` or ``sell``. Fast path for common inputs; kernel for the rest."""
    trimmed = str(action_side or "").strip()
    if not trimmed:
        return "sell"
    lower = trimmed.lower()
    if lower == "buy":
        return "buy"
    if lower == "sell":
        return "sell"
    return str(_require_offer_request_method("normalize_offer_side")(trimmed))


def quote_mojos_for_base_size(
    *,
    size_base_units: int,
    quote_price: float,
    quote_unit_multiplier: int,
) -> int:
    compute = _require_offer_request_method("quote_mojos_for_base_size")
    return int(
        compute(
            int(size_base_units),
            float(quote_price),
            int(quote_unit_multiplier),
        )
    )


def signer_split_asset_id(
    *,
    action_side: str,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
) -> str:
    resolve = _require_offer_request_method("signer_split_asset_id")
    return str(
        resolve(
            str(action_side),
            str(resolved_base_asset_id),
            str(resolved_quote_asset_id),
        )
    )


def normalize_offer_asset_id(asset_id: str) -> str:
    return str(_require_offer_request_method("normalize_offer_asset_id")(str(asset_id)))


def compute_signer_offer_leg_amounts(
    *,
    size_base_units: int,
    quote_price: float,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    action_side: str,
    pricing: dict[str, Any],
) -> SignerOfferLegAmounts:
    compute = _require_offer_request_method("compute_signer_offer_leg_amounts")
    payload = compute(
        int(size_base_units),
        float(quote_price),
        str(resolved_base_asset_id),
        str(resolved_quote_asset_id),
        str(action_side),
        dict(pricing),
    )
    return _coerce_signer_offer_leg_amounts(payload)
