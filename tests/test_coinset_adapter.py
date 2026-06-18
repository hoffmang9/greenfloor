from __future__ import annotations

from typing import Any

import pytest

from greenfloor.adapters.coinset import (
    CoinsetAdapter,
    build_webhook_callback_url,
    extract_coin_ids_from_offer_payload,
    extract_coinset_tx_ids_from_offer_payload,
)
from tests.helpers.coinset_cli_mock import make_coinset_cli_handler


@pytest.fixture(autouse=True)
def _mock_rust_coinset_io(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        "greenfloor.adapters.coinset_engine.run_engine_json",
        make_coinset_cli_handler(),
    )


def test_coinset_adapter_defaults_to_mainnet() -> None:
    adapter = CoinsetAdapter()
    assert adapter.base_url == CoinsetAdapter.MAINNET_BASE_URL


def test_coinset_adapter_network_testnet11() -> None:
    adapter = CoinsetAdapter(network="testnet11")
    assert adapter.base_url == CoinsetAdapter.TESTNET11_BASE_URL


def test_coinset_adapter_network_testnet_alias() -> None:
    adapter = CoinsetAdapter(network="testnet")
    assert adapter.network == "testnet11"
    assert adapter.base_url == CoinsetAdapter.TESTNET11_BASE_URL


def test_coinset_adapter_custom_base_url_overrides_network() -> None:
    adapter = CoinsetAdapter("https://coinset.custom", network="testnet11")
    assert adapter.base_url == "https://coinset.custom"


def test_coinset_adapter_get_all_mempool_tx_ids_uses_post_cli(monkeypatch) -> None:
    captured: dict[str, object] = {}

    def _post_handler(endpoint: str, body: dict[str, Any]) -> Any:
        captured["endpoint"] = endpoint
        captured["body"] = body
        return {"success": True, "mempool_tx_ids": ["0xabc", "0xdef"]}

    monkeypatch.setattr(
        "greenfloor.adapters.coinset_engine.run_engine_json",
        make_coinset_cli_handler(post_handler=_post_handler),
    )
    adapter = CoinsetAdapter("https://coinset.org")
    tx_ids = adapter.get_all_mempool_tx_ids()
    assert tx_ids == ["0xabc", "0xdef"]
    assert captured["endpoint"] == "get_all_mempool_tx_ids"
    assert captured["body"] == {}


def test_coinset_adapter_get_coin_records_by_puzzle_hash_filters_non_dicts(
    monkeypatch,
) -> None:
    def _post_handler(endpoint: str, _body: dict[str, Any]) -> Any:
        assert endpoint == "get_coin_records_by_puzzle_hash"
        return {"success": True, "coin_records": [{"coin": {"amount": 1}}, "bad"]}

    monkeypatch.setattr(
        "greenfloor.adapters.coinset_engine.run_engine_json",
        make_coinset_cli_handler(post_handler=_post_handler),
    )
    adapter = CoinsetAdapter()
    records = adapter.get_coin_records_by_puzzle_hash(
        puzzle_hash_hex="0x11", include_spent_coins=False
    )
    assert records == [{"coin": {"amount": 1}}]


def test_coinset_adapter_get_coin_record_by_name_success_and_failure(
    monkeypatch,
) -> None:
    responses = [
        {"success": True, "coin_record": {"coin": {"amount": 123}}},
        {"success": False, "error": "not_found"},
    ]

    def _post_handler(endpoint: str, _body: dict[str, Any]) -> Any:
        assert endpoint == "get_coin_record_by_name"
        return responses.pop(0)

    monkeypatch.setattr(
        "greenfloor.adapters.coinset_engine.run_engine_json",
        make_coinset_cli_handler(post_handler=_post_handler),
    )
    adapter = CoinsetAdapter()
    assert adapter.get_coin_record_by_name(coin_name_hex="0x22") == {"coin": {"amount": 123}}
    assert adapter.get_coin_record_by_name(coin_name_hex="0x33") is None


def test_coinset_adapter_get_puzzle_and_solution_adds_height_when_provided(
    monkeypatch,
) -> None:
    captured_bodies: list[dict[str, Any]] = []

    def _post_handler(endpoint: str, body: dict[str, Any]) -> Any:
        assert endpoint == "get_puzzle_and_solution"
        captured_bodies.append(body)
        return {
            "success": True,
            "coin_solution": {"puzzle_reveal": "80", "solution": "80"},
        }

    monkeypatch.setattr(
        "greenfloor.adapters.coinset_engine.run_engine_json",
        make_coinset_cli_handler(post_handler=_post_handler),
    )
    adapter = CoinsetAdapter()
    solution = adapter.get_puzzle_and_solution(coin_id_hex="0x44", height=50)
    assert solution == {"puzzle_reveal": "80", "solution": "80"}
    assert captured_bodies[0] == {"coin_id": "0x44", "height": 50}


