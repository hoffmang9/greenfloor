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
class BootstrapLadderEntry:
    size_base_units: int
    target_count: int
    split_buffer_count: int


@dataclass(frozen=True, slots=True)
class BootstrapPlan:
    source_coin_id: str
    source_amount: int
    output_amounts_base_units: list[int]
    total_output_amount: int
    change_amount: int
    deficits: list[LadderDeficit]


_KERNEL_REBUILD_HINT = (
    "greenfloor_signer extension is missing plan_bootstrap_mixed_outputs. "
    "Rebuild it (for example: `maturin develop --manifest-path "
    "greenfloor-signer-pyo3/Cargo.toml`)."
)


def _require_kernel_planner():
    from greenfloor.core.kernel_bridge import import_kernel

    kernel = import_kernel()
    planner = getattr(kernel, "plan_bootstrap_mixed_outputs", None)
    if planner is None:
        raise RuntimeError(_KERNEL_REBUILD_HINT)
    return planner


def plan_bootstrap_mixed_outputs(
    *,
    sell_ladder: list[Any],
    spendable_coins: list[Any],
) -> BootstrapPlan | None:
    """Build a one-shot mixed-output bootstrap plan from ladder deficits.

    `spendable_coins` may be dict-like wallet payloads or lightweight objects
    exposing `id` and `amount` attributes.
    """
    planner = _require_kernel_planner()
    plan = planner(sell_ladder=sell_ladder, spendable_coins=spendable_coins)
    if plan is None:
        return None
    if isinstance(plan, BootstrapPlan):
        return plan
    raise TypeError("plan_bootstrap_mixed_outputs returned unexpected result type")
