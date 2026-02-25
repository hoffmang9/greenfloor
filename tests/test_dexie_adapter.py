from __future__ import annotations

import io
import json
import urllib.error
from email.message import Message
from pathlib import Path

import pytest

from greenfloor.adapters.dexie import DexieAdapter


class _FakeHttpResponse:
    def __init__(self, payload: object) -> None:
        self._raw = json.dumps(payload).encode("utf-8")

    def read(self) -> bytes:
        return self._raw

    def __enter__(self) -> _FakeHttpResponse:
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        _ = exc_type, exc, tb
        return None


def _fixture_path(name: str) -> Path:
    return Path(__file__).parent / "fixtures" / "dexie" / name


def test_dexie_post_offer_uses_fixture_payload_and_returns_id(monkeypatch) -> None:
    adapter = DexieAdapter("https://api.dexie.space")
    fixture_offer = _fixture_path("sample.offer").read_text(encoding="utf-8").strip()
    assert fixture_offer.startswith("offer1")
    fixture_response = json.loads(
        _fixture_path("post_offer_response.json").read_text(encoding="utf-8")
    )

    captured = {}

    def _fake_urlopen(req, timeout=0):
        captured["request"] = req
        captured["timeout"] = timeout
        return _FakeHttpResponse(fixture_response)

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)

    out = adapter.post_offer(fixture_offer, drop_only=True, claim_rewards=False)
    assert out == fixture_response

    req = captured["request"]
    assert req.full_url == "https://api.dexie.space/v1/offers"
    assert req.get_method() == "POST"
    assert captured["timeout"] == 20
    assert req.get_header("Content-type") == "application/json"
    assert json.loads(req.data.decode("utf-8")) == {
        "offer": fixture_offer,
        "drop_only": True,
        "claim_rewards": False,
    }


def test_dexie_cancel_offer_posts_cancel_endpoint_and_id(monkeypatch) -> None:
    adapter = DexieAdapter("https://api.dexie.space")
    fixture_response = json.loads(
        _fixture_path("cancel_offer_response.json").read_text(encoding="utf-8")
    )

    captured = {}

    def _fake_urlopen(req, timeout=0):
        captured["request"] = req
        captured["timeout"] = timeout
        return _FakeHttpResponse(fixture_response)

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)

    out = adapter.cancel_offer("  offer-id-123  ")
    assert out == fixture_response

    req = captured["request"]
    assert req.full_url == "https://api.dexie.space/v1/offers/offer-id-123/cancel"
    assert req.get_method() == "POST"
    assert captured["timeout"] == 20
    assert req.get_header("Content-type") == "application/json"
    assert json.loads(req.data.decode("utf-8")) == {"id": "offer-id-123"}


def test_dexie_post_offer_http_error_is_classified(monkeypatch) -> None:
    adapter = DexieAdapter("https://api.dexie.space")

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        raise urllib.error.HTTPError(
            req.full_url,
            400,
            "bad_request",
            Message(),
            io.BytesIO(b'{"message":"invalid offer"}'),
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    out = adapter.post_offer("offer1abc")
    assert out["success"] is False
    assert out["error"].startswith("dexie_http_error:400:")
    assert "invalid offer" in out["error"]


def test_dexie_post_offer_network_error_is_classified(monkeypatch) -> None:
    adapter = DexieAdapter("https://api.dexie.space")

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        raise urllib.error.URLError("offline")

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    out = adapter.post_offer("offer1abc")
    assert out == {"success": False, "error": "dexie_network_error:offline"}


def test_dexie_post_offer_non_mapping_response_is_invalid_format(monkeypatch) -> None:
    adapter = DexieAdapter("https://api.dexie.space")

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return _FakeHttpResponse(["unexpected-list-shape"])

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    out = adapter.post_offer("offer1abc")
    assert out == {"success": False, "error": "invalid_response_format"}


def test_dexie_get_offer_requires_non_empty_offer_id() -> None:
    adapter = DexieAdapter("https://api.dexie.space")
    with pytest.raises(ValueError, match="offer_id is required"):
        adapter.get_offer("   ")
