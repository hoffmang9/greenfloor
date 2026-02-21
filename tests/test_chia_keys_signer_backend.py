import os
import sys
from pathlib import Path

from greenfloor.cli.chia_keys_signer_backend import execute_payload


def _payload(keyring_yaml_path: str) -> dict:
    return {
        "key_id": "fingerprint:123456789",
        "network": "testnet11",
        "asset_id": "xch",
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


def test_backend_returns_request_when_sign_missing(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    old = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD")
    try:
        os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", None)
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "skipped"
        assert (
            out["reason"].startswith("wallet_sdk_import_error:")
            or out["reason"].startswith("sdk_submit_")
            or out["reason"] == "no_unspent_xch_coins"
        )
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD"] = old


def test_backend_delegates_to_sign(tmp_path: Path, monkeypatch) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    script = tmp_path / "sign.py"
    script.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","spend_bundle_hex":"00ff"}))\n'
        ),
        encoding="utf-8",
    )

    class _Coin:
        parent_coin_info = b"\x01"
        puzzle_hash = b"\x02"
        amount = 50

        @staticmethod
        def coin_id() -> bytes:
            return b"\x03"

    class _Sdk:
        @staticmethod
        def select_coins(coins, amount):
            _ = amount
            return coins[:1]

        @staticmethod
        def to_hex(v: bytes) -> str:
            return v.hex()

    import importlib

    import greenfloor.cli.chia_keys_signer_backend as backend

    monkeypatch.setattr(importlib, "import_module", lambda _name: _Sdk)
    monkeypatch.setattr(backend, "_list_unspent_xch_coins", lambda **_kwargs: [_Coin()])

    old = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD")
    try:
        os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD"] = f"{sys.executable} {script}"
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "executed"
        assert out["spend_bundle_hex"] == "00ff"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD"] = old
