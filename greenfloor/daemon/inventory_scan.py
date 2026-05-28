"""Coinset inventory scans and websocket signal capture for the daemon."""

from __future__ import annotations

import urllib.parse
from typing import Any

from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.daemon.coinset_ws import capture_coinset_websocket_once
from greenfloor.daemon.market_logging import _daemon_logger
from greenfloor.daemon.watchlist import _match_watched_coin_ids
from greenfloor.hex_utils import is_hex_id
from greenfloor.runtime.coin_ops.coins import is_spendable_coin
from greenfloor.runtime.coinset_coins import list_unspent_coins_by_receive_address
from greenfloor.storage.sqlite import SqliteStore


def coinset_spendable_profiles_by_asset(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    asset_ids: set[str],
) -> dict[str, dict[str, int | bool]]:
    receive_address = str(market.receive_address).strip()
    network = str(program.app_network).strip()
    requested_asset_ids = {str(asset_id).strip() for asset_id in asset_ids if str(asset_id).strip()}
    profiles: dict[str, dict[str, int | bool]] = {
        asset_id: {
            "total": 0,
            "max_single": 0,
            "coin_count": 0,
            "max_single_known": True,
        }
        for asset_id in requested_asset_ids
    }
    if not requested_asset_ids or not receive_address:
        return profiles
    for requested_asset_id in requested_asset_ids:
        profile = profiles[requested_asset_id]
        try:
            coins = list_unspent_coins_by_receive_address(
                network=network,
                receive_address=receive_address,
                asset_id=requested_asset_id,
            )
        except Exception as exc:
            _daemon_logger.warning(
                "coinset_inventory_lookup_failed asset_id=%s error=%s",
                requested_asset_id,
                exc,
            )
            continue
        for coin in coins:
            if not isinstance(coin, dict):
                continue
            if not is_spendable_coin(coin):
                continue
            try:
                amount = int(coin.get("amount", 0))
            except (TypeError, ValueError):
                amount = 0
            if amount <= 0:
                continue
            profile["total"] += amount
            profile["coin_count"] += 1
            if amount > int(profile.get("max_single", 0)):
                profile["max_single"] = amount
    return profiles


_coinset_spendable_profiles_by_asset = coinset_spendable_profiles_by_asset


def _coinset_spendable_base_unit_coin_amounts(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    resolved_asset_id: str,
    base_unit_mojo_multiplier: int,
) -> list[int]:
    receive_address = str(market.receive_address).strip()
    if not receive_address or not str(resolved_asset_id).strip():
        return []
    multiplier = max(1, int(base_unit_mojo_multiplier))
    try:
        coins = list_unspent_coins_by_receive_address(
            network=str(program.app_network).strip(),
            receive_address=receive_address,
            asset_id=str(resolved_asset_id).strip(),
        )
    except Exception:
        return []
    amounts_base_units: list[int] = []
    for coin in coins:
        if not isinstance(coin, dict) or not is_spendable_coin(coin):
            continue
        try:
            amount_mojos = int(coin.get("amount", 0))
        except (TypeError, ValueError):
            continue
        if amount_mojos <= 0:
            continue
        amount_base_units = amount_mojos // multiplier
        if amount_base_units > 0:
            amounts_base_units.append(amount_base_units)
    return amounts_base_units


