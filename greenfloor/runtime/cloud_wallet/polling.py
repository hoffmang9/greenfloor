from __future__ import annotations

import collections.abc
import datetime as dt
import sys
import time
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.config.io import is_testnet
from greenfloor.moderate_retry import (
    call_with_moderate_retry,
    poll_with_exponential_backoff_until,
)


def parse_iso8601(value: str) -> dt.datetime | None:
    raw = value.strip()
    if not raw:
        return None
    normalized = raw.replace("Z", "+00:00")
    try:
        parsed = dt.datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=dt.UTC)
    return parsed.astimezone(dt.UTC)


def offer_markers(offers: list[dict]) -> set[str]:
    markers: set[str] = set()
    for offer in offers:
        offer_id = str(offer.get("offerId", "")).strip()
        if offer_id:
            markers.add(f"id:{offer_id}")
        bech32 = str(offer.get("bech32", "")).strip()
        if bech32:
            markers.add(f"bech32:{bech32}")
    return markers


def pick_new_offer_artifact(
    *,
    offers: list[dict],
    known_markers: set[str],
    min_created_at: dt.datetime | None = None,
    require_open_state: bool = False,
    prefer_newest: bool = True,
) -> str:
    candidates: list[tuple[dt.datetime, dt.datetime, str]] = []
    allowed_candidate_states = {"OPEN", "PENDING"}
    for offer in offers:
        state = str(offer.get("state", "")).strip().upper()
        if state not in allowed_candidate_states:
            continue
        if require_open_state and state != "OPEN":
            continue
        bech32 = str(offer.get("bech32", "")).strip()
        if not bech32.startswith("offer1"):
            continue
        offer_id = str(offer.get("offerId", "")).strip()
        markers = {f"bech32:{bech32}"}
        if offer_id:
            markers.add(f"id:{offer_id}")
        if markers.issubset(known_markers):
            continue
        created_at = parse_iso8601(str(offer.get("createdAt", "")).strip())
        if min_created_at is not None:
            if created_at is None or created_at < min_created_at:
                continue
        expires_at = parse_iso8601(str(offer.get("expiresAt", "")).strip())
        candidates.append(
            (
                created_at or dt.datetime.min.replace(tzinfo=dt.UTC),
                expires_at or dt.datetime.min.replace(tzinfo=dt.UTC),
                bech32,
            )
        )
    if not candidates:
        return ""
    candidates.sort(key=lambda row: (row[0], row[1]), reverse=bool(prefer_newest))
    return candidates[0][2]


def wallet_get_wallet_offers(
    wallet: CloudWalletAdapter,
    *,
    is_creator: bool | None,
    states: list[str] | None,
) -> dict[str, Any]:
    return wallet.get_wallet(is_creator=is_creator, states=states, first=100)


def _safe_int(value: object) -> int | None:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return None


def _is_transient_cloud_wallet_list_coins_error(error: str) -> bool:
    normalized = str(error).strip().lower()
    if not normalized:
        return False
    transient_markers = (
        "cloud_wallet_http_error:504",
        "cloud_wallet_http_error:503",
        "cloud_wallet_network_error",
        "http error 504",
        "http error 503",
        "gateway timeout",
        "service unavailable",
        "timed out",
        "timeout",
        "temporary failure",
        "connection reset",
        "connection refused",
        "remote end closed connection",
    )
    return any(marker in normalized for marker in transient_markers)


def _coinset_coin_url(*, coin_name: str, network: str = "mainnet") -> str:
    base = "https://testnet11.coinset.org" if is_testnet(network) else "https://coinset.org"
    return f"{base}/coin/{coin_name.strip()}"


def _coinset_reconcile_coin_state(*, network: str, coin_name: str) -> dict[str, str]:
    adapter = CoinsetAdapter(None, network=network)
    try:
        record = call_with_moderate_retry(
            action="coinset_get_coin_record_by_name",
            call=lambda: adapter.get_coin_record_by_name(coin_name_hex=coin_name),
        )
    except Exception as exc:
        return {"reconcile": "error", "error": str(exc)}
    if not isinstance(record, dict):
        return {"reconcile": "not_found"}
    confirmed_height = _safe_int(record.get("confirmed_block_index"))
    spent_height = _safe_int(record.get("spent_block_index"))
    return {
        "reconcile": "ok",
        "confirmed_block_index": str(confirmed_height if confirmed_height is not None else -1),
        "spent_block_index": str(spent_height if spent_height is not None else -1),
        "coinbase": str(bool(record.get("coinbase", False))).lower(),
    }


def _coinset_peak_height(*, network: str) -> int | None:
    adapter = CoinsetAdapter(None, network=network)
    state = call_with_moderate_retry(
        action="coinset_get_blockchain_state",
        call=adapter.get_blockchain_state,
    )
    if not isinstance(state, dict):
        return None
    candidates = [state.get("peak_height"), state.get("peakHeight")]
    peak = state.get("peak")
    if isinstance(peak, dict):
        candidates.extend([peak.get("height"), peak.get("peak_height")])
    for candidate in candidates:
        parsed = _safe_int(candidate)
        if parsed is not None and parsed >= 0:
            return parsed
    return None


