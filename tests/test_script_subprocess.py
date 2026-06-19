"""Pytest coverage for ``scripts/greenfloor_scripts`` subprocess adapters."""

from __future__ import annotations

from unittest.mock import patch

from greenfloor_scripts.coinset_scanner import coinset_with_retries, is_retryable_coinset_error
from greenfloor_scripts.coinset_subprocess import (
    coin_records_cli,
    record_from_cli,
    resolve_client_cli,
)
from greenfloor_scripts.hex_subprocess import normalize_hex_id
from greenfloor_scripts.kms_subprocess import get_public_key_compressed_hex


def test_resolve_client_cli_returns_normalized_fields() -> None:
    with patch("greenfloor_scripts.coinset_subprocess.run_engine_json") as mock_run:
        mock_run.return_value = {
            "network": "testnet11",
            "base_url": "https://testnet11.api.coinset.org",
        }
        network, base_url = resolve_client_cli("testnet", None)
    assert network == "testnet11"
    assert base_url == "https://testnet11.api.coinset.org"
    argv = mock_run.call_args.args[0]
    assert argv[:2] == ["coinset", "resolve-client"]
    assert "--network" in argv and "testnet" in argv
    assert "--base-url" not in argv


def test_coin_records_cli_passes_height_flags_and_returns_cli_records() -> None:
    with patch("greenfloor_scripts.coinset_subprocess.run_engine_json") as mock_run:
        mock_run.return_value = {
            "coin_records": [{"coin": {"amount": 1}}],
        }
        records = coin_records_cli(
            "mainnet",
            "https://coinset.test",
            "get_coin_records_by_puzzle_hash",
            {"puzzle_hash": "0x01", "include_spent_coins": False},
            start_height=10,
            end_height=20,
        )
    assert records == [{"coin": {"amount": 1}}]
    argv = mock_run.call_args.args[0]
    assert argv[:2] == ["coinset", "coin-records"]
    assert "--start-height" in argv and "10" in argv
    assert "--end-height" in argv and "20" in argv
    body_json = argv[argv.index("--body-json") + 1]
    assert "start_height" not in body_json
    assert "end_height" not in body_json


def test_coin_records_cli_does_not_mutate_input_body() -> None:
    body = {"puzzle_hash": "0x01", "include_spent_coins": False}
    with patch("greenfloor_scripts.coinset_subprocess.run_engine_json") as mock_run:
        mock_run.return_value = {"coin_records": []}
        coin_records_cli(
            "mainnet",
            None,
            "get_coin_records_by_puzzle_hash",
            body,
            start_height=10,
            end_height=20,
        )
    assert body == {"puzzle_hash": "0x01", "include_spent_coins": False}


def test_record_from_cli_returns_parsed_record() -> None:
    with patch("greenfloor_scripts.coinset_subprocess.run_engine_json") as mock_run:
        mock_run.return_value = {"record": {"peak_height": 1234}}
        record = record_from_cli(
            "mainnet",
            None,
            "get_blockchain_state",
            {},
            "blockchain_state",
        )
    assert record == {"peak_height": 1234}
    argv = mock_run.call_args.args[0]
    assert argv[:2] == ["coinset", "record"]
    assert "--key" in argv and "blockchain_state" in argv


def test_is_retryable_coinset_error_classifies_transient_failures() -> None:
    assert is_retryable_coinset_error(RuntimeError("coinset_http_error:503"))
    assert not is_retryable_coinset_error(RuntimeError("invalid puzzle hash"))


def test_coinset_with_retries_succeeds_after_retryable_failure() -> None:
    calls = {"count": 0}

    def flaky() -> str:
        calls["count"] += 1
        if calls["count"] == 1:
            raise RuntimeError("timed out")
        return "ok"

    with patch("greenfloor_scripts.coinset_scanner.time.sleep") as mock_sleep:
        assert coinset_with_retries(flaky, sleep=mock_sleep) == "ok"
    assert calls["count"] == 2
    mock_sleep.assert_called_once()


def test_coinset_with_retries_raises_immediately_on_non_retryable_error() -> None:
    calls = {"count": 0}

    def fail_fast() -> None:
        calls["count"] += 1
        raise RuntimeError("invalid request")

    with patch("greenfloor_scripts.coinset_scanner.time.sleep") as mock_sleep:
        try:
            coinset_with_retries(fail_fast, sleep=mock_sleep)
        except RuntimeError as exc:
            assert str(exc) == "invalid request"
        else:
            raise AssertionError("expected non-retryable error")
    assert calls["count"] == 1
    mock_sleep.assert_not_called()


def test_normalize_hex_id_delegates_to_engine_hex_cli() -> None:
    valid_id = "b" * 64
    with patch("greenfloor_scripts.hex_subprocess.run_engine_json") as mock_run:
        mock_run.return_value = {"normalized": [valid_id]}
        assert normalize_hex_id(f"0x{valid_id}") == valid_id
    mock_run.assert_called_once()
    argv = mock_run.call_args.args[0]
    assert argv[:2] == ["hex", "normalize-batch"]


def test_kms_subprocess_reads_public_key_field() -> None:
    with patch("greenfloor_scripts.kms_subprocess.run_engine_json") as mock_run:
        mock_run.return_value = {"public_key_compressed_hex": "03abc"}
        assert get_public_key_compressed_hex("key-1", "us-east-1") == "03abc"
    argv = mock_run.call_args.args[0]
    assert argv[0] == "kms-public-key-compressed-hex"
