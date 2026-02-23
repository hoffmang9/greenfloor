from __future__ import annotations

import json

from greenfloor.adapters.coinset import CoinsetAdapter


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
