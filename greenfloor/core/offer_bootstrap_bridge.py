"""Rust-backed bootstrap mixed-output planner (canonical Python bridge).

Call ``plan_bootstrap_mixed_outputs`` here (or via ``offer_bootstrap_policy``), not
``kernel_bridge.bootstrap_kernel()`` directly. Coinset coin dicts are coerced to
``BootstrapCoin`` in this module; the PyO3 layer accepts ``BootstrapCoin`` instances only.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from greenfloor.core import kernel_bridge

if TYPE_CHECKING:
    from greenfloor.offer_bootstrap import BootstrapCoin, BootstrapPlanOutcome, PlannerLadderRow

_KERNEL_REBUILD_HINT = (
    "greenfloor_signer extension is missing plan_bootstrap_mixed_outputs. "
    "Rebuild it (for example: `maturin develop --manifest-path "
    "greenfloor-signer-pyo3/Cargo.toml`)."
)


def _require_bootstrap_planner():
    planner = getattr(kernel_bridge.bootstrap_kernel(), "plan_bootstrap_mixed_outputs", None)
    if planner is None:
        raise RuntimeError(f"{_KERNEL_REBUILD_HINT} Missing symbol: plan_bootstrap_mixed_outputs")
    return planner


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


def plan_bootstrap_mixed_outputs(
    *,
    ladder_entries: list[PlannerLadderRow],
    spendable_coins: list[Any],
) -> BootstrapPlanOutcome:
    """Evaluate bootstrap inventory against denomination ladder rows."""
    from greenfloor.offer_bootstrap import PlannerLadderRow

    if not all(isinstance(row, PlannerLadderRow) for row in ladder_entries):
        raise TypeError("ladder_entries must contain PlannerLadderRow instances")

    outcome = _require_bootstrap_planner()(
        ladder_entries=ladder_entries,
        spendable_coins=_coerce_spendable_coins(spendable_coins),
    )
    return _coerce_planner_outcome(outcome)
