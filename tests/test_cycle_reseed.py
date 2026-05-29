"""Cycle reseed engine contract tests."""

from __future__ import annotations

from greenfloor.core.cycle._bridge_orchestration import (
    reseed_skip_reason_labels as rust_reseed_skip_reason_labels,
)
from greenfloor.core.cycle_reseed import ReseedSkipReason, python_reseed_skip_reason_labels


def test_reseed_skip_reason_labels_match_rust_engine() -> None:
    rust_labels = frozenset(rust_reseed_skip_reason_labels())
    python_labels = python_reseed_skip_reason_labels()
    assert rust_labels == python_labels
    assert len(rust_labels) == len(ReseedSkipReason)
