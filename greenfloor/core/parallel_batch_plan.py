"""Parallel managed-offer batch plan types (Rust FFI + dispatch planning)."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class ParallelSkipItem:
    submit_index: int
    reason: str


@dataclass(frozen=True, slots=True)
class ParallelQueueItem:
    submit_index: int
    requested_amounts: dict[str, int]
    available_amounts: dict[str, int]


@dataclass(frozen=True, slots=True)
class ParallelBatchPlan:
    skip_items: list[ParallelSkipItem]
    queue: list[ParallelQueueItem]
