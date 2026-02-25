from __future__ import annotations

import io
import json
import urllib.error
from email.message import Message
from pathlib import Path

import pytest

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig


class _FakeHttpResponse:
    def __init__(self, payload) -> None:
        self._payload = payload

    def read(self) -> bytes:
        return json.dumps(self._payload).encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        _ = exc_type, exc, tb
        return None


def _build_adapter(tmp_path: Path) -> CloudWalletAdapter:
    pem_path = tmp_path / "cloud-wallet-key.pem"
    pem_path.write_text(
        "\n".join(
            [
                "-----BEGIN PRIVATE KEY-----",
                "not-a-real-key",
                "-----END PRIVATE KEY-----",
            ]
        ),
        encoding="utf-8",
    )
    return CloudWalletAdapter(
        CloudWalletConfig(
            base_url="https://wallet.example.com",
            user_key_id="key-1",
            private_key_pem_path=str(pem_path),
            vault_id="Wallet_123",
            network="mainnet",
        )
    )


def test_cloud_wallet_list_coins_paginates_and_accumulates(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
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
                        {"node": {"id": "Coin_2", "name": "22", "amount": 20, "state": "CONFIRMED"}},
                        {"node": "not-a-dict"},
                    ],
                }
            }
        },
    ]

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return _FakeHttpResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    coins = adapter.list_coins()
    assert [c["id"] for c in coins] == ["Coin_1", "Coin_2"]


def test_cloud_wallet_list_coins_stops_on_missing_end_cursor(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})
    calls = {"n": 0}

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        calls["n"] += 1
        return _FakeHttpResponse(
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


def test_cloud_wallet_graphql_http_error_contains_status_and_snippet(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
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
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        raise urllib.error.URLError("offline")

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(RuntimeError, match=r"cloud_wallet_network_error:offline"):
        adapter._graphql(query="query test {}", variables={})


def test_cloud_wallet_graphql_error_payload_raises(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return _FakeHttpResponse({"errors": [{"message": "bad request"}]})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(RuntimeError, match=r"cloud_wallet_graphql_error:bad request"):
        adapter._graphql(query="query test {}", variables={})


def test_cloud_wallet_graphql_missing_data_raises(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return _FakeHttpResponse({"data": []})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(RuntimeError, match=r"cloud_wallet_missing_data"):
        adapter._graphql(query="query test {}", variables={})


def test_cloud_wallet_get_signature_request_handles_non_dict(monkeypatch, tmp_path: Path) -> None:
    adapter = _build_adapter(tmp_path)
    monkeypatch.setattr(adapter, "_build_auth_headers", lambda _body: {})

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return _FakeHttpResponse({"data": {"signatureRequest": "invalid-shape"}})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    payload = adapter.get_signature_request(signature_request_id="SignatureRequest_1")
    assert payload == {"id": "SignatureRequest_1", "status": "UNKNOWN"}