def test_coinset_adapter_get_puzzle_and_solution_omits_non_positive_height(
    monkeypatch,
) -> None:
    captured_bodies: list[dict[str, Any]] = []

    def _post_handler(endpoint: str, body: dict[str, Any]) -> Any:
        assert endpoint == "get_puzzle_and_solution"
        captured_bodies.append(body)
        return {
            "success": True,
            "coin_solution": {"puzzle_reveal": "80", "solution": "80"},
        }

    monkeypatch.setattr(
        "greenfloor.adapters.coinset_engine.run_engine_json",
        make_coinset_cli_handler(post_handler=_post_handler),
    )
    adapter = CoinsetAdapter()
    _ = adapter.get_puzzle_and_solution(coin_id_hex="0x55", height=0)
    assert captured_bodies[0] == {"coin_id": "0x55"}


def test_push_tx_returns_payload_dict() -> None:
    adapter = CoinsetAdapter()
    result = adapter.push_tx(spend_bundle_hex="0xdeadbeef")
    assert result["success"] is True
    assert result["status"] == "submitted"


def test_extract_tx_ids_from_flat_payload() -> None:
    payload = {"tx_id": "a" * 64, "other": "ignored"}
    assert extract_coinset_tx_ids_from_offer_payload(payload) == ["a" * 64]


def test_extract_tx_ids_from_nested_payload() -> None:
    payload = {"offer": {"data": {"takeTxId": "b" * 64}}}
    assert extract_coinset_tx_ids_from_offer_payload(payload) == ["b" * 64]


def test_extract_tx_ids_deduplicates() -> None:
    tx = "c" * 64
    payload = {"tx_id": tx, "nested": {"tx_id": tx}}
    assert extract_coinset_tx_ids_from_offer_payload(payload) == [tx]


def test_extract_tx_ids_handles_list_values() -> None:
    tx1, tx2 = "d" * 64, "e" * 64
    payload = {"mempool_tx_ids": [tx1, tx2]}
    result = extract_coinset_tx_ids_from_offer_payload(payload)
    assert result == [tx1, tx2]


def test_extract_tx_ids_ignores_non_hex() -> None:
    payload = {"tx_id": "not-a-valid-tx-id"}
    assert extract_coinset_tx_ids_from_offer_payload(payload) == []


def test_extract_tx_ids_ignores_wrong_length() -> None:
    payload = {"tx_id": "abcd"}
    assert extract_coinset_tx_ids_from_offer_payload(payload) == []


def test_extract_tx_ids_empty_payload() -> None:
    assert extract_coinset_tx_ids_from_offer_payload({}) == []


def test_extract_coin_ids_from_involved_coins_and_nested_inputs() -> None:
    coin_a = "a" * 64
    coin_b = "b" * 64
    payload = {
        "involved_coins": [f"0x{coin_a}", coin_b],
        "input_coins": {
            "xch": [
                {"id": f"0x{coin_a}"},
                {"name": coin_b},
            ]
        },
    }
    assert extract_coin_ids_from_offer_payload(payload) == [coin_a, coin_b]


def test_extract_coin_ids_ignores_non_hash_values() -> None:
    payload = {
        "involved_coins": ["not-a-coin", "0x1234"],
        "input_coins": {"xch": [{"id": "short"}]},
    }
    assert extract_coin_ids_from_offer_payload(payload) == []


def test_get_blockchain_state_success(monkeypatch) -> None:
    def _post_handler(endpoint: str, _body: dict[str, Any]) -> Any:
        assert endpoint == "get_blockchain_state"
        return {"success": True, "blockchain_state": {"peak_height": 1234}}

    monkeypatch.setattr(
        "greenfloor.adapters.coinset_engine.run_engine_json",
        make_coinset_cli_handler(post_handler=_post_handler),
    )
    adapter = CoinsetAdapter()
    state = adapter.get_blockchain_state()
    assert state == {"peak_height": 1234}


def test_get_blockchain_state_returns_none_on_failure(monkeypatch) -> None:
    def _post_handler(endpoint: str, _body: dict[str, Any]) -> Any:
        assert endpoint == "get_blockchain_state"
        return {"success": False}

    monkeypatch.setattr(
        "greenfloor.adapters.coinset_engine.run_engine_json",
        make_coinset_cli_handler(post_handler=_post_handler),
    )
    adapter = CoinsetAdapter()
    assert adapter.get_blockchain_state() is None


def test_build_webhook_callback_url_defaults() -> None:
    url = build_webhook_callback_url("127.0.0.1:8787")
    assert url == "http://127.0.0.1:8787/coinset/tx-block"


def test_build_webhook_callback_url_missing_port() -> None:
    url = build_webhook_callback_url("0.0.0.0")
    assert url == "http://0.0.0.0:8787/coinset/tx-block"


def test_build_webhook_callback_url_custom_path() -> None:
    url = build_webhook_callback_url("localhost:9090", path="/custom")
    assert url == "http://localhost:9090/custom"
