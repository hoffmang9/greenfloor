"""Typed models and shared coin-selection helpers for coin operations."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Any

from greenfloor.config.models import MarketLadderEntry


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
    """Filter asset-scoped coins for split/combine selection."""
    _ = verify_direct_spendable_lookup
    from greenfloor.runtime.coin_ops.coins import filter_spendable_scoped_coins

    return filter_spendable_scoped_coins(
        coins=coins,
        wallet=wallet,
        resolved_asset_id=resolved_asset_id,
        canonical_asset_id=canonical_asset_id,
        refresh_rows=(mode == CoinOpSelectionMode.DAEMON),
    )


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
    def from_ladder_entry(
        cls, entry: MarketLadderEntry, *, threshold: int
    ) -> CombineDenominationTarget:
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
