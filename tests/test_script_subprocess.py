"""Pytest coverage for ``scripts/greenfloor_scripts`` subprocess adapters."""

from __future__ import annotations

from unittest.mock import patch

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


def test_coin_records_cli_applies_height_and_parses_records() -> None:
    with patch("greenfloor_scripts.coinset_subprocess.run_engine_json") as mock_run:
        mock_run.return_value = {
            "coin_records": [{"coin": {"amount": 1}}, "skip"],
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
    body_json = argv[argv.index("--body-json") + 1]
    assert '"start_height":10' in body_json
    assert '"end_height":20' in body_json


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
