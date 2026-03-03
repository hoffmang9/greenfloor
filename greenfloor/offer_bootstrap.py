from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True, slots=True)
class LadderDeficit:
    size_base_units: int
    required_count: int
    current_count: int
    deficit_count: int


@dataclass(frozen=True, slots=True)
class BootstrapPlan:
    source_coin_id: str
    source_amount: int
    output_amounts_base_units: list[int]
    total_output_amount: int
    change_amount: int
    deficits: list[LadderDeficit]


def _sorted_ladder_entries(sell_ladder: list[Any]) -> list[Any]:
    return sorted(sell_ladder, key=lambda row: int(row.size_base_units))


def _count_exact_amount_coins(
    *, spendable_coin_amounts: list[int], ladder_sizes: list[int]
) -> dict[int, int]:
    ladder = set(ladder_sizes)
    counts = {size: 0 for size in ladder_sizes}
    for amount in spendable_coin_amounts:
        if amount in ladder:
            counts[amount] += 1
    return counts


def _coin_value(coin: Any, field: str, default: Any) -> Any:
    if isinstance(coin, dict):
        return coin.get(field, default)
    return getattr(coin, field, default)


def plan_bootstrap_mixed_outputs(
    *,
    sell_ladder: list[Any],
    spendable_coins: list[Any],
) -> BootstrapPlan | None:
    """Build a one-shot mixed-output bootstrap plan from ladder deficits.

    `spendable_coins` may be dict-like wallet payloads or lightweight objects
    exposing `id` and `amount` attributes.
    """
    sorted_ladder = _sorted_ladder_entries(sell_ladder)
    if not sorted_ladder:
        return None

    ladder_sizes = [int(row.size_base_units) for row in sorted_ladder]
    spendable_amounts = [int(_coin_value(coin, "amount", 0)) for coin in spendable_coins]
    counts = _count_exact_amount_coins(
        spendable_coin_amounts=spendable_amounts,
        ladder_sizes=ladder_sizes,
    )

    deficits: list[LadderDeficit] = []
    output_amounts: list[int] = []
    for row in sorted_ladder:
        size = int(row.size_base_units)
        required = int(row.target_count) + int(row.split_buffer_count)
        current = int(counts.get(size, 0))
        deficit = required - current
        if deficit <= 0:
            continue
        deficits.append(
            LadderDeficit(
                size_base_units=size,
                required_count=required,
                current_count=current,
                deficit_count=deficit,
            )
        )
        output_amounts.extend([size] * deficit)

    if not deficits:
        return None

    total_output_amount = sum(output_amounts)
    if total_output_amount <= 0:
        return None

    candidate = None
    for coin in sorted(
        spendable_coins, key=lambda c: int(_coin_value(c, "amount", 0)), reverse=True
    ):
        amount = int(_coin_value(coin, "amount", 0))
        coin_id = str(_coin_value(coin, "id", "")).strip()
        if not coin_id:
            continue
        if amount >= total_output_amount:
            candidate = (coin_id, amount)
            break
    if candidate is None:
        return None

    source_coin_id, source_amount = candidate
    return BootstrapPlan(
        source_coin_id=source_coin_id,
        source_amount=source_amount,
        output_amounts_base_units=output_amounts,
        total_output_amount=total_output_amount,
        change_amount=source_amount - total_output_amount,
        deficits=deficits,
    )
