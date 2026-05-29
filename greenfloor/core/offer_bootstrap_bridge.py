"""Rust-backed bootstrap mixed-output planner (canonical Python bridge)."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from greenfloor.core import kernel_bridge

if TYPE_CHECKING:
    from greenfloor.offer_bootstrap import BootstrapPlan

_KERNEL_REBUILD_HINT = (
    "greenfloor_signer extension is missing plan_bootstrap_mixed_outputs. "
    "Rebuild it (for example: `maturin develop --manifest-path "
    "greenfloor-signer-pyo3/Cargo.toml`)."
)


def _require_bootstrap_planner():
    planner = getattr(kernel_bridge.policy_kernel(), "plan_bootstrap_mixed_outputs", None)
    if planner is None:
        raise RuntimeError(f"{_KERNEL_REBUILD_HINT} Missing symbol: plan_bootstrap_mixed_outputs")
    return planner


def plan_bootstrap_mixed_outputs(
    *,
    sell_ladder: list[Any],
    spendable_coins: list[Any],
) -> BootstrapPlan | None:
    """Build a one-shot mixed-output bootstrap plan from ladder deficits."""
    from greenfloor.offer_bootstrap import BootstrapPlan

    plan = _require_bootstrap_planner()(
        sell_ladder=sell_ladder,
        spendable_coins=spendable_coins,
    )
    if plan is None:
        return None
    if isinstance(plan, BootstrapPlan):
        return plan
    raise TypeError("plan_bootstrap_mixed_outputs returned unexpected result type")
