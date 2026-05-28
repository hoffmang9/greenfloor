"""Canonical offer action side normalization."""

from __future__ import annotations

from typing import Any


def normalize_offer_side(value: str | Any | None) -> str:
    side = str(value or "").strip().lower()
    return "buy" if side == "buy" else "sell"
