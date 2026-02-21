import os
import sys
from pathlib import Path

from greenfloor.cli.chia_keys_executor import execute_payload


def _payload(keyring_yaml_path: str, asset_id: str = "xch", op_type: str = "split") -> dict:
    return {
        "selected_source": "chia_keys",
        "key_id": "fingerprint:123456789",
        "network": "testnet11",
        "asset_id": asset_id,
        "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        "plan": {
            "op_type": op_type,
            "size_base_units": 10,
            "op_count": 1,
            "reason": "r",
        },
        "signer_selection": {
            "selected_source": "chia_keys",
            "key_id": "fingerprint:123456789",
            "network": "testnet11",
            "keyring_yaml_path": keyring_yaml_path,
        },
    }


def test_chia_keys_executor_checks_keyring_exists(tmp_path: Path) -> None:
    out = execute_payload(_payload(str(tmp_path / "missing.yaml")))
    assert out["status"] == "skipped"
    assert out["reason"] == "keyring_yaml_not_found"


def test_chia_keys_executor_restricts_asset_type(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    out = execute_payload(_payload(str(keyring), asset_id="byc-cat-id"))
    assert out["status"] == "skipped"
    assert out["reason"] == "asset_not_supported_yet"


def test_chia_keys_executor_restricts_op_type(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    out = execute_payload(_payload(str(keyring), op_type="other"))
    assert out["status"] == "skipped"
    assert out["reason"] == "unsupported_operation_type"


def test_chia_keys_executor_delegates_when_signer_backend_is_set(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    script = tmp_path / "delegate.py"
    script.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","operation_id":"tx-live"}))\n'
        ),
        encoding="utf-8",
    )

    old = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD")
    try:
        os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD"] = f"{sys.executable} {script}"
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "executed"
        assert out["operation_id"] == "tx-live"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD"] = old


def test_chia_keys_executor_uses_default_signer_backend_module(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")

    old_exec = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD")
    old_worker = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD")
    script = tmp_path / "sign.py"
    script.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","operation_id":"z1"}))\n'
        ),
        encoding="utf-8",
    )
    try:
        os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", None)
        os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD"] = f"{sys.executable} {script}"
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "skipped"
        assert (
            out["reason"].startswith("wallet_sdk_import_error:")
            or out["reason"] == "no_unspent_xch_coins"
        )
    finally:
        if old_exec is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD"] = old_exec
        if old_worker is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD"] = old_worker


def test_chia_keys_executor_broadcasts_spend_bundle_from_signer_backend(
    tmp_path: Path, monkeypatch
) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    script = tmp_path / "delegate_bundle.py"
    script.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"spend_bundle_hex":"00ff"}))\n'
        ),
        encoding="utf-8",
    )

    class _FakeSpendBundle:
        @staticmethod
        def from_bytes(_b: bytes):
            class _Bundle:
                @staticmethod
                def hash() -> bytes:
                    return b"\x12\x34"

            return _Bundle()

    class _FakeResponse:
        success = True
        status = "SUCCESS"
        error = None

    class _FakeClient:
        async def push_tx(self, _bundle):
            return _FakeResponse()

    class _FakeRpcClient:
        @staticmethod
        def testnet11():
            return _FakeClient()

        @staticmethod
        def mainnet():
            return _FakeClient()

        def __init__(self, _url: str):
            pass

    class _FakeSdk:
        SpendBundle = _FakeSpendBundle
        RpcClient = _FakeRpcClient

        @staticmethod
        def to_hex(value: bytes) -> str:
            return value.hex()

    import importlib

    monkeypatch.setattr(importlib, "import_module", lambda _name: _FakeSdk)

    old = os.getenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD")
    try:
        os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD"] = f"{sys.executable} {script}"
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "executed"
        assert out["operation_id"] == "1234"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD"] = old
