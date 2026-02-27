from __future__ import annotations

import json

from greenfloor.adapters.coinset import (
    CoinsetAdapter,
    build_webhook_callback_url,
    extract_coinset_tx_ids_from_offer_payload,
)


class _FakeResponse:
    def __init__(self, payload) -> None:
        self._payload = payload

    def read(self) -> bytes:
        return json.dumps(self._payload).encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        return None


def test_coinset_adapter_defaults_to_mainnet() -> None:
    adapter = CoinsetAdapter()
    assert adapter.base_url == CoinsetAdapter.MAINNET_BASE_URL


def test_coinset_adapter_network_testnet11() -> None:
    adapter = CoinsetAdapter(network="testnet11")
    assert adapter.base_url == CoinsetAdapter.TESTNET11_BASE_URL


def test_coinset_adapter_require_testnet11_overrides_network() -> None:
    adapter = CoinsetAdapter(network="mainnet", require_testnet11=True)
    assert adapter.base_url == CoinsetAdapter.TESTNET11_BASE_URL


def test_coinset_adapter_custom_base_url_overrides_network() -> None:
    adapter = CoinsetAdapter("https://coinset.custom", network="testnet11")
    assert adapter.base_url == "https://coinset.custom"


def test_coinset_adapter_get_all_mempool_tx_ids_uses_post_json(monkeypatch) -> None:
    captured = {}

    def _fake_urlopen(req, timeout):
        captured["url"] = req.full_url
        captured["method"] = req.get_method()
        captured["timeout"] = timeout
        captured["content_type"] = req.get_header("Content-type")
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return _FakeResponse({"success": True, "mempool_tx_ids": ["0xabc", "0xdef"]})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter("https://coinset.org")
    tx_ids = adapter.get_all_mempool_tx_ids()
    assert tx_ids == ["0xabc", "0xdef"]
    assert captured["url"] == "https://coinset.org/get_all_mempool_tx_ids"
    assert captured["method"] == "POST"
    assert captured["timeout"] == 15
    assert captured["content_type"] == "application/json"
    assert captured["body"] == {}


def test_coinset_adapter_get_coin_records_by_puzzle_hash_filters_non_dicts(monkeypatch) -> None:
    def _fake_urlopen(_req, timeout=None):
        _ = timeout
        return _FakeResponse({"success": True, "coin_records": [{"coin": {"amount": 1}}, "bad"]})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter()
    records = adapter.get_coin_records_by_puzzle_hash(
        puzzle_hash_hex="0x11", include_spent_coins=False
    )
    assert records == [{"coin": {"amount": 1}}]


def test_coinset_adapter_get_coin_record_by_name_success_and_failure(monkeypatch) -> None:
    responses = [
        {"success": True, "coin_record": {"coin": {"amount": 123}}},
        {"success": False, "error": "not_found"},
    ]

    def _fake_urlopen(_req, timeout=None):
        _ = timeout
        return _FakeResponse(responses.pop(0))

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter()
    assert adapter.get_coin_record_by_name(coin_name_hex="0x22") == {"coin": {"amount": 123}}
    assert adapter.get_coin_record_by_name(coin_name_hex="0x33") is None


