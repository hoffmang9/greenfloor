"""Typed models and shared coin-selection helpers for coin operations."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Any

from greenfloor.config.models import MarketLadderEntry
from greenfloor.core.coin_ops_policy import coin_op_min_amount_mojos


class CoinOpSelectionMode(StrEnum):
    """How spendable coin rows are refreshed before selection."""

    CLI = "cli"
    DAEMON = "daemon"


def filter_spendable_for_coin_ops(
    *,
    coins: list[dict],
    wallet: Any,
    resolved_asset_id: str,
    canonical_asset_id: str,
    mode: CoinOpSelectionMode,
    verify_direct_spendable_lookup: bool = False,
) -> list[dict]:
    """Filter asset-scoped coins for split/combine selection.

    CLI uses the scoped list as-is (``include_pending=True`` on list_coins).
    Daemon re-fetches each row before selection to avoid stale locked coins.
    When ``verify_direct_spendable_lookup`` is set, rows are re-checked via
    ``get_coin_record`` when the wallet adapter supports it.
    """
    from greenfloor.runtime.cloud_wallet.coins import (
        coin_matches_direct_spendable_lookup,
        filter_spendable_scoped_coins,
    )

    scoped = filter_spendable_scoped_coins(
        coins=coins,
        wallet=wallet,
        resolved_asset_id=resolved_asset_id,
        canonical_asset_id=canonical_asset_id,
        refresh_rows=(mode == CoinOpSelectionMode.DAEMON),
    )
    if not verify_direct_spendable_lookup:
        return scoped
    lookup_cache: dict[str, bool] = {}
    return [
        coin
        for coin in scoped
        if coin_matches_direct_spendable_lookup(
            wallet=wallet,
            coin=coin,
            scoped_asset_id=resolved_asset_id,
            cache=lookup_cache,
        )
    ]


@dataclass(slots=True)
class SplitDenominationTarget:
    size_base_units: int
    target_count: int
    split_buffer_count: int
    required_count: int

    @classmethod
    def from_ladder_entry(cls, entry: MarketLadderEntry) -> SplitDenominationTarget:
        required_count = int(entry.target_count) + int(entry.split_buffer_count)
        return cls(
            size_base_units=int(entry.size_base_units),
            target_count=int(entry.target_count),
            split_buffer_count=int(entry.split_buffer_count),
            required_count=required_count,
        )

    def to_payload(self) -> dict[str, int | float]:
        return {
            "size_base_units": self.size_base_units,
            "target_count": self.target_count,
            "split_buffer_count": self.split_buffer_count,
            "required_count": self.required_count,
        }

    def split_readiness_kwargs(self) -> dict[str, int]:
        return {"required_min_count": self.required_count}


@dataclass(slots=True)
class CombineDenominationTarget:
    size_base_units: int
    target_count: int
    combine_when_excess_factor: float
    combine_threshold_count: int

    @classmethod
    def from_ladder_entry(cls, entry: MarketLadderEntry, *, threshold: int) -> CombineDenominationTarget:
        return cls(
            size_base_units=int(entry.size_base_units),
            target_count=int(entry.target_count),
            combine_when_excess_factor=float(entry.combine_when_excess_factor),
            combine_threshold_count=int(threshold),
        )

    def to_payload(self) -> dict[str, int | float]:
        return {
            "size_base_units": self.size_base_units,
            "target_count": self.target_count,
            "combine_when_excess_factor": self.combine_when_excess_factor,
            "combine_threshold_count": self.combine_threshold_count,
        }

    def combine_readiness_kwargs(self) -> dict[str, int]:
        return {"max_allowed_count": self.combine_threshold_count}


DenominationTarget = SplitDenominationTarget | CombineDenominationTarget | None


def denomination_target_payload(target: DenominationTarget) -> dict[str, int | float] | None:
    if target is None:
        return None
    return target.to_payload()


def select_largest_spendable_coin(
    coins: list[dict],
    *,
    min_amount_mojos: int = 0,
    exclude_coin_ids: set[str] | None = None,
) -> dict | None:
    excluded = exclude_coin_ids or set()
    eligible = [
        coin
        for coin in coins
        if isinstance(coin, dict)
        and str(coin.get("id", "")).strip()
        and str(coin.get("id", "")).strip() not in excluded
        and int(coin.get("amount", 0)) >= int(min_amount_mojos)
    ]
    if not eligible:
        return None
    return max(eligible, key=lambda coin: int(coin.get("amount", 0)))


def select_exact_amount_coin_ids(
    coins: list[dict],
    *,
    amount_mojos: int,
    exclude_coin_ids: set[str] | None = None,
    max_count: int | None = None,
) -> list[str]:
    excluded = {value.lower() for value in (exclude_coin_ids or set())}
    selected: list[str] = []
    for coin in coins:
        if not isinstance(coin, dict):
            continue
        coin_id = str(coin.get("id", "")).strip()
        if not coin_id or coin_id.lower() in excluded:
            continue
        try:
            amount = int(coin.get("amount", 0))
        except (TypeError, ValueError):
            continue
        if amount != int(amount_mojos):
            continue
        selected.append(coin_id)
        if max_count is not None and len(selected) >= int(max_count):
            break
    return selected


def split_would_create_sub_cat_change(
    *,
    selected_amount_mojos: int,
    required_amount_mojos: int,
    canonical_asset_id: str,
) -> tuple[bool, int]:
    remainder = int(selected_amount_mojos) - int(required_amount_mojos)
    min_cat_mojos = coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)
    if min_cat_mojos > 0 and remainder > 0 and remainder < int(min_cat_mojos):
        return True, remainder
    return False, remainder
