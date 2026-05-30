"""Coinset-backed unspent coin listing and confirmation waits (shared runtime)."""

from __future__ import annotations

import time
from typing import Any

from greenfloor.core.engine_bridge import import_engine


def list_unspent_coins_by_receive_address(
    *,
    network: str,
    receive_address: str,
    asset_id: str,
) -> list[dict[str, Any]]:
    engine = import_engine()
    raw_coins = engine.list_wallet_unspent_coins(
        str(network).strip(),
        str(receive_address).strip(),
        str(asset_id).strip(),
    )
    coins: list[dict[str, Any]] = []
    for coin in raw_coins or []:
        if not isinstance(coin, dict):
            continue
        try:
            amount = int(coin.get("amount", 0))
        except (TypeError, ValueError):
            continue
        if amount <= 0:
            continue
        coin_id = str(coin.get("id") or coin.get("name") or "").strip().lower()
        if not coin_id:
            continue
        coins.append(
            {
                "id": coin_id,
                "name": coin_id,
                "amount": amount,
                "state": str(coin.get("state") or "CONFIRMED"),
            }
        )
    return coins


def wait_for_coinset_confirmation(
    *,
    network: str,
    receive_address: str,
    asset_id: str,
    initial_coin_ids: set[str],
    timeout_seconds: int,
) -> list[dict[str, str]]:
    events: list[dict[str, str]] = []
    start = time.monotonic()
    sleep_seconds = 2.0
    while True:
        elapsed = int(time.monotonic() - start)
        if elapsed >= timeout_seconds:
            raise RuntimeError("confirmation_wait_timeout")
        coins = list_unspent_coins_by_receive_address(
            network=network,
            receive_address=receive_address,
            asset_id=asset_id,
        )
        new_confirmed = [
            coin for coin in coins if str(coin.get("id", "")).strip() not in initial_coin_ids
        ]
        if new_confirmed:
            events.append(
                {
                    "event": "confirmed",
                    "coin_name": str(new_confirmed[0].get("name", "")),
                    "elapsed_seconds": str(elapsed),
                }
            )
            return events
        time.sleep(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)
