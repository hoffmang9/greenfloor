from __future__ import annotations

import json
from pathlib import Path

from greenfloor.adapters.dexie import DexieAdapter


class _FakeHttpResponse:
    def __init__(self, payload: dict) -> None:
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
