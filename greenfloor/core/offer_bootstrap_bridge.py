"""Rust-backed bootstrap mixed-output planner (canonical Python bridge).

Call symbols here (or via ``offer_bootstrap_policy``), not ``kernel_bridge.bootstrap_kernel()``
directly. Coinset coin dicts are coerced to ``BootstrapCoin`` in this module; PyO3 accepts
``BootstrapCoin`` instances only.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from greenfloor.core import kernel_bridge

if TYPE_CHECKING:
    from greenfloor.offer_bootstrap import (
        BootstrapCoin,
        BootstrapPhaseResult,
        BootstrapPlanOutcome,
        PlannerLadderRow,
    )

_KERNEL_REBUILD_HINT = (
    "greenfloor_signer extension is missing bootstrap planner symbols. "
    "Rebuild it (for example: `maturin develop --manifest-path "
    "greenfloor-signer-pyo3/Cargo.toml`)."
)


def _require_bootstrap_kernel():
    return kernel_bridge.bootstrap_kernel()


def _require_bootstrap_method(method_name: str):
    method = getattr(_require_bootstrap_kernel(), method_name, None)
    if method is None:
        raise RuntimeError(f"{_KERNEL_REBUILD_HINT} Missing symbol: {method_name}")
    return method


def _coerce_spendable_coins(spendable_coins: list[Any]) -> list[BootstrapCoin]:
    from greenfloor.offer_bootstrap import BootstrapCoin

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
    from greenfloor.offer_bootstrap import BootstrapPlanOutcome

    if isinstance(payload, BootstrapPlanOutcome):
        return payload
    raise TypeError("plan_bootstrap_mixed_outputs returned unexpected result type")


def _coerce_phase_result(payload: object) -> BootstrapPhaseResult:
    from greenfloor.offer_bootstrap import BootstrapPhaseResult

    if isinstance(payload, BootstrapPhaseResult):
        return payload
    raise TypeError("bootstrap phase kernel call returned unexpected result type")


def plan_bootstrap_mixed_outputs(
    *,
    ladder_entries: list[PlannerLadderRow],
    spendable_coins: list[Any],
) -> BootstrapPlanOutcome:
    """Evaluate bootstrap inventory against denomination ladder rows."""
    from greenfloor.offer_bootstrap import PlannerLadderRow

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
