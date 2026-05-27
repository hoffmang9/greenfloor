from __future__ import annotations

import collections.abc
import datetime as dt
from dataclasses import dataclass
from typing import Any, Literal

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.hex_utils import default_mojo_multiplier_for_asset
from greenfloor.runtime.cloud_wallet.polling import (
    offer_markers,
    poll_offer_artifact_by_signature_request,
    poll_offer_artifact_until_available,
    poll_signature_request_until_not_unsigned,
    wallet_get_wallet_offers,
)
from greenfloor.runtime.offer_publish import normalize_offer_side

_ARTIFACT_TIMEOUT = "cloud_wallet_offer_artifact_timeout"


@dataclass(frozen=True, slots=True)
class _ArtifactPollStep:
    lookup: Literal["signature", "generic"]
    timeout_seconds: int
    states: tuple[str, ...] | None
    prefer_newest: bool


def cloud_wallet_create_offer_phase(
    *,
    wallet: CloudWalletAdapter,
    market: Any,
    size_base_units: int,
    quote_price: float,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    offer_fee_mojos: int,
    split_input_coins_fee: int,
    expiry_unit: str,
    expiry_value: int,
    action_side: str = "sell",
    signature_wait_timeout_seconds: int = 120,
    signature_wait_warning_interval_seconds: int = 60,
    wallet_get_wallet_offers_fn: collections.abc.Callable[..., dict[str, Any]] | None = None,
    poll_signature_request_until_not_unsigned_fn: collections.abc.Callable[..., Any] | None = None,
) -> dict[str, Any]:
    if wallet_get_wallet_offers_fn is None:
        wallet_get_wallet_offers_fn = wallet_get_wallet_offers
    if poll_signature_request_until_not_unsigned_fn is None:
        poll_signature_request_until_not_unsigned_fn = poll_signature_request_until_not_unsigned
    side = normalize_offer_side(action_side)
    prior_wallet_payload = wallet_get_wallet_offers_fn(
        wallet,
        is_creator=True,
        states=["OPEN", "PENDING"],
    )
    prior_offers = prior_wallet_payload.get("offers", [])
    known_offer_markers = offer_markers(prior_offers if isinstance(prior_offers, list) else [])
    offer_request_started_at = dt.datetime.now(dt.UTC)
    offer_amount = int(
        size_base_units
        * int(
            (market.pricing or {}).get(
                "base_unit_mojo_multiplier",
                default_mojo_multiplier_for_asset(str(resolved_base_asset_id)),
            )
        )
    )
    request_amount = int(
        round(
            float(size_base_units)
            * float(quote_price)
            * int(
                (market.pricing or {}).get(
                    "quote_unit_mojo_multiplier",
                    default_mojo_multiplier_for_asset(str(resolved_quote_asset_id)),
                )
            )
        )
    )
    if request_amount <= 0:
        raise ValueError("request_amount must be positive")
    if side == "buy":
        offered = [{"assetId": resolved_quote_asset_id, "amount": request_amount}]
        requested = [{"assetId": resolved_base_asset_id, "amount": offer_amount}]
    else:
        offered = [{"assetId": resolved_base_asset_id, "amount": offer_amount}]
        requested = [{"assetId": resolved_quote_asset_id, "amount": request_amount}]
    expires_at = (
        dt.datetime.now(dt.UTC) + dt.timedelta(**{expiry_unit: int(expiry_value)})
    ).isoformat()
    create_result = wallet.create_offer(
        offered=offered,
        requested=requested,
        fee=offer_fee_mojos,
        expires_at_iso=expires_at,
        split_input_coins=False,
        split_input_coins_fee=0,
    )
    signature_request_id = str(create_result.get("signature_request_id", "")).strip()
    wait_events: list[dict[str, str]] = []
    signature_state = str(create_result.get("status", "UNKNOWN")).strip()
    if signature_request_id:
        signature_state, signature_wait_events = poll_signature_request_until_not_unsigned_fn(
            wallet=wallet,
            signature_request_id=signature_request_id,
            timeout_seconds=max(5, int(signature_wait_timeout_seconds)),
            warning_interval_seconds=max(5, int(signature_wait_warning_interval_seconds)),
        )
        wait_events.extend(signature_wait_events)
    return {
        "known_offer_markers": known_offer_markers,
        "offer_request_started_at": offer_request_started_at,
        "signature_request_id": signature_request_id,
        "signature_state": signature_state,
        "wait_events": wait_events,
        "expires_at": expires_at,
        "offer_amount": offer_amount,
        "request_amount": request_amount,
        "side": side,
    }


def _artifact_poll_steps(
    *, signature_request_id: str, strict_timeout: int
) -> list[_ArtifactPollStep]:
    extended_timeout = max(45, strict_timeout * 3)
    steps: list[_ArtifactPollStep] = []
    if signature_request_id:
        steps.append(_ArtifactPollStep("signature", strict_timeout, None, True))
    else:
        steps.append(_ArtifactPollStep("generic", strict_timeout, ("OPEN", "PENDING"), True))
    if signature_request_id:
        steps.append(_ArtifactPollStep("signature", extended_timeout, None, True))
    steps.extend(
        [
            _ArtifactPollStep("generic", extended_timeout, ("OPEN", "PENDING"), True),
            _ArtifactPollStep("generic", 15, None, False),
        ]
    )
    return steps


def cloud_wallet_wait_offer_artifact_phase(
    *,
    wallet: CloudWalletAdapter,
    known_markers: set[str],
    offer_request_started_at: dt.datetime,
    signature_request_id: str = "",
    timeout_seconds: int = 15 * 60,
    poll_offer_artifact_until_available_fn: collections.abc.Callable[..., str] | None = None,
    poll_offer_artifact_by_signature_request_fn: collections.abc.Callable[..., str] | None = None,
) -> str:
    if poll_offer_artifact_until_available_fn is None:
        poll_offer_artifact_until_available_fn = poll_offer_artifact_until_available
    if poll_offer_artifact_by_signature_request_fn is None:
        poll_offer_artifact_by_signature_request_fn = poll_offer_artifact_by_signature_request
    strict_timeout = max(15, int(timeout_seconds))
    for step in _artifact_poll_steps(
        signature_request_id=signature_request_id,
        strict_timeout=strict_timeout,
    ):
        try:
            if step.lookup == "signature":
                return poll_offer_artifact_by_signature_request_fn(
                    wallet=wallet,
                    signature_request_id=signature_request_id,
                    known_markers=known_markers,
                    timeout_seconds=step.timeout_seconds,
                    min_created_at=offer_request_started_at,
                )
            return poll_offer_artifact_until_available_fn(
                wallet=wallet,
                known_markers=known_markers,
                timeout_seconds=step.timeout_seconds,
                min_created_at=offer_request_started_at,
                require_open_state=False,
                states=step.states,
                prefer_newest=step.prefer_newest,
            )
        except RuntimeError as exc:
            if str(exc) != _ARTIFACT_TIMEOUT:
                raise
    raise RuntimeError(_ARTIFACT_TIMEOUT)
