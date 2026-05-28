"""Rust-backed coin-op iteration gate and stop policy."""

from __future__ import annotations

from typing import Any

from greenfloor.core.kernel_bridge import import_kernel


def evaluate_coin_split_gate(
    *,
    asset_scoped_coins: list[dict[str, Any]],
    resolved_asset_id: str,
    size_base_units: int,
    required_count: int,
) -> dict[str, int | bool | str]:
    gate = import_kernel().evaluate_coin_split_gate(
        asset_scoped_coins,
        str(resolved_asset_id),
        int(size_base_units),
        int(required_count),
    )
    if not isinstance(gate, dict):
        raise TypeError("evaluate_coin_split_gate returned non-dict result")
    return {
        "asset_id": str(gate["asset_id"]),
        "size_base_units": int(gate["size_base_units"]),
        "required_min_count": int(gate["required_min_count"]),
        "current_count": int(gate["current_count"]),
        "larger_reserve_coin_count": int(gate["larger_reserve_coin_count"]),
        "extra_denom_coin_count": int(gate["extra_denom_coin_count"]),
        "reserve_ready": bool(gate["reserve_ready"]),
        "ready": bool(gate["ready"]),
    }


def coin_op_should_stop(
    *,
    until_ready: bool,
    final_readiness: dict[str, int | bool | str] | None,
    coin_ids: list[str],
    iteration: int,
    max_iterations: int,
) -> tuple[bool, str]:
    ready = None if final_readiness is None else bool(final_readiness.get("ready", False))
    stop, reason = import_kernel().coin_op_should_stop(
        bool(until_ready),
        ready,
        bool(coin_ids),
        int(iteration),
        int(max_iterations),
    )
    return bool(stop), str(reason)
