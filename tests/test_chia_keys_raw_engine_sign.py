import os
import sys
from pathlib import Path

from greenfloor.cli.chia_keys_raw_engine_sign import execute_payload


def _payload(keyring_yaml_path: str) -> dict:
    return {
        "key_id": "fingerprint:123456789",
        "network": "testnet11",
        "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        "keyring_yaml_path": keyring_yaml_path,
        "asset_id": "xch",
        "plan": {
            "op_type": "split",
            "size_base_units": 10,
            "op_count": 2,
            "target_total_base_units": 20,
            "reason": "r",
        },
        "selected_coins": [
            {"coin_id": "03", "parent_coin_info": "01", "puzzle_hash": "02", "amount": 50}
        ],
    }


def test_raw_engine_sign_returns_sign_job_when_impl_missing(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    old = os.getenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD")
    try:
        os.environ.pop("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD", None)
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "skipped"
        assert out["reason"].startswith("sdk_submit_")
        assert out["sign_job"]["plan"]["target_total_base_units"] == 20
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD"] = old


def test_raw_engine_sign_delegates_to_impl(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    script = tmp_path / "impl.py"
    script.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","spend_bundle_hex":"00ff"}))\n'
        ),
        encoding="utf-8",
    )
    old = os.getenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD")
    try:
        os.environ["GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD"] = f"{sys.executable} {script}"
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "executed"
        assert out["spend_bundle_hex"] == "00ff"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD"] = old
