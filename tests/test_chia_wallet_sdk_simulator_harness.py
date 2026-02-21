from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

import pytest

_SDK_ROOT = Path(__file__).resolve().parents[1] / "chia-wallet-sdk"
_SDK_MANIFEST = _SDK_ROOT / "Cargo.toml"


def _require_default_sim_harness_ready() -> None:
    if not _SDK_MANIFEST.exists():
        pytest.skip("chia-wallet-sdk submodule missing")
    if shutil.which("cargo") is None:
        pytest.skip("cargo not available")


def _require_full_sim_harness_enabled() -> None:
    _require_default_sim_harness_ready()
    if os.getenv("GREENFLOOR_RUN_SDK_SIM_TESTS_FULL", "").strip() != "1":
        pytest.skip(
            "set GREENFLOOR_RUN_SDK_SIM_TESTS_FULL=1 to run extended chia-wallet-sdk simulator harness"
        )


def _run_sdk_cmd(args: list[str], timeout_s: int = 600) -> None:
    completed = subprocess.run(
        args,
        cwd=_SDK_ROOT,
        capture_output=True,
        text=True,
        timeout=timeout_s,
        check=False,
        env={**os.environ, "CARGO_TERM_COLOR": "never"},
    )
    if completed.returncode != 0:
        raise AssertionError(
            "chia-wallet-sdk simulator harness command failed:\n"
            f"cmd={' '.join(args)}\n"
            f"exit={completed.returncode}\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}\n"
        )


def _run_driver_test(test_name: str, *, action_layer: bool = False) -> None:
    args = [
        "cargo",
        "test",
        "-p",
        "chia-sdk-driver",
    ]
    if action_layer:
        args.extend(["--features", "action-layer"])
    args.extend(
        [
            test_name,
            "--manifest-path",
            str(_SDK_MANIFEST),
            "--",
            "--exact",
        ]
    )
    _run_sdk_cmd(args, timeout_s=240)


def test_sdk_simulator_cat_issue_smoke() -> None:
    _require_default_sim_harness_ready()
    _run_driver_test("actions::issue_cat::tests::test_action_single_issuance_cat::case_1_normal")


def test_sdk_simulator_cat_send_with_change_smoke() -> None:
    _require_default_sim_harness_ready()
    _run_driver_test("actions::send::tests::test_action_send_cat_with_change::case_1_normal")


def test_sdk_simulator_cat_spend_primitive_smoke() -> None:
    _require_default_sim_harness_ready()
    _run_driver_test("primitives::cat::tests::test_cat_spends::case_1")


def test_sdk_simulator_cat_offer_catalog_smoke() -> None:
    _require_default_sim_harness_ready()
    # Default harness smoke test: CAT-backed offer make/take path in simulator.
    _run_driver_test(
        "primitives::action_layer::launch_drivers::tests::test_catalog",
        action_layer=True,
    )


def test_sdk_simulator_key_creation_and_spend_example() -> None:
    _require_full_sim_harness_enabled()
    # Optional extended check: explicit spend_simulator example run.
    _run_sdk_cmd(
        [
            "cargo",
            "run",
            "--example",
            "spend_simulator",
            "--manifest-path",
            str(_SDK_MANIFEST),
        ],
        timeout_s=240,
    )


def test_sdk_simulator_cat_offer_catalog_extended() -> None:
    _require_full_sim_harness_enabled()
    # Optional extended check: explicit CAT offer lifecycle regression test.
    _run_driver_test(
        "primitives::action_layer::launch_drivers::tests::test_managed_reward_distributor",
        action_layer=True,
    )
