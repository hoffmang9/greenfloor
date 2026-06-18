"""Coinset script IO via ``greenfloor-engine coinset`` CLI subprocess."""

from __future__ import annotations

from greenfloor.adapters.coinset.client import CoinsetAdapter, build_webhook_callback_url

__all__ = [
    "CoinsetAdapter",
    "build_webhook_callback_url",
]
