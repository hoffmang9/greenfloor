"""Pure daemon cycle dispatch helpers (Rust-backed)."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters import cycle_kernel


def expand_strategy_actions(strategy_actions: list[Any]) -> list[Any]:
    payload = [
        {
            "size": int(getattr(action, "size", 0)),
            "repeat": int(getattr(action, "repeat", 0)),
        }
        for action in strategy_actions
    ]
    expanded_payload = cycle_kernel.expand_strategy_actions(payload)
    if len(expanded_payload) != sum(max(0, int(getattr(action, "repeat", 0))) for action in strategy_actions):
        raise RuntimeError("expand_strategy_actions_contract_mismatch")
    expanded: list[Any] = []
    for action in strategy_actions:
        repeat = max(0, int(getattr(action, "repeat", 0)))
        expanded.extend(action for _ in range(repeat))
    return expanded


def expiry_seconds_for_action(action: Any) -> int | None:
    unit = str(getattr(action, "expiry_unit", "") or "").strip()
    try:
        value = int(getattr(action, "expiry_value", 0))
    except (TypeError, ValueError):
        return None
    return cycle_kernel.expiry_seconds_for_action(expiry_unit=unit, expiry_value=value)


def reservation_request_for_managed_offer(
    *,
    side: str,
    size_base_units: int,
    base_asset_id: str,
    quote_asset_id: str,
    base_unit_mojo_multiplier: int,
    quote_unit_mojo_multiplier: int,
    quote_price: float,
    fee_asset_id: str,
    fee_amount_mojos: int,
) -> dict[str, int]:
    return cycle_kernel.reservation_request_for_managed_offer(
        {
            "side": side,
            "size_base_units": int(size_base_units),
            "base_asset_id": str(base_asset_id),
            "quote_asset_id": str(quote_asset_id),
            "base_unit_mojo_multiplier": int(base_unit_mojo_multiplier),
            "quote_unit_mojo_multiplier": int(quote_unit_mojo_multiplier),
            "quote_price": float(quote_price),
            "fee_asset_id": str(fee_asset_id),
            "fee_amount_mojos": int(fee_amount_mojos),
        }
    )


def single_input_preferred_skip_reason(
    *,
    requested_amounts: dict[str, int],
    spendable_profiles: dict[str, dict[str, int]],
) -> str | None:
    profiles = {
        asset_id: {
            "total": int(profile.get("total", 0)),
            "max_single": int(profile.get("max_single", 0)),
            "max_single_known": bool(int(profile.get("max_single_known", 0))),
        }
        for asset_id, profile in spendable_profiles.items()
    }
    return cycle_kernel.single_input_preferred_skip_reason(
        requested_amounts=requested_amounts,
        spendable_profiles=profiles,
    )
