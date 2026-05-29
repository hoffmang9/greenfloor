"""Stable runtime import path for bootstrap planner, phase policy, and DTOs.

Engine-backed symbols live here. Call via this module (not ``engine_bridge.bootstrap_engine()``).
Coinset coin dicts are coerced to ``BootstrapCoin`` at the planner boundary; PyO3 requires
``BootstrapCoin`` instances.

**Policy ownership:** deterministic planner + early/executed phase mapping are Rust
(``greenfloor-engine/src/offer/bootstrap/``). **Fee eligibility** and mixed-split I/O are
Python-only (``greenfloor/runtime/offer_bootstrap.py``).
"""

from __future__ import annotations

from typing import Any

from greenfloor.core import engine_bridge
from greenfloor.offer_bootstrap import (
    BootstrapCoin,
    BootstrapPhaseResult,
    BootstrapPlan,
    BootstrapPlanOutcome,
    LadderDeficit,
    PlannerLadderRow,
)

__all__ = [
    "BootstrapCoin",
    "BootstrapPhaseResult",
    "BootstrapPlan",
    "BootstrapPlanOutcome",
    "LadderDeficit",
    "PlannerLadderRow",
    "bootstrap_early_phase",
    "bootstrap_executed_phase",
    "plan_bootstrap_mixed_outputs",
]


_require_bootstrap_method = engine_bridge.engine_method_getter(
    lambda: engine_bridge.bootstrap_engine(),
    missing="bootstrap planner",
)


def _coerce_spendable_coins(spendable_coins: list[Any]) -> list[BootstrapCoin]:
    coerced: list[BootstrapCoin] = []
    for index, coin in enumerate(spendable_coins):
        if isinstance(coin, BootstrapCoin):
            coerced.append(coin)
            continue
        if isinstance(coin, dict):
            if "amount" not in coin:
                raise ValueError(f"spendable_coins[{index}] missing required field: amount")
            coin_id = coin.get("id", "")
            if not isinstance(coin_id, str):
                raise ValueError(f"spendable_coins[{index}].id must be a string")
            coerced.append(BootstrapCoin(id=coin_id.strip(), amount=int(coin["amount"])))
            continue
        amount = getattr(coin, "amount", None)
        if amount is None:
            raise ValueError(f"spendable_coins[{index}] missing required attribute: amount")
        coin_id = getattr(coin, "id", "")
        if not isinstance(coin_id, str):
            raise ValueError(f"spendable_coins[{index}].id must be a string")
        coerced.append(BootstrapCoin(id=coin_id.strip(), amount=int(amount)))
    return coerced


def _coerce_planner_outcome(payload: object) -> BootstrapPlanOutcome:
    if isinstance(payload, BootstrapPlanOutcome):
        return payload
    raise TypeError("plan_bootstrap_mixed_outputs returned unexpected result type")


def _coerce_phase_result(payload: object) -> BootstrapPhaseResult:
    if isinstance(payload, BootstrapPhaseResult):
        return payload
    raise TypeError("bootstrap phase engine call returned unexpected result type")


def plan_bootstrap_mixed_outputs(
    *,
    ladder_entries: list[PlannerLadderRow],
    spendable_coins: list[Any],
) -> BootstrapPlanOutcome:
    """Evaluate bootstrap inventory against denomination ladder rows."""
    if not all(isinstance(row, PlannerLadderRow) for row in ladder_entries):
        raise TypeError("ladder_entries must contain PlannerLadderRow instances")

    outcome = _require_bootstrap_method("plan_bootstrap_mixed_outputs")(
        ladder_entries=ladder_entries,
        spendable_coins=_coerce_spendable_coins(spendable_coins),
    )
    return _coerce_planner_outcome(outcome)


def bootstrap_early_phase(
    *,
    outcome: BootstrapPlanOutcome,
) -> BootstrapPhaseResult | None:
    """Map a planner outcome to an early phase result, if mixed-split should not run."""
    phase = _require_bootstrap_method("bootstrap_early_phase")(outcome=outcome)
    if phase is None:
        return None
    return _coerce_phase_result(phase)


def bootstrap_executed_phase(
    *,
    remaining: BootstrapPlanOutcome,
) -> BootstrapPhaseResult:
    """Map a post-split replan outcome to executed-phase status/reason/ready."""
    return _coerce_phase_result(
        _require_bootstrap_method("bootstrap_executed_phase")(remaining=remaining)
    )
