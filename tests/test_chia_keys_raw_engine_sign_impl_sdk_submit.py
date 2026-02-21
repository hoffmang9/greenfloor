import os
import sys
from pathlib import Path
from types import SimpleNamespace

from greenfloor.cli.chia_keys_raw_engine_sign_impl_sdk_submit import execute_payload


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
        },
        "additions": [
            {
                "address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
                "amount": 10,
            },
            {
                "address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
                "amount": 10,
            },
        ],
        "selected_coins": [
            {"coin_id": "03", "parent_coin_info": "01", "puzzle_hash": "02", "amount": 50}
        ],
    }


def test_sdk_submit_in_process_missing_key_mapping(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    old = os.getenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD")
    try:
        os.environ.pop("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD", None)
        payload = _payload(str(keyring))
        payload["key_id"] = "k1"
        out = execute_payload(payload)
        assert out["status"] == "skipped"
        assert out["reason"] == "sdk_submit_in_process_failed:key_id_fingerprint_mapping_missing"
        assert out["submit_request"]["plan"]["target_total_base_units"] == 20
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD"] = old


def test_sdk_submit_in_process_success_with_mocks(tmp_path: Path, monkeypatch) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    payload = _payload(str(keyring))
    payload["key_id"] = "1001"

    class _MasterPrivateKey:
        def __bytes__(self) -> bytes:
            return b"\x11" * 32

    class _FakeKeyData:
        private_key = _MasterPrivateKey()

    class _FakeKeychain:
        def get_key(self, _fingerprint: int, include_secrets: bool = True) -> _FakeKeyData:
            _ = include_secrets
            return _FakeKeyData()

    class _KeyringWrapper:
        @staticmethod
        def cleanup_shared_instance() -> None:
            return None

    class _SdkSignature:
        @staticmethod
        def from_bytes(_b: bytes) -> "_SdkSignature":
            return _SdkSignature()

    class _SdkSpendBundle:
        def __init__(self, _coin_spends, _sig) -> None:
            pass

        @staticmethod
        def to_bytes() -> bytes:
            return b"\xaa\xbb"

    class _SdkAddress:
        @staticmethod
        def decode(_address: str) -> SimpleNamespace:
            return SimpleNamespace(puzzle_hash=b"\x02")

    class _SdkCoin:
        def __init__(self, parent_coin_info: bytes, puzzle_hash: bytes, amount: int) -> None:
            self.parent_coin_info = parent_coin_info
            self.puzzle_hash = puzzle_hash
            self.amount = amount

        def coin_id(self) -> bytes:
            return b"\x03"

    class _PendingSpend:
        def coin(self) -> _SdkCoin:
            return _SdkCoin(b"\x01", b"\x02", 50)

        def conditions(self) -> list[str]:
            return ["c"]

    class _Finished:
        @staticmethod
        def pending_spends() -> list[_PendingSpend]:
            return [_PendingSpend()]

    class _Spends:
        def __init__(self, _clvm, _change_puzzle_hash) -> None:
            pass

        def add_xch(self, _coin) -> None:
            return None

        def apply(self, _actions):
            return object()

        def prepare(self, _deltas) -> _Finished:
            return _Finished()

    class _Clvm:
        def delegated_spend(self, _conditions):
            return object()

        def spend_standard_coin(self, _coin, _synthetic_key, _spend) -> None:
            return None

        @staticmethod
        def coin_spends() -> list[SimpleNamespace]:
            coin = SimpleNamespace(parent_coin_info=b"\x01", puzzle_hash=b"\x02", amount=50)
            return [SimpleNamespace(coin=coin, puzzle_reveal=b"\x80", solution=b"\x80")]

    class _SdkSecretKey:
        @staticmethod
        def from_bytes(_b: bytes) -> "_SdkSecretKey":
            return _SdkSecretKey()

        def derive_unhardened_path(self, _path) -> "_SdkSecretKey":
            return self

        def derive_hardened_path(self, _path) -> "_SdkSecretKey":
            return self

        def derive_synthetic(self) -> "_SdkSecretKey":
            return self

        def public_key(self) -> SimpleNamespace:
            return SimpleNamespace(to_bytes=lambda: b"\x10" * 48)

        def to_bytes(self) -> bytes:
            return b"\x22" * 32

    class _SdkAction:
        @staticmethod
        def send(_id, _puzzle_hash, _amount):
            return object()

    class _SdkId:
        @staticmethod
        def xch():
            return object()

    class _Sdk:
        SecretKey = _SdkSecretKey
        Signature = _SdkSignature
        SpendBundle = _SdkSpendBundle
        Address = _SdkAddress
        Coin = _SdkCoin
        Spends = _Spends
        Clvm = _Clvm
        Action = _SdkAction
        Id = _SdkId

        @staticmethod
        def standard_puzzle_hash(_public_key) -> bytes:
            return b"\x02"

        @staticmethod
        def to_hex(value: bytes) -> str:
            return value.hex()

    class _SerializedProgram:
        @staticmethod
        def from_bytes(_b: bytes):
            return object()

    class _Coin:
        def __init__(self, _parent, _puzzle_hash, _amount) -> None:
            pass

    class _PrivateKey:
        @staticmethod
        def from_bytes(_b: bytes):
            return object()

    class _AugScheme:
        @staticmethod
        def sign(_sk, _message):
            return b"sig"

        @staticmethod
        def aggregate(_sigs):
            return b"agg"

    class _ChiaRs:
        PrivateKey = _PrivateKey
        AugSchemeMPL = _AugScheme
        Coin = _Coin

    class _ConditionTools:
        @staticmethod
        def conditions_dict_for_solution(_puzzle_reveal, _solution, _cost):
            return {}

        @staticmethod
        def pkm_pairs_for_conditions_dict(_conditions_dict, _coin, _additional_data):
            class _Pk:
                def __bytes__(self) -> bytes:
                    return b"\x10" * 48

            return [(_Pk(), b"msg")]

    def _fake_import(name: str):
        if name == "chia_wallet_sdk":
            return _Sdk
        if name == "chia.util.keychain":
            return SimpleNamespace(
                set_keys_root_path=lambda _p: None, Keychain=lambda: _FakeKeychain()
            )
        if name == "chia.util.keyring_wrapper":
            return SimpleNamespace(KeyringWrapper=_KeyringWrapper)
        if name == "chia.consensus.condition_tools":
            return _ConditionTools
        if name == "chia.consensus.default_constants":
            return SimpleNamespace(DEFAULT_CONSTANTS=SimpleNamespace(MAX_BLOCK_COST_CLVM=10))
        if name == "chia.types.blockchain_format.serialized_program":
            return SimpleNamespace(SerializedProgram=_SerializedProgram)
        if name == "chia_rs":
            return _ChiaRs
        raise AssertionError(name)

    import importlib

    monkeypatch.setattr(importlib, "import_module", _fake_import)
    out = execute_payload(payload)
    assert out["status"] == "executed"
    assert out["reason"] == "sdk_submit_in_process_success"
    assert out["spend_bundle_hex"] == "aabb"


def test_sdk_submit_delegates_to_cmd(tmp_path: Path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    script = tmp_path / "submit.py"
    script.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","spend_bundle_hex":"00ff"}))\n'
        ),
        encoding="utf-8",
    )
    old = os.getenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD")
    try:
        os.environ["GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD"] = (
            f"{sys.executable} {script}"
        )
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "executed"
        assert out["spend_bundle_hex"] == "00ff"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD"] = old
