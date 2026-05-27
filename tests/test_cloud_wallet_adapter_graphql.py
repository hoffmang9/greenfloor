from __future__ import annotations

import io
import logging
import urllib.error
from email.message import Message
from pathlib import Path
from typing import Any

import pytest

from tests.helpers.cloud_wallet_adapter_fixtures import (
    FakeHttpResponse,
    build_adapter,
)


def test_cloud_wallet_graphql_ok_log_includes_operation_and_duration(
    monkeypatch, tmp_path: Path, caplog: pytest.LogCaptureFixture
) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    monkeypatch.setattr(
        "urllib.request.urlopen",
        lambda *_a, **_k: FakeHttpResponse(
            {
                "data": {
                    "coins": {
                        "pageInfo": {"hasNextPage": False, "endCursor": ""},
                        "edges": [],
                    }
                }
            }
        ),
    )
    with caplog.at_level(logging.INFO, logger="greenfloor.adapters.cloud_wallet"):
        adapter.list_coins()
    assert any(
        "cloud_wallet_graphql_ok" in rec.message
        and "operation=query_listCoins" in rec.message
        and "duration_ms=" in rec.message
        for rec in caplog.records
    )


def test_cloud_wallet_list_coins_paginates_and_accumulates(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    responses = [
        {
            "data": {
                "coins": {
                    "pageInfo": {"hasNextPage": True, "endCursor": "cursor-1"},
                    "edges": [
                        {"node": {"id": "Coin_1", "name": "11", "amount": 10, "state": "CONFIRMED"}}
                    ],
                }
            }
        },
        {
            "data": {
                "coins": {
                    "pageInfo": {"hasNextPage": False, "endCursor": ""},
                    "edges": [
                        {
                            "node": {
                                "id": "Coin_2",
                                "name": "22",
                                "amount": 20,
                                "state": "CONFIRMED",
                            }
                        },
                        {"node": "not-a-dict"},
                    ],
                }
            }
        },
    ]

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    coins = adapter.list_coins()
    assert [c["id"] for c in coins] == ["Coin_1", "Coin_2"]


def test_cloud_wallet_list_coins_stops_on_missing_end_cursor(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    calls = {"n": 0}

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        calls["n"] += 1
        return FakeHttpResponse(
            {
                "data": {
                    "coins": {
                        "pageInfo": {"hasNextPage": True, "endCursor": ""},
                        "edges": [{"node": {"id": "Coin_1", "name": "11", "amount": 10}}],
                    }
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    coins = adapter.list_coins()
    assert len(coins) == 1
    assert calls["n"] == 1


def test_cloud_wallet_list_coins_omits_row_asset_for_asset_scoped_queries(
    monkeypatch, tmp_path: Path
) -> None:
    adapter = build_adapter(tmp_path)
    queries: list[str] = []

    def _fake_graphql(*, query, variables):
        queries.append(query)
        assert variables["assetId"] == "Asset_byc"
        assert variables["includePending"] is False
        assert variables["minAmount"] == "1000"
        return {
            "coins": {
                "pageInfo": {"hasNextPage": False, "endCursor": ""},
                "edges": [{"node": {"id": "Coin_1", "name": "11", "amount": 10}}],
            }
        }

    monkeypatch.setattr(adapter, "_graphql", _fake_graphql)
    coins = adapter.list_coins(asset_id="Asset_byc")
    assert len(coins) == 1
    assert "asset {" not in queries[0]
    assert "isLinkedToOpenOffer" in queries[0]


def test_cloud_wallet_list_coins_keeps_row_asset_for_unscoped_queries(
    monkeypatch, tmp_path: Path
) -> None:
    adapter = build_adapter(tmp_path)
    queries: list[str] = []

    def _fake_graphql(*, query, variables):
        queries.append(query)
        assert variables["assetId"] is None
        assert variables["includePending"] is False
        assert variables["minAmount"] == "1000"
        return {
            "coins": {
                "pageInfo": {"hasNextPage": False, "endCursor": ""},
                "edges": [{"node": {"id": "Coin_1", "name": "11", "amount": 10}}],
            }
        }

    monkeypatch.setattr(adapter, "_graphql", _fake_graphql)
    coins = adapter.list_coins()
    assert len(coins) == 1
    assert "asset {" in queries[0]


def test_cloud_wallet_list_coins_opt_out_pending_and_min_filter(
    monkeypatch, tmp_path: Path
) -> None:
    adapter = build_adapter(tmp_path)
    captured: dict[str, Any] = {}

    def _fake_graphql(*, query, variables):
        captured["variables"] = dict(variables)
        return {
            "coins": {
                "pageInfo": {"hasNextPage": False, "endCursor": ""},
                "edges": [],
            }
        }

    monkeypatch.setattr(adapter, "_graphql", _fake_graphql)
    adapter.list_coins(include_pending=True, min_amount_mojos=None)
    assert captured["variables"]["includePending"] is True
    assert captured["variables"]["minAmount"] is None


def test_cloud_wallet_get_chia_usd_quote_reads_numeric_price(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(
        adapter,
        "_graphql",
        lambda *, query, variables: {  # noqa: ARG005
            "quote": {
                "price": "31.42",
                "baseAsset": "chia",
                "currency": "usd",
                "source": "coingecko.com",
                "createdAt": "2026-03-10T12:00:00Z",
            }
        },
    )
    assert adapter.get_chia_usd_quote() == 31.42


def test_cloud_wallet_get_chia_usd_quote_rejects_missing_quote(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_graphql", lambda *, query, variables: {"quote": None})  # noqa: ARG005
    with pytest.raises(RuntimeError, match="cloud_wallet_missing_quote"):
        adapter.get_chia_usd_quote()


def test_cloud_wallet_graphql_http_error_contains_status_and_snippet(
    monkeypatch, tmp_path: Path
) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        raise urllib.error.HTTPError(
            req.full_url,
            500,
            "server error",
            Message(),
            io.BytesIO(b'{"error":"boom"}'),
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(RuntimeError, match=r"cloud_wallet_http_error:500:"):
        adapter._graphql(query="query test {}", variables={})


def test_cloud_wallet_graphql_network_error_is_classified(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        raise urllib.error.URLError("offline")

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(RuntimeError, match=r"cloud_wallet_network_error:offline"):
        adapter._graphql(query="query test {}", variables={})


def test_cloud_wallet_graphql_error_payload_raises(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return FakeHttpResponse({"errors": [{"message": "bad request"}]})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(RuntimeError, match=r"cloud_wallet_graphql_error:bad request"):
        adapter._graphql(query="query test {}", variables={})


def test_cloud_wallet_graphql_missing_data_raises(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return FakeHttpResponse({"data": []})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(RuntimeError, match=r"cloud_wallet_missing_data"):
        adapter._graphql(query="query test {}", variables={})


def test_cloud_wallet_graphql_retries_http_429_with_backoff(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    calls = {"n": 0}
    sleeps: list[float] = []

    def _fake_sleep(seconds: float) -> None:
        sleeps.append(float(seconds))

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        calls["n"] += 1
        if calls["n"] == 1:
            headers = Message()
            headers["Retry-After"] = "3"
            raise urllib.error.HTTPError(
                req.full_url,
                429,
                "too many requests",
                headers,
                io.BytesIO(b'{"error":"rate limited"}'),
            )
        if calls["n"] == 2:
            raise urllib.error.HTTPError(
                req.full_url,
                429,
                "too many requests",
                Message(),
                io.BytesIO(b'{"error":"rate limited"}'),
            )
        return FakeHttpResponse({"data": {"ok": True}})

    monkeypatch.setattr("time.sleep", _fake_sleep)
    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter._graphql(query="query test {}", variables={})
    assert payload == {"ok": True}
    assert calls["n"] == 3
    assert sleeps == [3.0, 2.0]


def test_cloud_wallet_graphql_retries_rate_limit_error_payload(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    sleeps: list[float] = []
    responses = [
        {"errors": [{"message": "Rate limit exceeded, please try again in 2 seconds"}]},
        {"errors": [{"message": "Rate limit exceeded, please try again in 2 seconds"}]},
        {"data": {"ok": True}},
    ]

    def _fake_sleep(seconds: float) -> None:
        sleeps.append(float(seconds))

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("time.sleep", _fake_sleep)
    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter._graphql(query="query test {}", variables={})
    assert payload == {"ok": True}
    assert sleeps == [2.0, 2.0]


def test_cloud_wallet_graphql_retries_transient_http_503(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    sleeps: list[float] = []
    calls = {"n": 0}

    def _fake_sleep(seconds: float) -> None:
        sleeps.append(float(seconds))

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        calls["n"] += 1
        if calls["n"] < 3:
            raise urllib.error.HTTPError(
                req.full_url,
                503,
                "service unavailable",
                Message(),
                io.BytesIO(b"<html><h1>503 Service Temporarily Unavailable</h1></html>"),
            )
        return FakeHttpResponse({"data": {"ok": True}})

    monkeypatch.setattr("time.sleep", _fake_sleep)
    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter._graphql(query="query test {}", variables={})
    assert payload == {"ok": True}
    assert calls["n"] == 3
    assert sleeps == [1.0, 2.0]


def test_cloud_wallet_graphql_retries_transient_network_timeout(
    monkeypatch, tmp_path: Path
) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    sleeps: list[float] = []
    calls = {"n": 0}

    def _fake_sleep(seconds: float) -> None:
        sleeps.append(float(seconds))

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        calls["n"] += 1
        if calls["n"] == 1:
            raise urllib.error.URLError(TimeoutError("The read operation timed out"))
        return FakeHttpResponse({"data": {"ok": True}})

    monkeypatch.setattr("time.sleep", _fake_sleep)
    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter._graphql(query="query test {}", variables={})
    assert payload == {"ok": True}
    assert calls["n"] == 2
    assert sleeps == [1.0]


def test_cloud_wallet_graphql_retries_direct_timeout_error(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    sleeps: list[float] = []
    calls = {"n": 0}

    def _fake_sleep(seconds: float) -> None:
        sleeps.append(float(seconds))

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        calls["n"] += 1
        if calls["n"] == 1:
            raise TimeoutError("The read operation timed out")
        return FakeHttpResponse({"data": {"ok": True}})

    monkeypatch.setattr("time.sleep", _fake_sleep)
    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter._graphql(query="query test {}", variables={})
    assert payload == {"ok": True}
    assert calls["n"] == 2
    assert sleeps == [1.0]


def test_cloud_wallet_graphql_refreshes_auth_headers_on_retry(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    header_calls: list[str] = []
    calls = {"n": 0}

    def _fake_build_auth_headers(_body: str) -> dict[str, str]:
        nonce = f"n-{len(header_calls)}"
        header_calls.append(nonce)
        return {
            "chia-user-key-id": "key-1",
            "chia-signature": "sig",
            "chia-nonce": nonce,
            "chia-timestamp": "123",
        }

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        calls["n"] += 1
        if calls["n"] == 1:
            raise urllib.error.HTTPError(
                req.full_url,
                503,
                "service unavailable",
                Message(),
                io.BytesIO(b"<html><h1>503 Service Temporarily Unavailable</h1></html>"),
            )
        return FakeHttpResponse({"data": {"ok": True}})

    monkeypatch.setattr(adapter, "_build_auth_headers", _fake_build_auth_headers)
    monkeypatch.setattr("time.sleep", lambda _seconds: None)
    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter._graphql(query="query test {}", variables={})
    assert payload == {"ok": True}
    assert calls["n"] == 2
    assert header_calls == ["n-0", "n-1"]
