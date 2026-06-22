"""Unittest coverage for ``scripts/greenfloor_scripts`` subprocess adapters."""

from __future__ import annotations

import json
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from greenfloor_scripts.binaries import (
    GreenfloorEngineBinaryError,
    cargo_target_directory,
)
from greenfloor_scripts.coinset_subprocess import (
    coin_records_cli,
    record_from_cli,
    resolve_client_cli,
)
from greenfloor_scripts.engine_subprocess import (
    ENGINE_CLI_FAILED_PREFIX,
    is_retryable_engine_cli_error,
    run_engine_json,
    structured_cli_error_from_detail,
)
from greenfloor_scripts.hex_subprocess import normalize_hex_id
from greenfloor_scripts.kms_subprocess import get_public_key_compressed_hex

ENGINE_CLI_JSON_COINSET_503 = json.dumps(
    {
        "success": False,
        "error": "coinset error: error decoding response body",
        "retryable": True,
    },
    separators=(",", ":"),
)
ENGINE_CLI_JSON_PARSE_BODY = json.dumps(
    {
        "success": False,
        "error": "parse body json: expected value at line 1 column 1",
        "retryable": False,
    },
    separators=(",", ":"),
)
ENGINE_CLI_FAILED_COINSET_503 = f"{ENGINE_CLI_FAILED_PREFIX}{ENGINE_CLI_JSON_COINSET_503}"
ENGINE_CLI_FAILED_PARSE_BODY = f"{ENGINE_CLI_FAILED_PREFIX}{ENGINE_CLI_JSON_PARSE_BODY}"


def subprocess_completed(*, returncode: int, stderr: str) -> SimpleNamespace:
    return SimpleNamespace(returncode=returncode, stdout="", stderr=stderr)


