"""Canonical offer action side normalization."""

from __future__ import annotations

from typing import Any

from greenfloor.core.kernel_bridge import import_kernel


def normalize_offer_side(value: str | Any | None) -> str:
    return str(import_kernel().normalize_offer_side(str(value or "")))
