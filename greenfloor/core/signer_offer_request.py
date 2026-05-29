"""Deterministic signer ``create_offer`` request construction (no IO)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, TypedDict

from greenfloor.config.models import MarketConfig
from greenfloor.core.kernel_bridge import import_kernel
from greenfloor.core.policy_bridge import mojo_multiplier_for_leg

_KERNEL_REBUILD_HINT = (
    "greenfloor_signer extension is missing required offer-request symbols. "
    "Rebuild it (for example: `maturin develop --manifest-path "
    "greenfloor-signer-pyo3/Cargo.toml`)."
)


def _kernel():
    return import_kernel()


def _require_kernel_method(method_name: str):
    method = getattr(_kernel(), method_name, None)
    if method is None:
        raise RuntimeError(f"{_KERNEL_REBUILD_HINT} Missing symbol: {method_name}")
    return method


class SignerCreateOfferPayload(TypedDict):
    receive_address: str
    offer_asset_id: str
    offer_amount: int
    request_asset_id: str
    request_amount: int
    offer_coin_ids: list[str]
    presplit_coin_ids: list[str]
    split_input_coins: bool
    broadcast_split: bool
    expires_at: int | None


COMPARABLE_RUNTIME_REQUEST_FIELDS = (
    "receive_address",
    "offer_asset_id",
    "request_asset_id",
    "offer_amount",
    "request_amount",
    "split_input_coins",
    "broadcast_split",
    "expires_at",
)


@dataclass(frozen=True, slots=True)
class SignerOfferLegAmounts:
    offer_asset_id: str
    request_asset_id: str
    offer_amount_mojos: int
    request_amount_mojos: int


@dataclass(frozen=True, slots=True)
class SignerCreateOfferRequest:
    receive_address: str
    offer_asset_id: str
    offer_amount: int
    request_asset_id: str
    request_amount: int
    offer_coin_ids: tuple[str, ...] = ()
    presplit_coin_ids: tuple[str, ...] = ()
    split_input_coins: bool = True
    broadcast_split: bool = True
    expires_at: int | None = None

    def to_payload(self) -> SignerCreateOfferPayload:
        return {
            "receive_address": self.receive_address,
            "offer_asset_id": self.offer_asset_id,
            "offer_amount": int(self.offer_amount),
            "request_asset_id": self.request_asset_id,
            "request_amount": int(self.request_amount),
            "offer_coin_ids": list(self.offer_coin_ids),
            "presplit_coin_ids": list(self.presplit_coin_ids),
            "split_input_coins": bool(self.split_input_coins),
            "broadcast_split": bool(self.broadcast_split),
            "expires_at": self.expires_at,
        }


def _leg_amounts_from_kernel(payload: object) -> SignerOfferLegAmounts:
    if not isinstance(payload, dict):
        raise TypeError("compute_signer_offer_leg_amounts must return dict payload")
    return SignerOfferLegAmounts(
        offer_asset_id=str(payload["offer_asset_id"]),
        request_asset_id=str(payload["request_asset_id"]),
        offer_amount_mojos=int(payload["offer_amount_mojos"]),
        request_amount_mojos=int(payload["request_amount_mojos"]),
    )


def resolve_quote_unit_multiplier(
    *,
    pricing: dict[str, Any],
    resolved_quote_asset_id: str,
) -> int:
    return int(
        mojo_multiplier_for_leg(
            pricing,
            "quote_unit_mojo_multiplier",
            str(resolved_quote_asset_id),
        )
    )


def quote_mojos_for_base_size(
    *,
    size_base_units: int,
    quote_price: float,
    quote_unit_multiplier: int,
) -> int:
    compute = _require_kernel_method("quote_mojos_for_base_size")
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
    resolve = _require_kernel_method("signer_split_asset_id")
    return str(
        resolve(
            str(action_side),
            str(resolved_base_asset_id),
            str(resolved_quote_asset_id),
        )
    )


def compute_signer_offer_leg_amounts(
    *,
    size_base_units: int,
    quote_price: float,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    action_side: str,
    pricing: dict[str, Any],
) -> SignerOfferLegAmounts:
    compute = _require_kernel_method("compute_signer_offer_leg_amounts")
    payload = compute(
        int(size_base_units),
        float(quote_price),
        str(resolved_base_asset_id),
        str(resolved_quote_asset_id),
        str(action_side),
        dict(pricing),
    )
    return _leg_amounts_from_kernel(payload)


def build_signer_create_offer_request(
    *,
    market: MarketConfig,
    size_base_units: int,
    quote_price: float,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    action_side: str = "sell",
    split_input_coins: bool = True,
    broadcast_split: bool = True,
    expires_at_unix: int | None = None,
) -> SignerCreateOfferRequest:
    """Build the request passed to ``rust_signer.build_vault_cat_offer``."""
    pricing = dict(market.pricing or {})
    leg = compute_signer_offer_leg_amounts(
        size_base_units=size_base_units,
        quote_price=quote_price,
        resolved_base_asset_id=resolved_base_asset_id,
        resolved_quote_asset_id=resolved_quote_asset_id,
        action_side=action_side,
        pricing=pricing,
    )

    receive_address = str(market.receive_address or "").strip()
    if not receive_address:
        raise ValueError("market.receive_address is required for signer offer build")

    normalize_asset = _require_kernel_method("normalize_offer_asset_id")
    return SignerCreateOfferRequest(
        receive_address=receive_address,
        offer_asset_id=str(normalize_asset(leg.offer_asset_id)),
        offer_amount=int(leg.offer_amount_mojos),
        request_asset_id=str(normalize_asset(leg.request_asset_id)),
        request_amount=int(leg.request_amount_mojos),
        split_input_coins=bool(split_input_coins),
        broadcast_split=bool(broadcast_split),
        expires_at=expires_at_unix,
    )


def signer_create_offer_request_from_fields(
    *,
    receive_address: str,
    offer_asset_id: str,
    offer_amount: int,
    request_asset_id: str,
    request_amount: int,
    offer_coin_ids: list[str] | tuple[str, ...] = (),
    presplit_coin_ids: list[str] | tuple[str, ...] = (),
    split_input_coins: bool = True,
    broadcast_split: bool = False,
    expires_at: int | None = None,
) -> SignerCreateOfferRequest:
    """Build a signer request from pre-resolved field values."""
    normalize_asset = _require_kernel_method("normalize_offer_asset_id")
    return SignerCreateOfferRequest(
        receive_address=str(receive_address).strip(),
        offer_asset_id=str(normalize_asset(str(offer_asset_id))),
        offer_amount=int(offer_amount),
        request_asset_id=str(normalize_asset(str(request_asset_id))),
        request_amount=int(request_amount),
        offer_coin_ids=tuple(
            str(value).strip().lower() for value in offer_coin_ids if str(value).strip()
        ),
        presplit_coin_ids=tuple(
            str(value).strip().lower() for value in presplit_coin_ids if str(value).strip()
        ),
        split_input_coins=bool(split_input_coins),
        broadcast_split=bool(broadcast_split),
        expires_at=expires_at,
    )