class ScriptAdapterTests(unittest.TestCase):
    def test_resolve_client_cli_returns_normalized_fields(self) -> None:
        with patch("greenfloor_scripts.coinset_subprocess.run_engine_json") as mock_run:
            mock_run.return_value = {
                "network": "testnet11",
                "base_url": "https://testnet11.api.coinset.org",
            }
            network, base_url = resolve_client_cli("testnet", None)
        self.assertEqual(network, "testnet11")
        self.assertEqual(base_url, "https://testnet11.api.coinset.org")
        argv = mock_run.call_args.args[0]
        self.assertEqual(argv[:2], ["coinset", "resolve-client"])
        self.assertIn("--network", argv)
        self.assertIn("testnet", argv)
        self.assertNotIn("--base-url", argv)

    def test_coin_records_cli_passes_height_flags_and_returns_cli_records(self) -> None:
        with patch("greenfloor_scripts.coinset_subprocess.run_engine_json") as mock_run:
            mock_run.return_value = {"coin_records": [{"coin": {"amount": 1}}]}
            records = coin_records_cli(
                "mainnet",
                "https://coinset.test",
                "get_coin_records_by_puzzle_hash",
                {"puzzle_hash": "0x01", "include_spent_coins": False},
                start_height=10,
                end_height=20,
            )
        self.assertEqual(records, [{"coin": {"amount": 1}}])
        argv = mock_run.call_args.args[0]
        self.assertEqual(argv[:2], ["coinset", "coin-records"])
        self.assertIn("--start-height", argv)
        self.assertIn("10", argv)
        self.assertIn("--end-height", argv)
        self.assertIn("20", argv)
        body_json = argv[argv.index("--body-json") + 1]
        self.assertNotIn("start_height", body_json)
        self.assertNotIn("end_height", body_json)

    def test_coin_records_cli_filters_non_object_records(self) -> None:
        with patch("greenfloor_scripts.coinset_subprocess.run_engine_json") as mock_run:
            mock_run.return_value = {"coin_records": [{"coin": {"amount": 1}}, "bad", None]}
            records = coin_records_cli(
                "mainnet",
                None,
                "get_coin_records_by_puzzle_hash",
                {"puzzle_hash": "0x01", "include_spent_coins": False},
            )
        self.assertEqual(records, [{"coin": {"amount": 1}}])

    def test_coin_records_cli_does_not_mutate_input_body(self) -> None:
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
        self.assertEqual(body, {"puzzle_hash": "0x01", "include_spent_coins": False})

    def test_record_from_cli_returns_parsed_record(self) -> None:
        with patch("greenfloor_scripts.coinset_subprocess.run_engine_json") as mock_run:
            mock_run.return_value = {"record": {"peak_height": 1234}}
            record = record_from_cli(
                "mainnet",
                None,
                "get_blockchain_state",
                {},
                "blockchain_state",
            )
        self.assertEqual(record, {"peak_height": 1234})
        argv = mock_run.call_args.args[0]
        self.assertEqual(argv[:2], ["coinset", "record"])
        self.assertIn("--key", argv)
        self.assertIn("blockchain_state", argv)

    def test_structured_cli_error_from_detail_reads_retryable_flag(self) -> None:
        error_text, retryable = structured_cli_error_from_detail(ENGINE_CLI_JSON_COINSET_503)
        self.assertTrue(retryable)
        self.assertIn("error decoding response body", error_text)

    def test_is_retryable_engine_cli_error_uses_structured_json_retryable_flag(self) -> None:
        self.assertTrue(is_retryable_engine_cli_error(RuntimeError(ENGINE_CLI_FAILED_COINSET_503)))
        self.assertFalse(is_retryable_engine_cli_error(RuntimeError(ENGINE_CLI_FAILED_PARSE_BODY)))
        self.assertFalse(is_retryable_engine_cli_error(RuntimeError("invalid puzzle hash")))

    def test_is_retryable_engine_cli_error_requires_json_retryable_flag(self) -> None:
        self.assertFalse(
            is_retryable_engine_cli_error(
                RuntimeError(f"{ENGINE_CLI_FAILED_PREFIX}error: coinset error: operation timed out")
            )
        )

    def test_run_engine_json_wraps_json_stderr_as_engine_cli_failed(self) -> None:
        with (
            patch(
                "greenfloor_scripts.engine_subprocess.resolve_greenfloor_engine_binary",
                return_value="/bin/fake-greenfloor-engine",
            ),
            patch("greenfloor_scripts.engine_subprocess.subprocess.run") as mock_run,
        ):
            mock_run.return_value = subprocess_completed(
                returncode=1,
                stderr=ENGINE_CLI_JSON_COINSET_503,
            )
            with self.assertRaises(RuntimeError) as ctx:
                run_engine_json(["coinset", "post"])
            self.assertEqual(str(ctx.exception), ENGINE_CLI_FAILED_COINSET_503)

    def test_normalize_hex_id_delegates_to_engine_hex_cli(self) -> None:
        valid_id = "b" * 64
        with patch("greenfloor_scripts.hex_subprocess.run_engine_json") as mock_run:
            mock_run.return_value = {"normalized": [valid_id]}
            self.assertEqual(normalize_hex_id(f"0x{valid_id}"), valid_id)
        mock_run.assert_called_once()
        argv = mock_run.call_args.args[0]
        self.assertEqual(argv[:2], ["hex", "normalize-batch"])

    def test_cargo_target_directory_reads_metadata(self) -> None:
        cargo_target_directory.cache_clear()
        with patch("greenfloor_scripts.binaries.subprocess.run") as mock_run:
            mock_run.return_value = SimpleNamespace(
                stdout='{"target_directory":"/repo/greenfloor-engine/target"}',
                stderr="",
                returncode=0,
            )
            self.assertEqual(
                cargo_target_directory(),
                Path("/repo/greenfloor-engine/target"),
            )
        mock_run.assert_called_once()

    def test_cargo_target_directory_requires_manifest(self) -> None:
        cargo_target_directory.cache_clear()
        with patch("greenfloor_scripts.binaries._engine_manifest") as mock_manifest:
            mock_manifest.return_value = Path("/missing/greenfloor-engine/Cargo.toml")
            with self.assertRaises(GreenfloorEngineBinaryError):
                cargo_target_directory()

    def test_cargo_target_directory_requires_target_directory_field(self) -> None:
        cargo_target_directory.cache_clear()
        with patch("greenfloor_scripts.binaries.subprocess.run") as mock_run:
            mock_run.return_value = SimpleNamespace(
                stdout='{"packages":[]}',
                stderr="",
                returncode=0,
            )
            with self.assertRaises(GreenfloorEngineBinaryError):
                cargo_target_directory()

    def test_resolve_greenfloor_engine_binary_honors_env_override(self) -> None:
        from greenfloor_scripts.binaries import resolve_greenfloor_engine_binary

        with patch.dict("os.environ", {"GREENFLOOR_ENGINE_BIN": __file__}, clear=False):
            self.assertEqual(
                resolve_greenfloor_engine_binary(build_if_missing=False), Path(__file__)
            )

    def test_resolve_greenfloor_engine_binary_rejects_missing_override(self) -> None:
        from greenfloor_scripts.binaries import resolve_greenfloor_engine_binary

        with patch.dict(
            "os.environ",
            {"GREENFLOOR_ENGINE_BIN": "/tmp/does-not-exist-greenfloor-engine"},
            clear=False,
        ):
            with self.assertRaises(GreenfloorEngineBinaryError):
                resolve_greenfloor_engine_binary(build_if_missing=False)

    def test_kms_subprocess_reads_public_key_field(self) -> None:
        with patch("greenfloor_scripts.kms_subprocess.run_engine_json") as mock_run:
            mock_run.return_value = {"public_key_compressed_hex": "03abc"}
            self.assertEqual(get_public_key_compressed_hex("key-1", "us-east-1"), "03abc")
        argv = mock_run.call_args.args[0]
        self.assertEqual(argv[0], "kms-public-key-compressed-hex")


if __name__ == "__main__":
    unittest.main()
