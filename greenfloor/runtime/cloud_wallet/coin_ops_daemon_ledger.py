"""Typed daemon coin-op ledger rows (serialized to dict for daemon responses)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Literal


@dataclass(frozen=True, slots=True)
class DaemonCoinOpLedgerItem:
    op_type: str
    size_base_units: int
    op_count: int
    status: Literal["skipped", "executed"]
    reason: str
    operation_id: str | None
    data: dict[str, Any] | None = None

    def to_dict(self) -> dict[str, Any]:
        row: dict[str, Any] = {
            "op_type": self.op_type,
            "size_base_units": int(self.size_base_units),
            "op_count": int(self.op_count),
            "status": self.status,
            "reason": self.reason,
            "operation_id": self.operation_id,
        }
        if self.data is not None:
            row["data"] = self.data
        return row


def daemon_coin_op_skipped(
    *,
    op_type: str,
    size_base_units: int,
    op_count: int,
    reason: str,
    data: dict[str, Any] | None = None,
) -> DaemonCoinOpLedgerItem:
    return DaemonCoinOpLedgerItem(
        op_type=op_type,
        size_base_units=size_base_units,
        op_count=op_count,
        status="skipped",
        reason=reason,
        operation_id=None,
        data=data,
    )


def daemon_coin_op_executed(
    *,
    op_type: str,
    size_base_units: int,
    op_count: int,
    reason: str,
    operation_id: str,
    data: dict[str, Any] | None = None,
) -> DaemonCoinOpLedgerItem:
    return DaemonCoinOpLedgerItem(
        op_type=op_type,
        size_base_units=size_base_units,
        op_count=op_count,
        status="executed",
        reason=reason,
        operation_id=operation_id,
        data=data,
    )
