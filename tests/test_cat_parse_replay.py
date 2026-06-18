from __future__ import annotations

import json
import os
from pathlib import Path

import pytest


def _hex_to_bytes(value: str) -> bytes:
    raw = value.strip().lower()
    if raw.startswith("0x"):
        raw = raw[2:]
    if len(raw) % 2:
        raw = f"0{raw}"
    return bytes.fromhex(raw)


def _load_cases() -> list[Path]:
    raw = os.getenv("GREENFLOOR_CAT_PARSE_REPLAY_CASES_DIR", "").strip()
    if not raw:
        return []
    root = Path(raw)
    if not root.exists() or not root.is_dir():
        return []
    return sorted(p for p in root.glob("*.json") if p.is_file())


def test_replay_captured_cat_parse_cases() -> None:
    case_paths = _load_cases()
    if not case_paths:
        pytest.skip("set GREENFLOOR_CAT_PARSE_REPLAY_CASES_DIR to replay captured CAT parse cases")

    try:
        import chia_wallet_sdk as sdk  # type: ignore
    except Exception:
        pytest.skip("chia-wallet-sdk not installed")

    clvm = sdk.Clvm()
    failures: list[str] = []
    for case_path in case_paths:
        case = json.loads(case_path.read_text(encoding="utf-8"))
        parent_coin = sdk.Coin(
            _hex_to_bytes(str(case["parent_coin_parent_coin_id"])),
            _hex_to_bytes(str(case["parent_coin_puzzle_hash"])),
            int(case["parent_coin_amount"]),
        )
        child_coin = sdk.Coin(
            _hex_to_bytes(str(case["coin_parent_coin_id"])),
            _hex_to_bytes(str(case["coin_puzzle_hash"])),
            int(case["coin_amount"]),
        )
        puzzle_reveal = _hex_to_bytes(str(case["puzzle_reveal"]))
        solution = _hex_to_bytes(str(case["solution"]))
        parent_puzzle_program = clvm.deserialize(puzzle_reveal)
        parent_solution = clvm.deserialize(solution)
        parent_puzzle = parent_puzzle_program.puzzle()

        parse_mode = "non_empty"
        parse_error = ""
        try:
            parsed_children = parent_puzzle.parse_child_cats(parent_coin, parent_solution)
        except Exception as exc:
            parsed_children = None
            parse_mode = "exception"
            parse_error = f"{type(exc).__name__}:{exc}"
        parsed_children_count = len(parsed_children) if parsed_children else 0
        if parse_mode == "non_empty" and parsed_children_count == 0:
            parse_mode = "empty"

        # Keep the same invariant check from signing diagnostics: parent spend should create child.
        output = parent_puzzle_program.run(parent_solution, 11_000_000_000, False)
        conditions = output.value.to_list() or []
        creates_child = False
        for condition in conditions:
            try:
                create_coin = condition.parse_create_coin()
            except Exception:
                create_coin = None
            if create_coin is None:
                continue
            created_coin = sdk.Coin(
                parent_coin.coin_id(),
                bytes(create_coin.puzzle_hash),
                int(create_coin.amount),
            )
            if sdk.to_hex(created_coin.coin_id()) == sdk.to_hex(child_coin.coin_id()):
                creates_child = True
                break

        if not creates_child:
            failures.append(f"{case_path.name}: parent spend does not recreate target child coin")
            continue

        if parse_mode != "non_empty":
            failures.append(
                f"{case_path.name}: parse_mode={parse_mode} parsed_children_count={parsed_children_count} parse_error={parse_error}"
            )

    assert not failures, "captured CAT parse replay failures:\n" + "\n".join(failures)
