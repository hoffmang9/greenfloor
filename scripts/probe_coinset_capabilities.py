#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib
import json
from pathlib import Path
from typing import Any

from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.config.launcher import launcher_id_from_program_config
from greenfloor.hex_utils import normalize_hex_id


def _import_sdk() -> Any:
    return importlib.import_module("chia_wallet_sdk")


def _hex_to_bytes(value: str) -> bytes:
    raw = value.strip().lower()
    if raw.startswith("0x"):
        raw = raw[2:]
    if len(raw) % 2:
        raw = f"0{raw}"
    return bytes.fromhex(raw)


def _to_coinset_hex(value: bytes) -> str:
    return f"0x{value.hex()}"


def _safe_int(value: object, default: int = 0) -> int:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return default


def _read_launcher_id_file(path: str) -> str:
    if not str(path).strip():
        return ""
    file_path = Path(path).expanduser()
    if not file_path.exists():
        return ""
    return normalize_hex_id(file_path.read_text(encoding="utf-8").strip()) or ""


def _coin_id_from_record(record: dict[str, Any]) -> str:
    coin = record.get("coin")
    if not isinstance(coin, dict):
        return ""
    parent = normalize_hex_id(coin.get("parent_coin_info"))
    puzzle = normalize_hex_id(coin.get("puzzle_hash"))
    amount = _safe_int(coin.get("amount"), default=-1)
    if not parent or not puzzle or amount < 0:
        return ""
    sdk = _import_sdk()
    coin_obj = sdk.Coin(_hex_to_bytes(parent), _hex_to_bytes(puzzle), amount)
    return normalize_hex_id(sdk.to_hex(coin_obj.coin_id())) or ""


