from __future__ import annotations

from unittest.mock import patch

from greenfloor.core.notifications import AlertEvent
from greenfloor.notify.pushover import render_low_inventory_message, send_pushover_alert


def _make_event() -> AlertEvent:
    return AlertEvent(
        market_id="m1",
        ticker="ECO",
        remaining_amount=42,
        receive_address="xch1abc",
        reason="low_triggered",
    )


def _make_program(*, pushover_enabled: bool = True):
    class _Prog:
        pass

    p = _Prog()
    p.pushover_enabled = pushover_enabled  # type: ignore[attr-defined]
    p.pushover_user_key_env = "PO_USER"  # type: ignore[attr-defined]
    p.pushover_app_token_env = "PO_TOKEN"  # type: ignore[attr-defined]
    p.pushover_recipient_key_env = "PO_RECIPIENT"  # type: ignore[attr-defined]
    return p


def test_render_low_inventory_message_contains_key_fields() -> None:
    event = _make_event()
    msg = render_low_inventory_message(event)
    assert "ECO" in msg
    assert "42" in msg
    assert "xch1abc" in msg
    assert "m1" in msg


def test_send_pushover_alert_skips_when_disabled() -> None:
    program = _make_program(pushover_enabled=False)
    with patch("urllib.request.urlopen") as mock_urlopen:
        send_pushover_alert(program, _make_event())  # type: ignore[arg-type]
        mock_urlopen.assert_not_called()


def test_send_pushover_alert_skips_when_missing_keys(monkeypatch) -> None:
    monkeypatch.delenv("PO_USER", raising=False)
    monkeypatch.delenv("PO_TOKEN", raising=False)
    monkeypatch.delenv("PO_RECIPIENT", raising=False)
    program = _make_program()
    with patch("urllib.request.urlopen") as mock_urlopen:
        send_pushover_alert(program, _make_event())  # type: ignore[arg-type]
        mock_urlopen.assert_not_called()


def test_send_pushover_alert_calls_api_when_configured(monkeypatch) -> None:
    monkeypatch.setenv("PO_USER", "user123")
    monkeypatch.setenv("PO_TOKEN", "token456")
    program = _make_program()

    class _FakeResp:
        def __enter__(self):
            return self

        def __exit__(self, *a):
            return None

    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout=None):
        captured["url"] = req.full_url
        captured["data"] = req.data
        captured["method"] = req.get_method()
        return _FakeResp()

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    send_pushover_alert(program, _make_event())  # type: ignore[arg-type]
    assert captured["url"] == "https://api.pushover.net/1/messages.json"
    assert captured["method"] == "POST"
    assert b"token=token456" in captured["data"]  # type: ignore[operator]
    assert b"user=user123" in captured["data"]  # type: ignore[operator]
