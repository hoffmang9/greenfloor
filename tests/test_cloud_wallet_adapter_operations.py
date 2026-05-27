from __future__ import annotations

import io
import json
import urllib.error
from email.message import Message
from pathlib import Path

import pytest

from tests.helpers.cloud_wallet_adapter_fixtures import (
    FakeHttpResponse,
    build_adapter,
)


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


def test_cloud_wallet_get_signature_request_handles_non_dict(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return FakeHttpResponse({"data": {"signatureRequest": "invalid-shape"}})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.get_signature_request(signature_request_id="SignatureRequest_1")
    assert payload == {"id": "SignatureRequest_1", "status": "UNKNOWN"}


def test_cloud_wallet_cancel_offer_returns_signature_request(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHttpResponse(
            {
                "data": {
                    "cancelOffer": {
                        "signatureRequest": {
                            "id": "SignatureRequest_cancel_1",
                            "status": "SUBMITTED",
                        }
                    }
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.cancel_offer(offer_id="Offer_abc")
    assert payload == {
        "signature_request_id": "SignatureRequest_cancel_1",
        "status": "SUBMITTED",
    }
    variables = captured["body"]["variables"]["input"]  # type: ignore[index]
    assert variables["offerId"] == "Offer_abc"  # type: ignore[index]
    assert variables["cancelOffChain"] is False  # type: ignore[index]


def test_cloud_wallet_cancel_offer_off_chain_sets_flag(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHttpResponse({"data": {"cancelOffer": {"signatureRequest": None}}})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.cancel_offer(offer_id="Offer_pending", cancel_off_chain=True)
    assert payload == {"signature_request_id": "", "status": ""}
    variables = captured["body"]["variables"]["input"]  # type: ignore[index]
    assert variables["offerId"] == "Offer_pending"  # type: ignore[index]
    assert variables["cancelOffChain"] is True  # type: ignore[index]


def test_cloud_wallet_create_offer_includes_split_input_coin_options(
    monkeypatch, tmp_path: Path
) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHttpResponse(
            {
                "data": {
                    "createOffer": {
                        "signatureRequest": {
                            "id": "SignatureRequest_create_1",
                            "status": "SUBMITTED",
                        }
                    }
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.create_offer(
        offered=[{"assetId": "Asset_a", "amount": 1}],
        requested=[{"assetId": "Asset_b", "amount": 2}],
        fee=0,
        expires_at_iso="2026-02-26T00:00:00+00:00",
        split_input_coins=True,
        split_input_coins_fee=0,
    )
    assert payload == {
        "signature_request_id": "SignatureRequest_create_1",
        "status": "SUBMITTED",
    }
    variables = captured["body"]["variables"]["input"]  # type: ignore[index]
    assert variables["splitInputCoins"] is True  # type: ignore[index]
    assert variables["splitInputCoinsFee"] == 0  # type: ignore[index]


def test_cloud_wallet_get_wallet_passes_offer_filters(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHttpResponse(
            {
                "data": {
                    "wallet": {
                        "offers": {
                            "edges": [
                                {
                                    "node": {
                                        "id": "WalletOffer_1",
                                        "offerId": "Offer_1",
                                        "state": "OPEN",
                                        "settlementType": "UNSIGNED",
                                        "expiresAt": None,
                                        "bech32": "offer1abc",
                                        "createdAt": "2026-02-26T00:00:00+00:00",
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.get_wallet(is_creator=True, states=["OPEN", "PENDING"], first=25)
    assert len(payload["offers"]) == 1
    variables = captured["body"]["variables"]  # type: ignore[index]
    assert variables["isCreator"] is True  # type: ignore[index]
    assert variables["states"] == ["OPEN", "PENDING"]  # type: ignore[index]
    assert variables["first"] == 25  # type: ignore[index]


def test_cloud_wallet_get_wallet_clamps_first_to_cloud_wallet_max(
    monkeypatch, tmp_path: Path
) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHttpResponse({"data": {"wallet": {"offers": {"edges": []}}}})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.get_wallet(is_creator=True, states=["OPEN"], first=120)
    assert payload == {"offers": []}
    variables = captured["body"]["variables"]  # type: ignore[index]
    assert variables["first"] == 100  # type: ignore[index]


# ---------------------------------------------------------------------------
# split_coins / combine_coins
# ---------------------------------------------------------------------------


def test_cloud_wallet_split_coins_sends_correct_variables(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHttpResponse(
            {
                "data": {
                    "splitCoins": {"signatureRequest": {"id": "SigReq_s1", "status": "UNSIGNED"}}
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    result = adapter.split_coins(
        coin_ids=["Coin_a", "Coin_b"],
        amount_per_coin=100,
        number_of_coins=5,
        fee=500,
    )
    assert result["signature_request_id"] == "SigReq_s1"
    assert result["status"] == "UNSIGNED"
    variables = captured["body"]["variables"]  # type: ignore[index]
    assert variables["coinIds"] == ["Coin_a", "Coin_b"]
    assert variables["amountPerCoin"] == 100
    assert variables["numberOfCoins"] == 5
    assert variables["fee"] == 500
    assert variables["walletId"] == "Wallet_123"


def test_cloud_wallet_combine_coins_sends_correct_variables(monkeypatch, tmp_path: Path) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHttpResponse(
            {
                "data": {
                    "combineCoins": {"signatureRequest": {"id": "SigReq_c1", "status": "SUBMITTED"}}
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    result = adapter.combine_coins(
        number_of_coins=3,
        fee=1000,
        largest_first=False,
        asset_id="Asset_xyz",
        input_coin_ids=["Coin_1", "Coin_2"],
        target_amount=500,
    )
    assert result["signature_request_id"] == "SigReq_c1"
    assert result["status"] == "SUBMITTED"
    variables = captured["body"]["variables"]  # type: ignore[index]
    assert variables["numberOfCoins"] == 3
    assert variables["fee"] == 1000
    assert variables["largestFirst"] is False
    assert variables["assetId"] == "Asset_xyz"
    assert variables["inputCoinIds"] == ["Coin_1", "Coin_2"]
    assert variables["targetAmount"] == 500


def test_cloud_wallet_combine_coins_optional_fields_default_none(
    monkeypatch, tmp_path: Path
) -> None:
    adapter = build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHttpResponse(
            {
                "data": {
                    "combineCoins": {"signatureRequest": {"id": "SigReq_c2", "status": "UNSIGNED"}}
                }
            }
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    result = adapter.combine_coins(number_of_coins=2, fee=0)
    assert result["signature_request_id"] == "SigReq_c2"
    variables = captured["body"]["variables"]  # type: ignore[index]
    assert variables["targetAmount"] is None
    assert variables["inputCoinIds"] is None
    assert variables["assetId"] is None
