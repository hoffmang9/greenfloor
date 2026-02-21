from __future__ import annotations

import time
from collections.abc import Callable
from typing import Any


class PriceAdapter:
    def __init__(
        self,
        *,
        ttl_seconds: int = 60,
        url: str = "https://coincodex.com/api/coincodex/get_coin/xch",
        now_fn: Callable[[], float] | None = None,
        session_factory: Callable[[], Any] | None = None,
    ) -> None:
        self.ttl_seconds = max(1, int(ttl_seconds))
        self.url = url
        self._now_fn = now_fn or time.time
        self._session_factory = session_factory
        self._cached_price_usd: float | None = None
        self._cached_at_epoch_s: float | None = None

    async def get_xch_price(self) -> float:
        now = float(self._now_fn())
        if (
            self._cached_price_usd is not None
            and self._cached_at_epoch_s is not None
            and (now - self._cached_at_epoch_s) <= self.ttl_seconds
        ):
            return self._cached_price_usd

        try:
            price = await self._fetch_xch_price()
        except Exception:
            if self._cached_price_usd is not None:
                return self._cached_price_usd
            raise

        self._cached_price_usd = price
        self._cached_at_epoch_s = now
        return price

    async def _fetch_xch_price(self) -> float:
        if self._session_factory is None:
            import aiohttp

            session_cm = aiohttp.ClientSession()
        else:
            session_cm = self._session_factory()

        async with session_cm as session:
            async with session.get(self.url) as response:
                payload = await response.json()

        if isinstance(payload, dict) and "last_price_usd" in payload:
            return float(payload["last_price_usd"])

        if isinstance(payload, list) and payload and isinstance(payload[0], dict):
            if "current_price" in payload[0]:
                return float(payload[0]["current_price"])

        raise ValueError("coincodex_response_missing_price")
