import os
import sys
from pathlib import Path

from greenfloor.cli.chia_keys_worker import execute_payload


def _payload(keyring_yaml_path: str) -> dict:
    return {
        "selected_source": "chia_keys",
        "key_id": "fingerprint:123456789",
        "network": "testnet11",
        "asset_id": "xch",
        "market_id": "m1",
        "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        "keyring_yaml_path": keyring_yaml_path,
        "chia_keys_dir": "/tmp/.chia_keys",
        "plan": {
            "op_type": "split",
            "size_base_units": 10,
            "op_count": 2,
            "reason": "r",
            "target_total_base_units": 20,
        },
    }


def test_worker_uses_default_signer_module_when_signer_env_missing(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    old = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_CMD")
    old_backend = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD")
    try:
        os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_CMD", None)
        os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", None)
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "skipped"
        assert (
            out["reason"].startswith("wallet_sdk_import_error:")
            or out["reason"].startswith("sdk_submit_")
            or out["reason"] == "no_unspent_xch_coins"
        )
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_CMD"] = old
        if old_backend is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD"] = old_backend


def test_worker_delegates_to_signer(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    signer = tmp_path / "signer.py"
    signer.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","spend_bundle_hex":"00ff"}))\n'
        ),
        encoding="utf-8",
    )
    old = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_CMD")
    try:
        os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_CMD"] = f"{sys.executable} {signer}"
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "executed"
        assert out["spend_bundle_hex"] == "00ff"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_CMD"] = old