def _watch_reorg_risk_with_coinset(
    *,
    network: str,
    confirmed_block_index: int,
    additional_blocks: int,
    warning_interval_seconds: int,
    timeout_seconds: int = 60 * 60,
) -> list[dict[str, str]]:
    events: list[dict[str, str]] = []
    target_height = int(confirmed_block_index) + int(additional_blocks)
    events.append(
        {
            "event": "reorg_watch_started",
            "confirmed_block_index": str(confirmed_block_index),
            "target_height": str(target_height),
        }
    )
    start = time.monotonic()
    next_warning = warning_interval_seconds
    sleep_seconds = 8.0
    while True:
        elapsed = int(time.monotonic() - start)
        peak_height = _coinset_peak_height(network=network)
        if peak_height is None:
            events.append(
                {
                    "event": "reorg_watch_skipped",
                    "reason": "coinset_peak_height_unavailable",
                    "elapsed_seconds": str(elapsed),
                }
            )
            return events
        remaining = target_height - peak_height
        if remaining <= 0:
            events.append(
                {
                    "event": "reorg_watch_complete",
                    "peak_height": str(peak_height),
                    "target_height": str(target_height),
                    "elapsed_seconds": str(elapsed),
                }
            )
            return events
        if elapsed >= timeout_seconds:
            events.append(
                {
                    "event": "reorg_watch_timeout",
                    "peak_height": str(peak_height),
                    "target_height": str(target_height),
                    "remaining_blocks": str(remaining),
                    "elapsed_seconds": str(elapsed),
                }
            )
            return events
        if elapsed >= next_warning:
            events.append(
                {
                    "event": "reorg_watch_warning",
                    "peak_height": str(peak_height),
                    "target_height": str(target_height),
                    "remaining_blocks": str(remaining),
                    "elapsed_seconds": str(elapsed),
                }
            )
            next_warning += warning_interval_seconds
        time.sleep(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)


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


def poll_signature_request_until_not_unsigned(
    *,
    wallet: CloudWalletAdapter,
    signature_request_id: str,
    timeout_seconds: int,
    warning_interval_seconds: int,
    retry_fn: collections.abc.Callable[..., Any] | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
    monotonic_fn: collections.abc.Callable[[], float] | None = None,
) -> tuple[str, list[dict[str, str]]]:
    if retry_fn is None:
        retry_fn = call_with_moderate_retry
    if sleep_fn is None:
        sleep_fn = time.sleep
    if monotonic_fn is None:
        monotonic_fn = time.monotonic
    events: list[dict[str, str]] = []
    start = monotonic_fn()
    next_warning = warning_interval_seconds
    warning_count = 0
    next_heartbeat = 5
    sleep_seconds = 2.0
    while True:
        elapsed = int(monotonic_fn() - start)
        status_payload = retry_fn(
            action="wallet_get_signature_request",
            call=lambda: wallet.get_signature_request(signature_request_id=signature_request_id),
            elapsed_seconds=elapsed,
            events=events,
        )
        status = str(status_payload.get("status", "")).strip().upper()
        if status and status != "UNSIGNED":
            if next_heartbeat > 5:
                print("", file=sys.stderr, flush=True)
            print(
                f"signature submitted: {signature_request_id} status={status}",
                file=sys.stderr,
                flush=True,
            )
            return status, events
        if elapsed >= next_heartbeat:
            print(".", end="", file=sys.stderr, flush=True)
            next_heartbeat += 5
        if elapsed >= timeout_seconds:
            raise RuntimeError("signature_request_timeout_waiting_for_signature")
        if elapsed >= next_warning:
            warning_count += 1
            events.append(
                {
                    "event": "signature_wait_warning",
                    "elapsed_seconds": str(elapsed),
                    "signing_state_age_seconds": str(elapsed),
                    "message": "still_waiting_on_user_signature",
                    "wait_reason": "waiting_on_user_signature",
                    "warning_count": str(warning_count),
                }
            )
            if warning_count >= 2:
                events.append(
                    {
                        "event": "signature_wait_escalation",
                        "elapsed_seconds": str(elapsed),
                        "message": "extended_user_signature_delay",
                        "wait_reason": "waiting_on_user_signature",
                        "warning_count": str(warning_count),
                    }
                )
            next_warning += warning_interval_seconds
        sleep_fn(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)


def _coin_asset_id(coin: dict) -> str:
    asset_raw = coin.get("asset")
    if isinstance(asset_raw, dict):
        return str(asset_raw.get("id", "xch")).strip() or "xch"
    if isinstance(asset_raw, str):
        return asset_raw.strip() or "xch"
    return "xch"


