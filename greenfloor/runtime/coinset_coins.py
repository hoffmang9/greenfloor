"""Coinset-backed unspent coin listing and confirmation waits (shared runtime)."""

from __future__ import annotations

import time
from typing import Any

from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.hex_utils import canonical_is_xch


def list_unspent_coins_by_receive_address(
    *,
    network: str,
    receive_address: str,
    asset_id: str,
) -> list[dict[str, Any]]:
    try:
        import chia_wallet_sdk as sdk  # type: ignore[import-not-found]
    except Exception as exc:
        raise RuntimeError(f"wallet_sdk_import_error:{exc}") from exc
    coinset = CoinsetAdapter(None, network=network)
    address = sdk.Address.decode(receive_address)
    inner_puzzle_hash = address.puzzle_hash
    asset_raw = str(asset_id).strip().lower()
    if canonical_is_xch(asset_raw):
        puzzle_hash = inner_puzzle_hash
    else:
        asset_id_bytes = bytes.fromhex(asset_raw.removeprefix("0x"))
        puzzle_hash = sdk.cat_puzzle_hash(asset_id_bytes, inner_puzzle_hash)
    puzzle_hash_hex = sdk.to_hex(puzzle_hash)
    records = coinset.get_coin_records_by_puzzle_hash(
        puzzle_hash_hex=puzzle_hash_hex,
        include_spent_coins=False,
    )
    coins: list[dict[str, Any]] = []
    for record in records or []:
        if not isinstance(record, dict):
            continue
        coin_payload = record.get("coin")
        if not isinstance(coin_payload, dict):
            continue
        amount_raw = coin_payload.get("amount")
        if amount_raw is None:
            continue
        try:
            amount = int(amount_raw)
        except (TypeError, ValueError):
            continue
        if amount <= 0:
            continue
        coin_name = str(record.get("coin_id") or record.get("coin_name") or "").strip().lower()
        if not coin_name:
            coin_name = str(coin_payload.get("parent_coin_info", "")).strip().lower()
        if not coin_name:
            continue
        coins.append(
            {
                "id": coin_name,
                "name": coin_name,
                "amount": amount,
                "state": "CONFIRMED",
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
