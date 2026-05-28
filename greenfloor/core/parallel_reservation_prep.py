"""Parallel managed-offer reservation prep types (Rust FFI + dispatch planning)."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class ParallelActionReservationInput:
    submit_index: int
    side: str
    size_base_units: int


@dataclass(frozen=True, slots=True)
class ParallelReservationContext:
    base_asset_id: str
    quote_asset_id: str
    fee_asset_id: str
    fee_amount_mojos: int
    base_unit_mojo_multiplier: int
    quote_unit_mojo_multiplier: int
    quote_price: float


@dataclass(frozen=True, slots=True)
class ParallelReservationEntry:
    submit_index: int
    requested_amounts: dict[str, int]


@dataclass(frozen=True, slots=True)
class ParallelReservationPrep:
    entries: list[ParallelReservationEntry]
    asset_ids: list[str]
