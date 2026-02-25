from __future__ import annotations

import json
import urllib.error
from email.message import Message

import pytest

from greenfloor.adapters.splash import SplashAdapter


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


def test_splash_post_offer_posts_expected_payload(monkeypatch) -> None:
    adapter = SplashAdapter("http://localhost:4000")
    captured = {}

    def _fake_urlopen(req, timeout=0):
        captured["request"] = req
        captured["timeout"] = timeout
        return _FakeHttpResponse({"success": True, "id": "splash-1"})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    out = adapter.post_offer("offer1abc")

    assert out["success"] is True
    req = captured["request"]
    assert req.full_url == "http://localhost:4000"
    assert req.get_method() == "POST"
    assert captured["timeout"] == 30
    assert req.get_header("Content-type") == "application/json"
    assert json.loads(req.data.decode("utf-8")) == {"offer": "offer1abc"}


def test_splash_post_offer_non_mapping_response_returns_invalid_format(monkeypatch) -> None:
    adapter = SplashAdapter("http://localhost:4000")

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        return _FakeHttpResponse(["unexpected"])

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    out = adapter.post_offer("offer1abc")
    assert out == {"success": False, "error": "invalid_response_format"}


def test_splash_post_offer_propagates_http_error(monkeypatch) -> None:
    adapter = SplashAdapter("http://localhost:4000")

    def _fake_urlopen(_req, timeout=0):
        _ = timeout
        raise urllib.error.HTTPError(
            url="http://localhost:4000",
            code=502,
            msg="bad_gateway",
            hdrs=Message(),
            fp=None,
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    with pytest.raises(urllib.error.HTTPError):
        adapter.post_offer("offer1abc")
