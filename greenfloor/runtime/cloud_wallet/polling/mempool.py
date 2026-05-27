from __future__ import annotations

import collections.abc
import sys
import time
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.config.io import is_testnet
from greenfloor.moderate_retry import call_with_moderate_retry
from greenfloor.runtime.cloud_wallet.coins import coin_asset_id, safe_int


def coinset_coin_url(*, coin_name: str, network: str = "mainnet") -> str:
    base = "https://testnet11.coinset.org" if is_testnet(network) else "https://coinset.org"
    return f"{base}/coin/{coin_name.strip()}"


def coinset_reconcile_coin_state(*, network: str, coin_name: str) -> dict[str, str]:
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
    confirmed_height = safe_int(record.get("confirmed_block_index"))
    spent_height = safe_int(record.get("spent_block_index"))
    return {
        "reconcile": "ok",
        "confirmed_block_index": str(confirmed_height if confirmed_height is not None else -1),
        "spent_block_index": str(spent_height if spent_height is not None else -1),
        "coinbase": str(bool(record.get("coinbase", False))).lower(),
    }


def coinset_peak_height(*, network: str) -> int | None:
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
        parsed = safe_int(candidate)
        if parsed is not None and parsed >= 0:
            return parsed
    return None


def watch_reorg_risk_with_coinset(
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
        peak_height = coinset_peak_height(network=network)
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
    retry_fn: collections.abc.Callable[..., Any] | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
    monotonic_fn: collections.abc.Callable[[], float] | None = None,
    coinset_reconcile_fn: collections.abc.Callable[..., dict[str, str]] | None = None,
    reorg_watch_fn: collections.abc.Callable[..., list[dict[str, str]]] | None = None,
) -> list[dict[str, str]]:
    if retry_fn is None:
        retry_fn = call_with_moderate_retry
    if sleep_fn is None:
        sleep_fn = time.sleep
    if monotonic_fn is None:
        monotonic_fn = time.monotonic
    if coinset_reconcile_fn is None:
        coinset_reconcile_fn = coinset_reconcile_coin_state
    if reorg_watch_fn is None:
        reorg_watch_fn = watch_reorg_risk_with_coinset

    events: list[dict[str, str]] = []
    start = monotonic_fn()
    seen_pending = False
    next_heartbeat = 5
    sleep_seconds = 2.0
    next_mempool_warning = mempool_warning_seconds
    next_confirmation_warning = confirmation_warning_seconds
    target_asset_raw = asset_id.strip() if isinstance(asset_id, str) and asset_id.strip() else None
    target_asset = target_asset_raw.lower() if target_asset_raw else None
    while True:
        elapsed = int(monotonic_fn() - start)
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
        coins = retry_fn(
            action="wallet_list_coins",
            call=list_coins_call,
            elapsed_seconds=elapsed,
            events=events,
            sleep_fn=sleep_fn,
        )
        pending = [
            c
            for c in coins
            if target_asset is None or coin_asset_id(c).lower() == target_asset
            if str(c.get("id", "")).strip() not in initial_coin_ids
            if str(c.get("state", "")).strip().upper() in {"PENDING", "MEMPOOL"}
        ]
        confirmed = [
            c
            for c in coins
            if target_asset is None or coin_asset_id(c).lower() == target_asset
            if str(c.get("id", "")).strip() not in initial_coin_ids
            if str(c.get("state", "")).strip().upper() not in {"PENDING", "MEMPOOL"}
        ]
        if pending and not seen_pending:
            seen_pending = True
            sample = str(pending[0].get("name", pending[0].get("id", ""))).strip()
            sample_id = str(pending[0].get("id", "")).strip()
            coinset_url = coinset_coin_url(coin_name=sample, network=network)
            reconcile = coinset_reconcile_fn(network=network, coin_name=sample)
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
            confirmation_reconcile = coinset_reconcile_fn(
                network=network, coin_name=sample_confirmed
            )
            confirmed_height = safe_int(confirmation_reconcile.get("confirmed_block_index"))
            events.append(
                {
                    "event": "confirmed",
                    "coin_name": sample_confirmed,
                    "coinset_url": coinset_coin_url(coin_name=sample_confirmed, network=network),
                    "elapsed_seconds": str(elapsed),
                    "wait_reason": "waiting_for_confirmation",
                    **confirmation_reconcile,
                }
            )
            if confirmed_height is not None and confirmed_height >= 0:
                events.extend(
                    reorg_watch_fn(
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
        sleep_fn(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)
