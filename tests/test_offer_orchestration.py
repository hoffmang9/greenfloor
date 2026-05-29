from __future__ import annotations

from greenfloor.offer_bootstrap import BootstrapPhaseResult
from greenfloor.runtime.offer_orchestration import bootstrap_blocks_offer


def test_bootstrap_blocks_offer_uses_kernel_policy_result(monkeypatch) -> None:
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.bootstrap_block_error",
        lambda **_kwargs: "bootstrap_pending:split_submitted",
    )
    blocked, error = bootstrap_blocks_offer(
        BootstrapPhaseResult(status="executed", reason="split_submitted", ready=False)
    )
    assert blocked is True
    assert error == "bootstrap_pending:split_submitted"


def test_bootstrap_blocks_offer_blocks_underfunded_skip() -> None:
    blocked, error = bootstrap_blocks_offer(
        BootstrapPhaseResult(
            status="skipped",
            reason="bootstrap_underfunded:total_output_amount=20",
            ready=False,
        )
    )
    assert blocked is True
    assert error == "bootstrap_precheck_skipped:bootstrap_underfunded:total_output_amount=20"


def test_bootstrap_blocks_offer_blocks_invalid_ladder_failure() -> None:
    blocked, error = bootstrap_blocks_offer(
        BootstrapPhaseResult(
            status="failed",
            reason="bootstrap_invalid_ladder",
            ready=False,
        )
    )
    assert blocked is True
    assert error == "bootstrap_failed:bootstrap_invalid_ladder"


def test_bootstrap_blocks_offer_allows_offer_when_policy_returns_none(monkeypatch) -> None:
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.bootstrap_block_error",
        lambda **_kwargs: None,
    )
    blocked, error = bootstrap_blocks_offer(
        BootstrapPhaseResult(status="skipped", reason="already_ready", ready=False)
    )
    assert blocked is False
    assert error is None