def _coinset_cat_spendable_base_unit_coin_amounts(
    *,
    canonical_asset_id: str,
    receive_address: str,
    network: str,
    base_unit_mojo_multiplier: int,
) -> list[int]:
    asset_hex = str(canonical_asset_id).strip().lower()
    if not asset_hex or not is_hex_id(asset_hex):
        return []
    try:
        import chia_wallet_sdk as sdk  # type: ignore[import-untyped]

        address = sdk.Address.decode(str(receive_address))
        inner_puzzle_hash = bytes(address.puzzle_hash)
        asset_id_bytes = bytes.fromhex(asset_hex)
        cat_puzzle_hash = sdk.cat_puzzle_hash(asset_id_bytes, inner_puzzle_hash)
        coinset = CoinsetAdapter(network=str(network))
        records = coinset.get_coin_records_by_puzzle_hash(
            puzzle_hash_hex=f"0x{bytes(cat_puzzle_hash).hex()}",
            include_spent_coins=False,
        )
    except Exception:
        return []
    multiplier = max(1, int(base_unit_mojo_multiplier))
    amounts: list[int] = []
    for record in records:
        if not isinstance(record, dict):
            continue
        coin_payload = record.get("coin")
        if not isinstance(coin_payload, dict):
            continue
        try:
            amount_mojos = int(coin_payload.get("amount", 0))
        except (TypeError, ValueError):
            continue
        if amount_mojos <= 0:
            continue
        amount_base_units = amount_mojos // multiplier
        if amount_base_units > 0:
            amounts.append(amount_base_units)
    return amounts


def _resolve_coinset_ws_url(*, program, coinset_base_url: str) -> str:
    configured = str(getattr(program, "tx_block_websocket_url", "")).strip()
    if configured:
        return configured
    base_url = coinset_base_url.strip()
    if not base_url:
        if program.app_network.strip().lower() in {"testnet", "testnet11"}:
            return "wss://testnet11.api.coinset.org/ws"
        return "wss://api.coinset.org/ws"
    parsed = urllib.parse.urlparse(base_url)
    scheme = "wss" if parsed.scheme == "https" else "ws"
    host = parsed.netloc or parsed.path
    if not host:
        return "wss://api.coinset.org/ws"
    return f"{scheme}://{host}/ws"


def _build_coinset_adapter(*, program, coinset_base_url: str) -> CoinsetAdapter:
    base_url = coinset_base_url.strip() or None
    try:
        return CoinsetAdapter(base_url, network=program.app_network)
    except TypeError as exc:
        if "network" not in str(exc):
            raise
        return CoinsetAdapter(base_url)


def _run_coinset_signal_capture_once(
    *,
    program,
    coinset_base_url: str,
    store: SqliteStore,
) -> None:
    coinset = _build_coinset_adapter(program=program, coinset_base_url=coinset_base_url)
    ws_url = _resolve_coinset_ws_url(program=program, coinset_base_url=coinset_base_url)

    def _on_mempool_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return
        new_count = store.observe_mempool_tx_ids(tx_ids)
        if new_count:
            store.add_audit_event(
                "mempool_observed",
                {"new_tx_ids": new_count, "source": "coinset_websocket"},
            )

    def _on_confirmed_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return
        confirmed = store.confirm_tx_ids(tx_ids)
        store.add_audit_event(
            "tx_block_confirmed",
            {
                "tx_ids": tx_ids,
                "confirmed_count": confirmed,
                "source": "coinset_websocket",
            },
        )

    def _on_audit_event(event_type: str, payload: dict[str, Any]) -> None:
        store.add_audit_event(event_type, payload)

    def _on_observed_coin_ids(coin_ids: list[str]) -> None:
        if not coin_ids:
            return
        hits = _match_watched_coin_ids(observed_coin_ids=coin_ids)
        if not hits:
            return
        store.add_audit_event(
            "coin_watch_hit",
            {
                "coin_id_count": len(coin_ids),
                "coin_ids_sample": sorted({str(c).strip().lower() for c in coin_ids})[:10],
                "market_hits": {market_id: ids[:10] for market_id, ids in hits.items()},
                "source": "coinset_websocket",
            },
        )

    capture_coinset_websocket_once(
        ws_url=ws_url,
        reconnect_interval_seconds=program.tx_block_websocket_reconnect_interval_seconds,
        capture_window_seconds=max(1, program.tx_block_fallback_poll_interval_seconds),
        on_mempool_tx_ids=_on_mempool_tx_ids,
        on_confirmed_tx_ids=_on_confirmed_tx_ids,
        on_audit_event=_on_audit_event,
        on_observed_coin_ids=_on_observed_coin_ids,
        recovery_poll=coinset.get_all_mempool_tx_ids,
    )