def _supports_call(call: Any) -> tuple[bool, str | None, int | None]:
    try:
        rows = call()
    except Exception as exc:  # noqa: BLE001
        return False, str(exc), None
    if isinstance(rows, list):
        return True, None, len(rows)
    if rows is None:
        return True, None, None
    return True, None, None


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Probe Coinset endpoint and height-window capabilities for vault scans."
    )
    parser.add_argument("--network", default="mainnet", choices=["mainnet", "testnet11", "testnet"])
    parser.add_argument("--coinset-base-url", default="")
    parser.add_argument("--launcher-id", default="")
    parser.add_argument("--launcher-id-file", default="")
    parser.add_argument(
        "--program-config",
        default="",
        help="Path to program.yaml used to resolve vault.launcher_id when --launcher-id is omitted.",
    )
    parser.add_argument("--nonce", type=int, default=0, help="Member nonce to probe (default 0).")
    parser.add_argument(
        "--height-window",
        type=int,
        default=50000,
        help="Probe range window in blocks from chain peak (default 50000).",
    )
    args = parser.parse_args()

    launcher_id = normalize_hex_id(args.launcher_id)
    launcher_source = "arg"
    if not launcher_id and str(args.launcher_id_file).strip():
        launcher_id = _read_launcher_id_file(args.launcher_id_file)
        if launcher_id:
            launcher_source = "file"
    if not launcher_id:
        program_config = str(args.program_config).strip()
        if not program_config:
            raise ValueError("launcher-id, launcher-id-file, or --program-config is required")
        launcher_id = launcher_id_from_program_config(program_config)
        launcher_source = "program_config"

    adapter = CoinsetAdapter(
        base_url=(str(args.coinset_base_url).strip() or None),
        network=args.network,
    )

    sdk = _import_sdk()
    cfg = sdk.MemberConfig().with_top_level(True).with_nonce(int(args.nonce))
    p2_hash = normalize_hex_id(
        sdk.to_hex(sdk.singleton_member_hash(cfg, _hex_to_bytes(launcher_id), False))
    )
    if not p2_hash:
        raise RuntimeError("failed_to_derive_p2_hash")
    p2_coinset_hex = _to_coinset_hex(_hex_to_bytes(p2_hash))

    chain_state = adapter.get_blockchain_state() or {}
    peak_height = -1
    if isinstance(chain_state, dict):
        peak = chain_state.get("peak")
        if isinstance(peak, dict):
            peak_height = _safe_int(peak.get("height"), default=-1)
        if peak_height < 0:
            peak_height = _safe_int(chain_state.get("peak_height"), default=-1)
    if peak_height < 0:
        peak_height = 0
    start_height = max(0, peak_height - max(1, int(args.height_window)))
    end_height = peak_height

    puzzle_all_ok, puzzle_all_err, puzzle_all_count = _supports_call(
        lambda: adapter.get_coin_records_by_puzzle_hashes(
            puzzle_hashes_hex=[p2_coinset_hex], include_spent_coins=True
        )
    )
    puzzle_range_ok, puzzle_range_err, puzzle_range_count = _supports_call(
        lambda: adapter.get_coin_records_by_puzzle_hashes(
            puzzle_hashes_hex=[p2_coinset_hex],
            include_spent_coins=True,
            start_height=start_height,
            end_height=end_height,
        )
    )
    hints_all_ok, hints_all_err, hints_all_count = _supports_call(
        lambda: adapter.get_coin_records_by_hints(
            hints_hex=[p2_coinset_hex], include_spent_coins=True
        )
    )
    hints_range_ok, hints_range_err, hints_range_count = _supports_call(
        lambda: adapter.get_coin_records_by_hints(
            hints_hex=[p2_coinset_hex],
            include_spent_coins=True,
            start_height=start_height,
            end_height=end_height,
        )
    )

    sample_name = ""
    if puzzle_all_ok:
        records = adapter.get_coin_records_by_puzzle_hashes(
            puzzle_hashes_hex=[p2_coinset_hex], include_spent_coins=True
        )
        for row in records:
            if not isinstance(row, dict):
                continue
            sample_name = _coin_id_from_record(row)
            if sample_name:
                break

    by_name_all_ok = None
    by_name_range_ok = None
    by_name_all_err = None
    by_name_range_err = None
    by_name_all_count = None
    by_name_range_count = None
    if sample_name:
        by_name_all_ok, by_name_all_err, by_name_all_count = _supports_call(
            lambda: adapter.get_coin_records_by_names(
                coin_names_hex=[_to_coinset_hex(_hex_to_bytes(sample_name))],
                include_spent_coins=True,
            )
        )
        by_name_range_ok, by_name_range_err, by_name_range_count = _supports_call(
            lambda: adapter.get_coin_records_by_names(
                coin_names_hex=[_to_coinset_hex(_hex_to_bytes(sample_name))],
                include_spent_coins=True,
                start_height=start_height,
                end_height=end_height,
            )
        )

    print(
        json.dumps(
            {
                "network": adapter.network,
                "coinset_base_url": adapter.base_url,
                "launcher_id": launcher_id,
                "launcher_id_source": launcher_source,
                "probe_nonce": int(args.nonce),
                "probe_p2_hash": p2_hash,
                "scan_window": {
                    "start_height": start_height,
                    "end_height": end_height,
                    "peak_height": peak_height,
                },
                "capabilities": {
                    "get_coin_records_by_puzzle_hashes": {
                        "all_supported": puzzle_all_ok,
                        "all_error": puzzle_all_err,
                        "all_count": puzzle_all_count,
                        "range_supported": puzzle_range_ok,
                        "range_error": puzzle_range_err,
                        "range_count": puzzle_range_count,
                    },
                    "get_coin_records_by_hints": {
                        "all_supported": hints_all_ok,
                        "all_error": hints_all_err,
                        "all_count": hints_all_count,
                        "range_supported": hints_range_ok,
                        "range_error": hints_range_err,
                        "range_count": hints_range_count,
                    },
                    "get_coin_records_by_names": {
                        "sample_name": sample_name or None,
                        "all_supported": by_name_all_ok,
                        "all_error": by_name_all_err,
                        "all_count": by_name_all_count,
                        "range_supported": by_name_range_ok,
                        "range_error": by_name_range_err,
                        "range_count": by_name_range_count,
                    },
                },
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
