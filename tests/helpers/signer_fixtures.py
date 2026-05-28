"""Load and compare golden signer fixture JSON."""

from __future__ import annotations

from dataclasses import replace
from pathlib import Path
from typing import TypedDict

from greenfloor.config.models import MarketConfig
from greenfloor.core.signer_offer_request import (
    COMPARABLE_RUNTIME_REQUEST_FIELDS,
    SignerCreateOfferPayload,
)
from tests.helpers.config_fixtures import minimal_market_config

SIGNER_FIXTURE_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "signer"


class SignerFixtureRuntimeParity(TypedDict):
    action_side: str
    resolved_base_asset_id: str
    resolved_quote_asset_id: str
    size_base_units: int
    quote_price: float
    base_unit_mojo_multiplier: int
    quote_unit_mojo_multiplier: int


_RUNTIME_PARITY_KEYS = (
    "action_side",
    "resolved_base_asset_id",
    "resolved_quote_asset_id",
    "size_base_units",
    "quote_price",
    "base_unit_mojo_multiplier",
    "quote_unit_mojo_multiplier",
)

_IO_BOUNDARY_REQUEST_KEYS = (
    "offer_coin_ids",
    "presplit_coin_ids",
)


def _require_mapping(raw: object, *, label: str) -> dict[str, object]:
    if not isinstance(raw, dict):
        raise AssertionError(f"{label} must be an object")
    return raw


def _require_str(raw: object, *, label: str) -> str:
    if not isinstance(raw, str) or not raw.strip():
        raise AssertionError(f"{label} must be a non-empty string")
    return raw.strip()


def _require_int(raw: object, *, label: str) -> int:
    if isinstance(raw, bool) or not isinstance(raw, int):
        raise AssertionError(f"{label} must be an integer")
    return raw


def _require_float(raw: object, *, label: str) -> float:
    if isinstance(raw, bool) or not isinstance(raw, int | float):
        raise AssertionError(f"{label} must be a number")
    return float(raw)


def _require_bool(raw: object, *, label: str) -> bool:
    if not isinstance(raw, bool):
        raise AssertionError(f"{label} must be a boolean")
    return raw


def _require_str_list(raw: object, *, label: str) -> list[str]:
    if not isinstance(raw, list):
        raise AssertionError(f"{label} must be a list")
    return [str(item) for item in raw]


def parse_runtime_parity(raw: object) -> SignerFixtureRuntimeParity:
    payload = _require_mapping(raw, label="runtime_parity")
    for key in _RUNTIME_PARITY_KEYS:
        if key not in payload:
            raise AssertionError(f"runtime_parity missing key: {key}")
    return SignerFixtureRuntimeParity(
        action_side=_require_str(payload["action_side"], label="runtime_parity.action_side"),
        resolved_base_asset_id=_require_str(
            payload["resolved_base_asset_id"],
            label="runtime_parity.resolved_base_asset_id",
        ),
        resolved_quote_asset_id=_require_str(
            payload["resolved_quote_asset_id"],
            label="runtime_parity.resolved_quote_asset_id",
        ),
        size_base_units=_require_int(
            payload["size_base_units"],
            label="runtime_parity.size_base_units",
        ),
        quote_price=_require_float(payload["quote_price"], label="runtime_parity.quote_price"),
        base_unit_mojo_multiplier=_require_int(
            payload["base_unit_mojo_multiplier"],
            label="runtime_parity.base_unit_mojo_multiplier",
        ),
        quote_unit_mojo_multiplier=_require_int(
            payload["quote_unit_mojo_multiplier"],
            label="runtime_parity.quote_unit_mojo_multiplier",
        ),
    )


def parse_create_offer_request(raw: object) -> tuple[SignerCreateOfferPayload, int | None]:
    payload = _require_mapping(raw, label="create_offer_request")
    required_keys = (
        "receive_address",
        "offer_asset_id",
        "offer_amount",
        "request_asset_id",
        "request_amount",
        "split_input_coins",
        "broadcast_split",
        *_IO_BOUNDARY_REQUEST_KEYS,
    )
    for key in required_keys:
        if key not in payload:
            raise AssertionError(f"create_offer_request missing key: {key}")
    expires_at_unix: int | None = None
    if "expires_at" in payload and payload["expires_at"] is not None:
        expires_at_unix = _require_int(
            payload["expires_at"],
            label="create_offer_request.expires_at",
        )
    return (
        SignerCreateOfferPayload(
            receive_address=_require_str(
                payload["receive_address"],
                label="create_offer_request.receive_address",
            ),
            offer_asset_id=_require_str(
                payload["offer_asset_id"],
                label="create_offer_request.offer_asset_id",
            ),
            offer_amount=_require_int(
                payload["offer_amount"],
                label="create_offer_request.offer_amount",
            ),
            request_asset_id=_require_str(
                payload["request_asset_id"],
                label="create_offer_request.request_asset_id",
            ),
            request_amount=_require_int(
                payload["request_amount"],
                label="create_offer_request.request_amount",
            ),
            offer_coin_ids=_require_str_list(
                payload["offer_coin_ids"],
                label="create_offer_request.offer_coin_ids",
            ),
            presplit_coin_ids=_require_str_list(
                payload["presplit_coin_ids"],
                label="create_offer_request.presplit_coin_ids",
            ),
            split_input_coins=_require_bool(
                payload["split_input_coins"],
                label="create_offer_request.split_input_coins",
            ),
            broadcast_split=_require_bool(
                payload["broadcast_split"],
                label="create_offer_request.broadcast_split",
            ),
            expires_at=expires_at_unix,
        ),
        expires_at_unix,
    )


def _normalize_comparable_value(key: str, value: object) -> object:
    if key.endswith("_asset_id"):
        return str(value).strip().lower().removeprefix("0x")
    return value


def comparable_runtime_payload(payload: SignerCreateOfferPayload) -> dict[str, object]:
    return {
        key: _normalize_comparable_value(key, payload[key])
        for key in COMPARABLE_RUNTIME_REQUEST_FIELDS
    }


def comparable_fixture_runtime_fields(
    request: SignerCreateOfferPayload,
    *,
    expires_at_unix: int | None,
) -> dict[str, object]:
    comparable = {
        key: _normalize_comparable_value(key, request[key])
        for key in COMPARABLE_RUNTIME_REQUEST_FIELDS
        if key != "expires_at"
    }
    comparable["expires_at"] = expires_at_unix
    return comparable


def market_config_from_fixture(
    *,
    create_offer_request: SignerCreateOfferPayload,
    parity: SignerFixtureRuntimeParity,
) -> MarketConfig:
    return replace(
        minimal_market_config(),
        receive_address=create_offer_request["receive_address"],
        pricing={
            "base_unit_mojo_multiplier": parity["base_unit_mojo_multiplier"],
            "quote_unit_mojo_multiplier": parity["quote_unit_mojo_multiplier"],
        },
    )
