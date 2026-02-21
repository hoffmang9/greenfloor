import os
import sys
from pathlib import Path

from greenfloor.cli.chia_keys_passthrough import execute_payload


def _payload(keyring_yaml_path: str) -> dict:
    return {
        "selected_source": "chia_keys",
        "key_id": "fingerprint:123456789",
        "network": "testnet11",
        "market_id": "m1",
        "asset_id": "xch",
        "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        "plan": {
            "op_type": "split",
            "size_base_units": 10,
            "op_count": 2,
            "reason": "r",
        },
        "signer_selection": {
            "selected_source": "chia_keys",
            "key_id": "fingerprint:123456789",
            "network": "testnet11",
            "keyring_yaml_path": keyring_yaml_path,
            "chia_keys_dir": "/tmp/.chia_keys",
        },
    }


def test_passthrough_skips_without_worker_and_returns_contract(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    old = os.getenv("GREENFLOOR_CHIA_KEYS_PASSTHROUGH_WORKER_CMD")
    try:
        os.environ.pop("GREENFLOOR_CHIA_KEYS_PASSTHROUGH_WORKER_CMD", None)
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "skipped"
        assert out["reason"].startswith("wallet_sdk_import_error:") or out["reason"] in {
            "sdk_submit_not_configured",
            "worker_failed:sdk_submit_not_configured",
            "no_unspent_xch_coins",
            "worker_failed:no_unspent_xch_coins",
        }
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_PASSTHROUGH_WORKER_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_PASSTHROUGH_WORKER_CMD"] = old


def test_passthrough_delegates_to_worker(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    script = tmp_path / "worker.py"
    script.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","operation_id":"w1","spend_bundle_hex":"00ff"}))\n'
        ),
        encoding="utf-8",
    )
    old = os.getenv("GREENFLOOR_CHIA_KEYS_PASSTHROUGH_WORKER_CMD")
    try:
        os.environ["GREENFLOOR_CHIA_KEYS_PASSTHROUGH_WORKER_CMD"] = f"{sys.executable} {script}"
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "executed"
        assert out["operation_id"] == "w1"
        assert out["spend_bundle_hex"] == "00ff"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_PASSTHROUGH_WORKER_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_PASSTHROUGH_WORKER_CMD"] = old
