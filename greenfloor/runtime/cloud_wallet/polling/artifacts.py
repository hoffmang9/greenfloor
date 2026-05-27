from __future__ import annotations

import collections.abc
import datetime as dt
import time
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.moderate_retry import (
    call_with_moderate_retry,
    poll_with_exponential_backoff_until,
)

from greenfloor.runtime.cloud_wallet.polling.common import (
    parse_iso8601,
    pick_new_offer_artifact,
    wallet_get_wallet_offers,
)


def poll_offer_artifact_until_available(
    *,
    wallet: CloudWalletAdapter,
    known_markers: set[str],
    timeout_seconds: int,
    min_created_at: dt.datetime | None = None,
    require_open_state: bool = False,
    states: tuple[str, ...] | None = ("OPEN", "PENDING"),
    prefer_newest: bool = True,
    wallet_get_wallet_offers_fn: collections.abc.Callable[..., dict[str, Any]] | None = None,
    retry_fn: collections.abc.Callable[..., Any] | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
    monotonic_fn: collections.abc.Callable[[], float] | None = None,
) -> str:
    if wallet_get_wallet_offers_fn is None:
        wallet_get_wallet_offers_fn = wallet_get_wallet_offers
    if retry_fn is None:
        retry_fn = call_with_moderate_retry
    if sleep_fn is None:
        sleep_fn = time.sleep
    if monotonic_fn is None:
        monotonic_fn = time.monotonic

    def _on_tick(elapsed: int) -> str | None:
        wallet_payload = retry_fn(
            action="wallet_get_wallet",
            call=lambda: wallet_get_wallet_offers_fn(
                wallet,
                is_creator=True,
                states=list(states) if states is not None else None,
            ),
            elapsed_seconds=elapsed,
        )
        offers = wallet_payload.get("offers", [])
        if isinstance(offers, list):
            offer_text = pick_new_offer_artifact(
                offers=offers,
                known_markers=known_markers,
                min_created_at=min_created_at,
                require_open_state=require_open_state,
                prefer_newest=prefer_newest,
            )
            if offer_text:
                return offer_text
        return None

    return poll_with_exponential_backoff_until(
        monotonic_fn=monotonic_fn,
        sleep_fn=sleep_fn,
        timeout_seconds=timeout_seconds,
        initial_sleep=2.0,
        max_sleep=20.0,
        sleep_multiplier=1.5,
        on_tick=_on_tick,
        timeout_error="cloud_wallet_offer_artifact_timeout",
    )


def poll_offer_artifact_by_signature_request(
    *,
    wallet: CloudWalletAdapter,
    signature_request_id: str,
    known_markers: set[str],
    timeout_seconds: int,
    min_created_at: dt.datetime | None = None,
    retry_fn: collections.abc.Callable[..., Any] | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
    monotonic_fn: collections.abc.Callable[[], float] | None = None,
) -> str:
    if retry_fn is None:
        retry_fn = call_with_moderate_retry
    if sleep_fn is None:
        sleep_fn = time.sleep
    if monotonic_fn is None:
        monotonic_fn = time.monotonic

    def _on_tick(elapsed: int) -> str | None:
        payload = retry_fn(
            action="wallet_get_signature_request_offer",
            call=lambda: wallet.get_signature_request_offer(
                signature_request_id=signature_request_id
            ),
            elapsed_seconds=elapsed,
        )
        bech32 = str(payload.get("bech32", "")).strip()
        offer_id = str(payload.get("offer_id", "")).strip()
        offer_state = str(payload.get("state", "")).strip().upper()
        created_at = parse_iso8601(str(payload.get("created_at", "")).strip())
        markers = {f"bech32:{bech32}"} if bech32 else set()
        if offer_id:
            markers.add(f"id:{offer_id}")
        markers_already_known = bool(markers) and markers.issubset(known_markers)
        created_at_gte_min = (
            bool(created_at and min_created_at and created_at >= min_created_at)
            if min_created_at is not None
            else True
        )
        if (
            bech32.startswith("offer1")
            and offer_state in {"OPEN", "PENDING", "SETTLED"}
            and not markers_already_known
            and created_at_gte_min
        ):
            return bech32
        return None

    return poll_with_exponential_backoff_until(
        monotonic_fn=monotonic_fn,
        sleep_fn=sleep_fn,
        timeout_seconds=timeout_seconds,
        initial_sleep=2.0,
        max_sleep=20.0,
        sleep_multiplier=1.5,
        on_tick=_on_tick,
        timeout_error="cloud_wallet_offer_artifact_timeout",
    )
