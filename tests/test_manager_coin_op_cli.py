"""Native greenfloor-manager coin-op CLI validation tests (no wallet mocks)."""

from __future__ import annotations

import json
from pathlib import Path

from tests.helpers.manager_cli import parse_json_output, run_manager
from tests.helpers.manager_program_fixtures import (
    CAT_ASSET_HEX,
    write_manager_program_with_signer,
    write_markets_cat_for_coin_ops,
    write_markets_with_ladder,
)


def _coin_op_env(coins: list[dict[str, int | str]]) -> dict[str, str]:
    return {
        "GREENFLOOR_TEST_WALLET_COINS_JSON": json.dumps(coins),
        "GREENFLOOR_TEST_MIXED_SPLIT_OPERATION_ID": "sr-test-op-1",
    }


def test_coin_split_until_ready_requires_size_base_units(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets_with_ladder(markets)

    code, _stdout, stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "coin-split",
            "--market-id",
            "m1",
            "--until-ready",
            "--network",
            "mainnet",
        ]
    )
    assert code != 0
    assert "until-ready mode requires --size-base-units" in stderr


def test_coin_split_until_ready_disallows_no_wait(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets_with_ladder(markets)

    code, _stdout, stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "coin-split",
            "--market-id",
            "m1",
            "--until-ready",
            "--size-base-units",
            "10",
            "--no-wait",
            "--network",
            "mainnet",
        ]
    )
    assert code != 0
    assert "until-ready mode requires wait mode" in stderr


def test_coin_combine_until_ready_requires_size_base_units(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets_with_ladder(markets)

    code, _stdout, stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "coin-combine",
            "--market-id",
            "m1",
            "--until-ready",
            "--network",
            "mainnet",
        ]
    )
    assert code != 0
    assert "until-ready mode requires --size-base-units" in stderr


def test_coin_split_auto_selects_largest_spendable_coin(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets_cat_for_coin_ops(markets)
    coins = [
        {"id": "Coin_small", "amount": 50_000},
        {"id": "Coin_big", "amount": 150_000},
    ]

    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "coin-split",
            "--market-id",
            "m1",
            "--amount-per-coin",
            "50",
            "--number-of-coins",
            "2",
            "--no-wait",
            "--network",
            "mainnet",
        ],
        env=_coin_op_env(coins),
    )
    assert code == 0
    payload = parse_json_output(stdout)
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["resolved_asset_id"] == CAT_ASSET_HEX
    assert payload["operations"][0]["selected_coin_ids"] == ["Coin_big"]


def test_coin_split_guardrail_blocks_when_all_spendable_selected(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets_cat_for_coin_ops(markets)
    coins = [
        {"id": "Coin_a", "amount": 150_000},
        {"id": "Coin_b", "amount": 150_000},
    ]

    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "coin-split",
            "--market-id",
            "m1",
            "--coin-id",
            "Coin_a",
            "--coin-id",
            "Coin_b",
            "--amount-per-coin",
            "50",
            "--number-of-coins",
            "2",
            "--no-wait",
            "--network",
            "mainnet",
        ],
        env=_coin_op_env(coins),
    )
    assert code == 2
    payload = parse_json_output(stdout)
    assert payload["error"] == "coin_split_lockup_guardrail_would_lock_all_spendable_coins"


def test_coin_split_until_ready_succeeds_when_gate_ready(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets_cat_for_coin_ops(markets)
    # size 10 -> 10_000 mojos per denom; ladder requires 3 denom coins + reserve
    coins = [
        {"id": "Coin_a", "amount": 10_000},
        {"id": "Coin_b", "amount": 10_000},
        {"id": "Coin_c", "amount": 10_000},
        {"id": "Coin_reserve", "amount": 20_000},
    ]

    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "coin-split",
            "--market-id",
            "m1",
            "--until-ready",
            "--size-base-units",
            "10",
            "--network",
            "mainnet",
        ],
        env={"GREENFLOOR_TEST_WALLET_COINS_JSON": json.dumps(coins)},
    )
    assert code == 0
    payload = parse_json_output(stdout)
    assert payload["stop_reason"] == "ready"
    assert payload["operations"] == []


def test_coin_split_uses_config_split_fee_mojos(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets_cat_for_coin_ops(markets)
    text = program.read_text(encoding="utf-8")
    if "split_fee_mojos:" not in text:
        text = text.replace("coin_ops:", "coin_ops:\n  split_fee_mojos: 42\n", 1)
    else:
        text = text.replace("split_fee_mojos: 0", "split_fee_mojos: 42")
    program.write_text(text, encoding="utf-8")
    coins = [{"id": "Coin_big", "amount": 150_000}, {"id": "Coin_reserve", "amount": 10_000}]

    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "coin-split",
            "--market-id",
            "m1",
            "--amount-per-coin",
            "50",
            "--number-of-coins",
            "2",
            "--no-wait",
            "--network",
            "mainnet",
        ],
        env=_coin_op_env(coins),
    )
    assert code == 0
    payload = parse_json_output(stdout)
    assert payload["operations"]
