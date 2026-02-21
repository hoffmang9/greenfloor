import os
import sys
from pathlib import Path

from greenfloor.cli.chia_keys_signer import execute_payload


def _payload(keyring_yaml_path: str) -> dict:
    return {
        "selected_source": "chia_keys",
        "key_id": "fingerprint:123456789",
        "network": "testnet11",
        "asset_id": "xch",
        "market_id": "m1",
        "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        "keyring_yaml_path": keyring_yaml_path,
        "plan": {
            "op_type": "split",
            "size_base_units": 10,
            "op_count": 2,
            "target_total_base_units": 20,
            "reason": "r",
        },
    }


def test_signer_returns_backend_request_when_backend_missing(tmp_path: Path, monkeypatch) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    old = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD")
    try:
        os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", None)
        import importlib

        monkeypatch.setattr(importlib, "import_module", lambda _name: object())
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "skipped"
        assert (
            out["reason"].startswith("wallet_sdk_import_error:")
            or out["reason"].startswith("sdk_submit_")
            or out["reason"] == "no_unspent_xch_coins"
        )
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD"] = old


def test_signer_delegates_to_backend(tmp_path: Path, monkeypatch) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    backend = tmp_path / "backend.py"
    backend.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","spend_bundle_hex":"00ff"}))\n'
        ),
        encoding="utf-8",
    )
    old = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD")
    try:
        os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD"] = f"{sys.executable} {backend}"
        import importlib

        monkeypatch.setattr(importlib, "import_module", lambda _name: object())
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "executed"
        assert out["spend_bundle_hex"] == "00ff"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD"] = old
