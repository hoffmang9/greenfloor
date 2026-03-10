from __future__ import annotations

import asyncio

import pytest

from greenfloor.adapters.price import PriceAdapter, XchPriceProvider


class _FakeResponse:
    def __init__(self, payload):
        self._payload = payload

    async def json(self):
        if isinstance(self._payload, Exception):
            raise self._payload
        return self._payload

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        _ = exc_type, exc, tb
        return None


class _FakeSession:
    def __init__(self, payloads, call_counter):
        self._payloads = payloads
        self._counter = call_counter

    def get(self, _url):
        self._counter["count"] += 1
        idx = self._counter["count"] - 1
        payload = self._payloads[idx]
        return _FakeResponse(payload)

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        _ = exc_type, exc, tb
        return None


def test_get_xch_price_uses_ttl_cache() -> None:
    now = {"value": 1_000.0}
    counter = {"count": 0}
    payloads = [{"last_price_usd": "31.25"}]

    adapter = PriceAdapter(
        ttl_seconds=60,
        now_fn=lambda: now["value"],
        session_factory=lambda: _FakeSession(payloads, counter),
    )

    first = asyncio.run(adapter.get_xch_price())
    second = asyncio.run(adapter.get_xch_price())

    assert first == 31.25
    assert second == 31.25
    assert counter["count"] == 1


def test_get_xch_price_refreshes_after_ttl() -> None:
    now = {"value": 1_000.0}
    counter = {"count": 0}
    payloads = [{"last_price_usd": "31.25"}, {"last_price_usd": "32.00"}]

    adapter = PriceAdapter(
        ttl_seconds=60,
        now_fn=lambda: now["value"],
        session_factory=lambda: _FakeSession(payloads, counter),
    )

    first = asyncio.run(adapter.get_xch_price())
    now["value"] = 1_061.0
    second = asyncio.run(adapter.get_xch_price())

    assert first == 31.25
    assert second == 32.0
    assert counter["count"] == 2


def test_get_xch_price_returns_stale_cache_on_fetch_failure() -> None:
    now = {"value": 1_000.0}
    counter = {"count": 0}
    payloads = [{"last_price_usd": "31.25"}, RuntimeError("upstream timeout")]

    adapter = PriceAdapter(
        ttl_seconds=60,
        now_fn=lambda: now["value"],
        session_factory=lambda: _FakeSession(payloads, counter),
    )

    fresh = asyncio.run(adapter.get_xch_price())
    now["value"] = 1_061.0
    stale = asyncio.run(adapter.get_xch_price())

    assert fresh == 31.25
    assert stale == 31.25
    assert counter["count"] == 2


def test_get_xch_price_raises_when_no_cache_and_fetch_fails() -> None:
    adapter = PriceAdapter(
        ttl_seconds=60,
        now_fn=lambda: 1_000.0,
        session_factory=lambda: _FakeSession([RuntimeError("offline")], {"count": 0}),
    )

    with pytest.raises(RuntimeError, match="offline"):
        asyncio.run(adapter.get_xch_price())


def test_xch_price_provider_prefers_cloud_wallet_and_uses_ttl_cache() -> None:
    now = {"value": 1_000.0}
    calls = {"count": 0}

    def _cloud_wallet_quote() -> float:
        calls["count"] += 1
        return 42.0

    provider = XchPriceProvider(
        cloud_wallet_price_fn=_cloud_wallet_quote,
        cloud_wallet_ttl_seconds=120,
        fallback_price_adapter=PriceAdapter(
            ttl_seconds=60,
            now_fn=lambda: now["value"],
            session_factory=lambda: _FakeSession([{"last_price_usd": "31.25"}], {"count": 0}),
        ),
        now_fn=lambda: now["value"],
    )

    first = asyncio.run(provider.get_xch_price())
    second = asyncio.run(provider.get_xch_price())

    assert first == 42.0
    assert second == 42.0
    assert calls["count"] == 1


def test_xch_price_provider_falls_back_to_coincodex_when_cloud_wallet_fails() -> None:
    fallback_counter = {"count": 0}
    provider = XchPriceProvider(
        cloud_wallet_price_fn=lambda: (_ for _ in ()).throw(RuntimeError("cw down")),
        fallback_price_adapter=PriceAdapter(
            ttl_seconds=60,
            now_fn=lambda: 1_000.0,
            session_factory=lambda: _FakeSession([{"last_price_usd": "33.50"}], fallback_counter),
        ),
        now_fn=lambda: 1_000.0,
    )

    value = asyncio.run(provider.get_xch_price())
    assert value == 33.5
    assert fallback_counter["count"] == 1
