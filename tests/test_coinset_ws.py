from __future__ import annotations

import asyncio
import json
import logging

from greenfloor.daemon.coinset_ws import CoinsetWebsocketClient, _classify_payload_tx_ids


def test_classify_payload_tx_ids_marks_confirmed_by_event_hint() -> None:
    tx_id = "a" * 64
    mempool, confirmed = _classify_payload_tx_ids({"event": "tx_confirmed", "tx_id": tx_id})
    assert mempool == []
    assert confirmed == [tx_id]


def test_classify_payload_tx_ids_marks_mempool_when_unconfirmed() -> None:
    tx_id = "b" * 64
    mempool, confirmed = _classify_payload_tx_ids({"event": "mempool_seen", "tx_id": tx_id})
    assert mempool == [tx_id]
    assert confirmed == []


def test_handle_text_message_routes_tx_ids_to_callbacks() -> None:
    mempool_calls: list[list[str]] = []
    confirmed_calls: list[list[str]] = []
    audit_calls: list[tuple[str, dict]] = []
    tx_id = "c" * 64
    client = CoinsetWebsocketClient(
        ws_url="wss://coinset.org/ws",
        reconnect_interval_seconds=1,
        on_mempool_tx_ids=lambda tx_ids: mempool_calls.append(list(tx_ids)),
        on_confirmed_tx_ids=lambda tx_ids: confirmed_calls.append(list(tx_ids)),
        on_audit_event=lambda event_type, payload: audit_calls.append((event_type, payload)),
    )

    client._handle_text_message(json.dumps({"event": "mempool_seen", "tx_id": tx_id}))
    client._handle_text_message(json.dumps({"event": "tx_confirmed", "tx_id": tx_id}))

    assert mempool_calls == [[tx_id]]
    assert confirmed_calls == [[tx_id]]
    assert [event for event, _ in audit_calls] == [
        "coinset_ws_mempool_event",
        "coinset_ws_tx_block_event",
    ]


def test_handle_text_message_emits_parse_error_for_invalid_json() -> None:
    audit_calls: list[tuple[str, dict]] = []
    client = CoinsetWebsocketClient(
        ws_url="wss://coinset.org/ws",
        reconnect_interval_seconds=1,
        on_mempool_tx_ids=lambda _tx_ids: None,
        on_confirmed_tx_ids=lambda _tx_ids: None,
        on_audit_event=lambda event_type, payload: audit_calls.append((event_type, payload)),
    )

    client._handle_text_message("{not-json")

    assert len(audit_calls) == 1
    assert audit_calls[0][0] == "coinset_ws_payload_parse_error"
    assert "raw" in audit_calls[0][1]


def test_handle_text_message_ignores_non_mapping_payload() -> None:
    audit_calls: list[tuple[str, dict]] = []
    client = CoinsetWebsocketClient(
        ws_url="wss://coinset.org/ws",
        reconnect_interval_seconds=1,
        on_mempool_tx_ids=lambda _tx_ids: None,
        on_confirmed_tx_ids=lambda _tx_ids: None,
        on_audit_event=lambda event_type, payload: audit_calls.append((event_type, payload)),
    )

    client._handle_text_message(json.dumps(["not", "a", "mapping"]))

    assert len(audit_calls) == 1
    assert audit_calls[0][0] == "coinset_ws_payload_ignored"
    assert audit_calls[0][1]["kind"] == "list"


def test_run_recovery_poll_emits_count_and_routes_tx_ids() -> None:
    mempool_calls: list[list[str]] = []
    audit_calls: list[tuple[str, dict]] = []
    tx_ids = ["a" * 64, "b" * 64]
    client = CoinsetWebsocketClient(
        ws_url="wss://coinset.org/ws",
        reconnect_interval_seconds=1,
        on_mempool_tx_ids=lambda ids: mempool_calls.append(list(ids)),
        on_confirmed_tx_ids=lambda _tx_ids: None,
        on_audit_event=lambda event_type, payload: audit_calls.append((event_type, payload)),
        recovery_poll=lambda: tx_ids,
    )

    asyncio.run(client._run_recovery_poll(reason="connected"))

    assert mempool_calls == [tx_ids]
    assert audit_calls[-1][0] == "coinset_ws_recovery_poll"
    assert audit_calls[-1][1]["reason"] == "connected"
    assert audit_calls[-1][1]["tx_id_count"] == 2


def test_run_recovery_poll_emits_error_event() -> None:
    audit_calls: list[tuple[str, dict]] = []
    client = CoinsetWebsocketClient(
        ws_url="wss://coinset.org/ws",
        reconnect_interval_seconds=1,
        on_mempool_tx_ids=lambda _ids: None,
        on_confirmed_tx_ids=lambda _tx_ids: None,
        on_audit_event=lambda event_type, payload: audit_calls.append((event_type, payload)),
        recovery_poll=lambda: (_ for _ in ()).throw(RuntimeError("poll_down")),
    )

    asyncio.run(client._run_recovery_poll(reason="reconnect"))

    assert audit_calls[-1][0] == "coinset_ws_recovery_poll_error"
    assert audit_calls[-1][1]["reason"] == "reconnect"
    assert "poll_down" in audit_calls[-1][1]["error"]


def test_run_recovery_poll_logs_warning_on_failure(caplog) -> None:
    client = CoinsetWebsocketClient(
        ws_url="wss://coinset.org/ws",
        reconnect_interval_seconds=1,
        on_mempool_tx_ids=lambda _ids: None,
        on_confirmed_tx_ids=lambda _tx_ids: None,
        on_audit_event=lambda _event_type, _payload: None,
        recovery_poll=lambda: (_ for _ in ()).throw(RuntimeError("poll_down")),
    )
    with caplog.at_level(logging.WARNING, logger="greenfloor.daemon.coinset_ws"):
        asyncio.run(client._run_recovery_poll(reason="reconnect"))
    assert "coinset websocket recovery poll failed: poll_down" in caplog.text


def test_sleep_with_stop_returns_when_stop_set() -> None:
    client = CoinsetWebsocketClient(
        ws_url="wss://coinset.org/ws",
        reconnect_interval_seconds=5,
        on_mempool_tx_ids=lambda _ids: None,
        on_confirmed_tx_ids=lambda _tx_ids: None,
        on_audit_event=lambda _event_type, _payload: None,
    )
    client._stop_event.set()
    asyncio.run(client._sleep_with_stop(5.0))