def test_coinset_adapter_get_puzzle_and_solution_adds_height_when_provided(monkeypatch) -> None:
    captured_bodies = []

    def _fake_urlopen(req, timeout=None):
        _ = timeout
        captured_bodies.append(json.loads(req.data.decode("utf-8")))
        return _FakeResponse(
            {"success": True, "coin_solution": {"puzzle_reveal": "80", "solution": "80"}}
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter()
    solution = adapter.get_puzzle_and_solution(coin_id_hex="0x44", height=50)
    assert solution == {"puzzle_reveal": "80", "solution": "80"}
    assert captured_bodies[0] == {"coin_id": "0x44", "height": 50}


def test_coinset_adapter_get_puzzle_and_solution_omits_non_positive_height(monkeypatch) -> None:
    captured_bodies = []

    def _fake_urlopen(req, timeout=None):
        _ = timeout
        captured_bodies.append(json.loads(req.data.decode("utf-8")))
        return _FakeResponse(
            {"success": True, "coin_solution": {"puzzle_reveal": "80", "solution": "80"}}
        )

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter()
    _ = adapter.get_puzzle_and_solution(coin_id_hex="0x55", height=0)
    assert captured_bodies[0] == {"coin_id": "0x55"}


def test_coinset_adapter_push_tx_returns_payload_dict(monkeypatch) -> None:
    def _fake_urlopen(_req, timeout=None):
        _ = timeout
        return _FakeResponse({"success": True, "status": "submitted"})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter()
    result = adapter.push_tx(spend_bundle_hex="0xdeadbeef")
    assert result["success"] is True
    assert result["status"] == "submitted"


# ---------------------------------------------------------------------------
# extract_coinset_tx_ids_from_offer_payload
# ---------------------------------------------------------------------------


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


# ---------------------------------------------------------------------------
# get_conservative_fee_estimate
# ---------------------------------------------------------------------------


def test_conservative_fee_estimate_uses_max_of_estimates(monkeypatch) -> None:
    def _fake_urlopen(_req, timeout=None):
        _ = timeout
        return _FakeResponse({"success": True, "estimates": [100, 500, 200]})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter()
    assert adapter.get_conservative_fee_estimate() == 500


def test_conservative_fee_estimate_falls_back_to_fee_estimate_field(monkeypatch) -> None:
    def _fake_urlopen(_req, timeout=None):
        _ = timeout
        return _FakeResponse({"success": True, "fee_estimate": 42})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter()
    assert adapter.get_conservative_fee_estimate() == 42


def test_conservative_fee_estimate_returns_none_on_failure(monkeypatch) -> None:
    def _fake_urlopen(_req, timeout=None):
        _ = timeout
        return _FakeResponse({"success": False})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter()
    assert adapter.get_conservative_fee_estimate() is None


def test_conservative_fee_estimate_skips_invalid_estimate_values(monkeypatch) -> None:
    def _fake_urlopen(_req, timeout=None):
        _ = timeout
        return _FakeResponse({"success": True, "estimates": ["bad", -1, 300]})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter()
    assert adapter.get_conservative_fee_estimate() == 300


# ---------------------------------------------------------------------------
# get_blockchain_state
# ---------------------------------------------------------------------------


def test_get_blockchain_state_success(monkeypatch) -> None:
    def _fake_urlopen(_req, timeout=None):
        _ = timeout
        return _FakeResponse({"success": True, "blockchain_state": {"peak_height": 1234}})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter()
    state = adapter.get_blockchain_state()
    assert state == {"peak_height": 1234}


def test_get_blockchain_state_returns_none_on_failure(monkeypatch) -> None:
    def _fake_urlopen(_req, timeout=None):
        _ = timeout
        return _FakeResponse({"success": False})

    monkeypatch.setattr("urllib.request.urlopen", _fake_urlopen)
    adapter = CoinsetAdapter()
    assert adapter.get_blockchain_state() is None


# ---------------------------------------------------------------------------
# build_webhook_callback_url
# ---------------------------------------------------------------------------


def test_build_webhook_callback_url_defaults() -> None:
    url = build_webhook_callback_url("127.0.0.1:8787")
    assert url == "http://127.0.0.1:8787/coinset/tx-block"


def test_build_webhook_callback_url_missing_port() -> None:
    url = build_webhook_callback_url("0.0.0.0")
    assert url == "http://0.0.0.0:8787/coinset/tx-block"


def test_build_webhook_callback_url_custom_path() -> None:
    url = build_webhook_callback_url("localhost:9090", path="/custom")
    assert url == "http://localhost:9090/custom"
