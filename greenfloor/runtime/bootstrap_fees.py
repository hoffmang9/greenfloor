"""Bootstrap split fee resolution (coinset advice + config fallback)."""

from __future__ import annotations

from greenfloor.runtime.coinset_runtime import _resolve_taker_or_coin_operation_fee


def bootstrap_fee_cost_for_output_count(output_count: int) -> int:
    count = max(1, int(output_count))
    return 1_000_000 + max(0, count - 1) * 250_000


def resolve_bootstrap_split_fee(
    *,
    network: str,
    minimum_fee_mojos: int,
    output_count: int,
) -> tuple[int, str, str | None]:
    fee_cost = bootstrap_fee_cost_for_output_count(output_count)
    spend_count = max(1, int(output_count))
    try:
        fee_mojos, fee_source = _resolve_taker_or_coin_operation_fee(
            network=network,
            minimum_fee_mojos=minimum_fee_mojos,
            fee_cost=fee_cost,
            spend_count=spend_count,
        )
        return int(fee_mojos), fee_source, None
    except Exception as exc:
        fallback_fee = max(0, int(minimum_fee_mojos))
        return fallback_fee, "config_minimum_fee_fallback", str(exc)
