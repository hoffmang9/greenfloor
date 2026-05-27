from __future__ import annotations

import io
import json
import logging
import urllib.error
from email.message import Message
from pathlib import Path
from typing import Any

import pytest

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig

from tests.helpers.cloud_wallet_adapter_fixtures import (
    FAKE_KMS_PUBKEY_HEX,
    FakeHttpResponse,
    build_adapter,
    build_kms_adapter,
    write_pem,
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

