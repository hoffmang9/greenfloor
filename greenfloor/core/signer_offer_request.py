"""Signer create-offer request types and builders (no IO).

Leg math and asset normalization live in the Rust kernel; Python reaches them via
``greenfloor.core.offer_request_bridge``. This module owns dataclasses and request assembly.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, TypedDict

from greenfloor.core.offer_request_bridge import (
    compute_signer_offer_leg_amounts,
    normalize_offer_asset_id,
    quote_mojos_for_base_size,
    signer_split_asset_id,
)
from greenfloor.core.policy_bridge import mojo_multiplier_for_leg

__all__ = [
    "COMPARABLE_RUNTIME_REQUEST_FIELDS",
    "SignerCreateOfferPayload",
    "SignerCreateOfferRequest",
    "SignerOfferLegAmounts",
    "compute_signer_offer_leg_amounts",
    "normalize_offer_asset_id",
    "quote_mojos_for_base_size",
    "resolve_quote_unit_multiplier",
    "signer_create_offer_request_from_fields",
    "signer_split_asset_id",
]


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
    return SignerCreateOfferRequest(
        receive_address=str(receive_address).strip(),
        offer_asset_id=normalize_offer_asset_id(str(offer_asset_id)),
        offer_amount=int(offer_amount),
        request_asset_id=normalize_offer_asset_id(str(request_asset_id)),
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
