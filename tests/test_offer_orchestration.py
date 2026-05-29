from __future__ import annotations

from greenfloor.runtime.offer_orchestration import bootstrap_blocks_offer


def test_bootstrap_blocks_offer_uses_kernel_policy_result(monkeypatch) -> None:
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.bootstrap_block_error",
        lambda **_kwargs: "bootstrap_pending:split_submitted",
    )
    blocked, error = bootstrap_blocks_offer(
        {"status": "executed", "reason": "split_submitted", "ready": False}
    )
    assert blocked is True
    assert error == "bootstrap_pending:split_submitted"


def test_bootstrap_blocks_offer_allows_offer_when_policy_returns_none(monkeypatch) -> None:
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.bootstrap_block_error",
        lambda **_kwargs: None,
    )
    blocked, error = bootstrap_blocks_offer(
        {"status": "skipped", "reason": "already_ready", "ready": False}
    )
    assert blocked is False
    assert error is None
