"""Coinset inventory scans for the daemon testing harness."""

from __future__ import annotations

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.daemon.market_logging import _daemon_logger
from greenfloor.hex_utils import is_hex_id
from greenfloor.runtime.coin_ops.coins import is_spendable_coin
from greenfloor.runtime.coinset_coins import list_unspent_coins_by_receive_address


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
        coins = list_unspent_coins_by_receive_address(
            network=str(network),
            receive_address=str(receive_address),
            asset_id=asset_hex,
        )
    except Exception:
        return []
    multiplier = max(1, int(base_unit_mojo_multiplier))
    amounts: list[int] = []
    for coin in coins:
        if not isinstance(coin, dict):
            continue
        try:
            amount_mojos = int(coin.get("amount", 0))
        except (TypeError, ValueError):
            continue
        if amount_mojos <= 0:
            continue
        amount_base_units = amount_mojos // multiplier
        if amount_base_units > 0:
            amounts.append(amount_base_units)
    return amounts
