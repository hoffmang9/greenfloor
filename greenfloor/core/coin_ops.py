"""Coin-operation planning kernel (Rust-backed)."""

from __future__ import annotations

import importlib
from dataclasses import dataclass

_INSTALL_HINT = (
    "Install the greenfloor_signer extension (for example: "
    "`maturin develop -m greenfloor-signer-pyo3` from the repo root)."
)

__all__ = ["BucketSpec", "CoinOpPlan", "plan_coin_ops"]


def _import_signer():
    try:
        return importlib.import_module("greenfloor_signer")
    except ImportError as exc:
        raise ImportError(
            f"greenfloor_signer is not available. {_INSTALL_HINT} Original error: {exc}"
        ) from exc


@dataclass(frozen=True, slots=True)
class BucketSpec:
    size_base_units: int
    target_count: int
    split_buffer_count: int
    combine_when_excess_factor: float
    current_count: int


@dataclass(frozen=True, slots=True)
class CoinOpPlan:
    op_type: str
    size_base_units: int
    op_count: int
    reason: str


def _require_coin_op_plans(value: object) -> list[CoinOpPlan]:
    if not isinstance(value, list):
        raise TypeError("signer returned non-list result")
    plans: list[CoinOpPlan] = []
    for item in value:
        if not isinstance(item, CoinOpPlan):
            raise TypeError("signer returned non-CoinOpPlan result")
        plans.append(item)
    return plans


def plan_coin_ops(
    *,
    buckets: list[BucketSpec],
    max_operations_per_run: int,
    max_fee_budget_mojos: int,
    split_fee_mojos: int,
    combine_fee_mojos: int,
) -> list[CoinOpPlan]:
    return _require_coin_op_plans(
        _import_signer().plan_coin_ops(
            buckets,
            int(max_operations_per_run),
            int(max_fee_budget_mojos),
            int(split_fee_mojos),
            int(combine_fee_mojos),
        )
    )
