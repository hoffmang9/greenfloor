import os
import sys
from pathlib import Path

from greenfloor.cli.chia_keys_bundle_signer import execute_payload


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


def test_bundle_signer_returns_request_when_raw_missing(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    old = os.getenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD")
    try:
        os.environ.pop("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD", None)
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "skipped"
        assert out["reason"].startswith("sdk_submit_")
        assert (
            out.get("sign_job", {}).get("plan", {}).get("target_total_base_units") == 20
            or out.get("engine_request", {}).get("plan", {}).get("target_total_base_units") == 20
        )
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD"] = old


def test_bundle_signer_delegates_to_raw(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    script = tmp_path / "raw.py"
    script.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","spend_bundle_hex":"00ff"}))\n'
        ),
        encoding="utf-8",
    )
    old = os.getenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD")
    try:
        os.environ["GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD"] = f"{sys.executable} {script}"
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "executed"
        assert out["spend_bundle_hex"] == "00ff"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD"] = old
