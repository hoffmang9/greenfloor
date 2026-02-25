from __future__ import annotations

import json

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