def wait_for_mempool_then_confirmation(
    *,
    wallet: CloudWalletAdapter,
    network: str,
    initial_coin_ids: set[str],
    asset_id: str | None = None,
    include_pending: bool = False,
    mempool_warning_seconds: int,
    confirmation_warning_seconds: int,
    timeout_seconds: int | None = None,
) -> list[dict[str, str]]:
    events: list[dict[str, str]] = []
    start = time.monotonic()
    seen_pending = False
    next_heartbeat = 5
    sleep_seconds = 2.0
    next_mempool_warning = mempool_warning_seconds
    next_confirmation_warning = confirmation_warning_seconds
    target_asset_raw = asset_id.strip() if isinstance(asset_id, str) and asset_id.strip() else None
    target_asset = target_asset_raw.lower() if target_asset_raw else None
    while True:
        elapsed = int(time.monotonic() - start)
        list_coins_call: collections.abc.Callable[[], list[dict[str, Any]]]
        if include_pending and target_asset_raw is None:

            def list_coins_call() -> list[dict[str, Any]]:
                return wallet.list_coins(include_pending=True)

        elif target_asset_raw is not None:

            def list_coins_call() -> list[dict[str, Any]]:
                if include_pending:
                    return wallet.list_coins(asset_id=target_asset_raw, include_pending=True)
                return wallet.list_coins(asset_id=target_asset_raw)

        else:
            list_coins_call = wallet.list_coins
        coins = call_with_moderate_retry(
            action="wallet_list_coins",
            call=list_coins_call,
            elapsed_seconds=elapsed,
            events=events,
        )
        pending = [
            c
            for c in coins
            if target_asset is None or _coin_asset_id(c).lower() == target_asset
            if str(c.get("id", "")).strip() not in initial_coin_ids
            if str(c.get("state", "")).strip().upper() in {"PENDING", "MEMPOOL"}
        ]
        confirmed = [
            c
            for c in coins
            if target_asset is None or _coin_asset_id(c).lower() == target_asset
            if str(c.get("id", "")).strip() not in initial_coin_ids
            if str(c.get("state", "")).strip().upper() not in {"PENDING", "MEMPOOL"}
        ]
        if pending and not seen_pending:
            seen_pending = True
            sample = str(pending[0].get("name", pending[0].get("id", ""))).strip()
            sample_id = str(pending[0].get("id", "")).strip()
            coinset_url = _coinset_coin_url(coin_name=sample, network=network)
            reconcile = _coinset_reconcile_coin_state(network=network, coin_name=sample)
            events.append(
                {
                    "event": "in_mempool",
                    "coin_id": sample_id,
                    "coin_name": sample,
                    "coinset_url": coinset_url,
                    "elapsed_seconds": str(elapsed),
                    "wait_reason": "waiting_for_mempool_admission",
                    **reconcile,
                }
            )
            if next_heartbeat > 5:
                print("", file=sys.stderr, flush=True)
            print(f"in mempool: {coinset_url}", file=sys.stderr, flush=True)
        if confirmed:
            sample_confirmed = str(confirmed[0].get("name", confirmed[0].get("id", ""))).strip()
            confirmation_reconcile = _coinset_reconcile_coin_state(
                network=network, coin_name=sample_confirmed
            )
            confirmed_height = _safe_int(confirmation_reconcile.get("confirmed_block_index"))
            events.append(
                {
                    "event": "confirmed",
                    "coin_name": sample_confirmed,
                    "coinset_url": _coinset_coin_url(coin_name=sample_confirmed, network=network),
                    "elapsed_seconds": str(elapsed),
                    "wait_reason": "waiting_for_confirmation",
                    **confirmation_reconcile,
                }
            )
            if confirmed_height is not None and confirmed_height >= 0:
                events.extend(
                    _watch_reorg_risk_with_coinset(
                        network=network,
                        confirmed_block_index=confirmed_height,
                        additional_blocks=6,
                        warning_interval_seconds=15 * 60,
                    )
                )
            if next_heartbeat > 5:
                print("", file=sys.stderr, flush=True)
            return events
        if elapsed >= next_heartbeat:
            print(".", end="", file=sys.stderr, flush=True)
            next_heartbeat += 5
        if timeout_seconds is not None and timeout_seconds > 0 and elapsed >= timeout_seconds:
            raise RuntimeError("confirmation_wait_timeout")
        if not seen_pending and elapsed >= next_mempool_warning:
            events.append({"event": "mempool_wait_warning", "elapsed_seconds": str(elapsed)})
            next_mempool_warning += mempool_warning_seconds
        if seen_pending and elapsed >= next_confirmation_warning:
            events.append({"event": "confirmation_wait_warning", "elapsed_seconds": str(elapsed)})
            next_confirmation_warning += confirmation_warning_seconds
        time.sleep(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)


def _is_spendable_coin(coin: dict) -> bool:
    if bool(coin.get("isLocked", False)):
        return False
    coin_state = str(coin.get("state", "")).strip().upper()
    if not coin_state:
        return False
    if coin_state in {
        "PENDING",
        "MEMPOOL",
        "SPENT",
        "SPENDING",
        "LOCKED",
        "RESERVED",
        "UNCONFIRMED",
    }:
        return False
    return coin_state in {"CONFIRMED", "UNSPENT", "SPENDABLE", "AVAILABLE", "SETTLED"}
