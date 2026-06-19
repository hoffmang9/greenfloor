"""Pytest coverage for ``scripts/greenfloor_scripts`` subprocess adapters."""

from __future__ import annotations

from unittest.mock import patch

from greenfloor_scripts.coinset_subprocess import coin_records_cli, post_json_cli
from greenfloor_scripts.hex_subprocess import normalize_hex_id
from greenfloor_scripts.kms_subprocess import get_public_key_compressed_hex


def test_coin_records_cli_applies_height_and_parses_records() -> None:
    with patch("greenfloor_scripts.coinset_subprocess.post_json_cli") as mock_post:
        mock_post.return_value = {
            "success": True,
            "coin_records": [{"coin": {"amount": 1}}, "skip"],
        }
        records = coin_records_cli(
            "mainnet",
            None,
            "get_coin_records_by_puzzle_hash",
            {"puzzle_hash": "0x01", "include_spent_coins": False},
            start_height=10,
            end_height=20,
        )
    assert records == [{"coin": {"amount": 1}}]
    body = mock_post.call_args.args[3]
    assert body["start_height"] == 10
    assert body["end_height"] == 20


def test_coin_records_cli_does_not_mutate_input_body() -> None:
    body = {"puzzle_hash": "0x01", "include_spent_coins": False}
    with patch("greenfloor_scripts.coinset_subprocess.post_json_cli") as mock_post:
        mock_post.return_value = {"success": True, "coin_records": []}
        coin_records_cli(
            "mainnet",
            None,
            "get_coin_records_by_puzzle_hash",
            body,
            start_height=10,
            end_height=20,
        )
    assert body == {"puzzle_hash": "0x01", "include_spent_coins": False}


def test_post_json_cli_builds_coinset_post_argv() -> None:
    with patch("greenfloor_scripts.coinset_subprocess.run_engine_json") as mock_run:
        mock_run.return_value = {"success": True}
        payload = post_json_cli("testnet11", "https://coinset.test", "get_blockchain_state", {})
    assert payload == {"success": True}
    mock_run.assert_called_once()
    argv = mock_run.call_args.args[0]
    assert argv[:2] == ["coinset", "post"]
    assert "--network" in argv and "testnet11" in argv
    assert "--base-url" in argv and "https://coinset.test" in argv
    assert "--endpoint" in argv and "get_blockchain_state" in argv


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
