from __future__ import annotations

import json

from greenfloor.adapters.splash import SplashAdapter


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
