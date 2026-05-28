"""Parallel managed-offer reservation context for Coinset profile fetch."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class ParallelReservationContext:
    base_asset_id: str
    quote_asset_id: str
    fee_asset_id: str
    fee_amount_mojos: int
    base_unit_mojo_multiplier: int
    quote_unit_mojo_multiplier: int
    quote_price: float


def parallel_reservation_asset_ids(ctx: ParallelReservationContext) -> set[str]:
    return {
        asset_id
        for asset_id in (
            str(ctx.base_asset_id).strip(),
            str(ctx.quote_asset_id).strip(),
            str(ctx.fee_asset_id).strip(),
        )
        if asset_id
    }
