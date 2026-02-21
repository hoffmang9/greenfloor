import os
import sys
from pathlib import Path

from greenfloor.cli.wallet_executor import execute_payload


def test_wallet_executor_requires_source() -> None:
    out = execute_payload({})
    assert out["status"] == "skipped"
    assert out["reason"] == "missing_selected_source"


def test_wallet_executor_skips_when_source_delegate_missing() -> None:
    old = os.getenv("GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD")
    try:
        os.environ.pop("GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD", None)
        out = execute_payload(
            {
                "selected_source": "chia_keys",
                "key_id": "fingerprint:123456789",
                "network": "testnet11",
                "plan": {"op_type": "split", "size_base_units": 10, "op_count": 1, "reason": "r"},
            }
        )
        assert out["status"] == "skipped"
        assert out["reason"] == "missing_keyring_yaml_path"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD"] = old


def test_wallet_executor_delegates_to_source_command(tmp_path: Path) -> None:
    script = tmp_path / "delegate.py"
    script.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","operation_id":"tx-42"}))\n'
        ),
        encoding="utf-8",
    )
    old = os.getenv("GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD")
    try:
        os.environ["GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD"] = f"{sys.executable} {script}"
        out = execute_payload(
            {
                "selected_source": "chia_keys",
                "key_id": "fingerprint:123456789",
                "network": "testnet11",
                "signer_selection": {"keyring_yaml_path": str(tmp_path / "dummy.yaml")},
                "plan": {"op_type": "combine", "size_base_units": 50, "op_count": 1, "reason": "r"},
            }
        )
        assert out["status"] == "executed"
        assert out["operation_id"] == "tx-42"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD"] = old


def test_wallet_executor_full_default_chain_with_sdk_submit_override(
    tmp_path: Path, monkeypatch
) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")

    fake_sdk = tmp_path / "chia_wallet_sdk.py"
    fake_sdk.write_text(
        (
            "class Address:\n"
            "    @staticmethod\n"
            "    def decode(_address):\n"
            "        class _A:\n"
            "            puzzle_hash = b'\\x02'\n"
            "        return _A()\n"
            "\n"
            "class Coin:\n"
            "    def __init__(self, parent_coin_info, puzzle_hash, amount):\n"
            "        self.parent_coin_info = parent_coin_info\n"
            "        self.puzzle_hash = puzzle_hash\n"
            "        self.amount = amount\n"
            "    def coin_id(self):\n"
            "        return b'\\x03'\n"
            "\n"
            "class _CoinRecord:\n"
            "    def __init__(self, coin):\n"
            "        self.coin = coin\n"
            "\n"
            "class _GetCoinRecordsResp:\n"
            "    success = True\n"
            "    coin_records = [_CoinRecord(Coin(b'\\x01', b'\\x02', 50))]\n"
            "\n"
            "class _PushResp:\n"
            "    success = True\n"
            "    status = 'SUCCESS'\n"
            "    error = None\n"
            "\n"
            "class _RpcClient:\n"
            "    async def get_coin_records_by_puzzle_hash(self, _puzzle_hash, includeSpentCoins=False):\n"
            "        _ = includeSpentCoins\n"
            "        return _GetCoinRecordsResp()\n"
            "    async def push_tx(self, _spend_bundle):\n"
            "        return _PushResp()\n"
            "\n"
            "class RpcClient:\n"
            "    def __init__(self, _url):\n"
            "        pass\n"
            "    @staticmethod\n"
            "    def testnet11():\n"
            "        return _RpcClient()\n"
            "    @staticmethod\n"
            "    def mainnet():\n"
            "        return _RpcClient()\n"
            "\n"
            "class SpendBundle:\n"
            "    @staticmethod\n"
            "    def from_bytes(_b):\n"
            "        class _Bundle:\n"
            "            @staticmethod\n"
            "            def hash():\n"
            "                return b'\\x12\\x34'\n"
            "        return _Bundle()\n"
            "\n"
            "def select_coins(coins, _amount):\n"
            "    return [coins[0]]\n"
            "\n"
            "def to_hex(value):\n"
            "    return value.hex()\n"
        ),
        encoding="utf-8",
    )

    submit = tmp_path / "submit.py"
    submit.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","spend_bundle_hex":"00ff"}))\n'
        ),
        encoding="utf-8",
    )

    existing_pythonpath = os.environ.get("PYTHONPATH", "")
    pythonpath_parts = [str(tmp_path)]
    if existing_pythonpath:
        pythonpath_parts.append(existing_pythonpath)
    monkeypatch.setenv("PYTHONPATH", os.pathsep.join(pythonpath_parts))
    monkeypatch.setenv(
        "GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD",
        f"{sys.executable} {submit}",
    )
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD_LEGACY", raising=False)

    out = execute_payload(
        {
            "selected_source": "chia_keys",
            "key_id": "fingerprint:123456789",
            "network": "testnet11",
            "market_id": "m1",
            "asset_id": "xch",
            "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 2, "reason": "r"},
            "signer_selection": {
                "selected_source": "chia_keys",
                "key_id": "fingerprint:123456789",
                "network": "testnet11",
                "keyring_yaml_path": str(keyring),
            },
        }
    )
    assert out["status"] == "executed"
    assert out["operation_id"] == "1234"


def test_wallet_executor_full_default_chain_propagates_no_unspent_coins_reason(
    tmp_path: Path, monkeypatch
) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")

    fake_sdk = tmp_path / "chia_wallet_sdk.py"
    fake_sdk.write_text(
        (
            "class Address:\n"
            "    @staticmethod\n"
            "    def decode(_address):\n"
            "        class _A:\n"
            "            puzzle_hash = b'\\x02'\n"
            "        return _A()\n"
            "\n"
            "class _GetCoinRecordsResp:\n"
            "    success = True\n"
            "    coin_records = []\n"
            "\n"
            "class _RpcClient:\n"
            "    async def get_coin_records_by_puzzle_hash(self, _puzzle_hash, includeSpentCoins=False):\n"
            "        _ = includeSpentCoins\n"
            "        return _GetCoinRecordsResp()\n"
            "\n"
            "class RpcClient:\n"
            "    def __init__(self, _url):\n"
            "        pass\n"
            "    @staticmethod\n"
            "    def testnet11():\n"
            "        return _RpcClient()\n"
            "    @staticmethod\n"
            "    def mainnet():\n"
            "        return _RpcClient()\n"
        ),
        encoding="utf-8",
    )

    existing_pythonpath = os.environ.get("PYTHONPATH", "")
    pythonpath_parts = [str(tmp_path)]
    if existing_pythonpath:
        pythonpath_parts.append(existing_pythonpath)
    monkeypatch.setenv("PYTHONPATH", os.pathsep.join(pythonpath_parts))
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD_LEGACY", raising=False)

    out = execute_payload(
        {
            "selected_source": "chia_keys",
            "key_id": "fingerprint:123456789",
            "network": "testnet11",
            "market_id": "m1",
            "asset_id": "xch",
            "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 2, "reason": "r"},
            "signer_selection": {
                "selected_source": "chia_keys",
                "key_id": "fingerprint:123456789",
                "network": "testnet11",
                "keyring_yaml_path": str(keyring),
            },
        }
    )
    assert out["status"] == "skipped"
    assert out["reason"] == "no_unspent_xch_coins"


def test_wallet_executor_full_default_chain_propagates_coin_selection_failed_reason(
    tmp_path: Path, monkeypatch
) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")

    fake_sdk = tmp_path / "chia_wallet_sdk.py"
    fake_sdk.write_text(
        (
            "class Address:\n"
            "    @staticmethod\n"
            "    def decode(_address):\n"
            "        class _A:\n"
            "            puzzle_hash = b'\\x02'\n"
            "        return _A()\n"
            "\n"
            "class Coin:\n"
            "    def __init__(self, parent_coin_info, puzzle_hash, amount):\n"
            "        self.parent_coin_info = parent_coin_info\n"
            "        self.puzzle_hash = puzzle_hash\n"
            "        self.amount = amount\n"
            "\n"
            "class _CoinRecord:\n"
            "    def __init__(self, coin):\n"
            "        self.coin = coin\n"
            "\n"
            "class _GetCoinRecordsResp:\n"
            "    success = True\n"
            "    coin_records = [_CoinRecord(Coin(b'\\x01', b'\\x02', 50))]\n"
            "\n"
            "class _RpcClient:\n"
            "    async def get_coin_records_by_puzzle_hash(self, _puzzle_hash, includeSpentCoins=False):\n"
            "        _ = includeSpentCoins\n"
            "        return _GetCoinRecordsResp()\n"
            "\n"
            "class RpcClient:\n"
            "    def __init__(self, _url):\n"
            "        pass\n"
            "    @staticmethod\n"
            "    def testnet11():\n"
            "        return _RpcClient()\n"
            "    @staticmethod\n"
            "    def mainnet():\n"
            "        return _RpcClient()\n"
            "\n"
            "def select_coins(_coins, _amount):\n"
            "    raise RuntimeError('boom')\n"
        ),
        encoding="utf-8",
    )

    existing_pythonpath = os.environ.get("PYTHONPATH", "")
    pythonpath_parts = [str(tmp_path)]
    if existing_pythonpath:
        pythonpath_parts.append(existing_pythonpath)
    monkeypatch.setenv("PYTHONPATH", os.pathsep.join(pythonpath_parts))
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD_LEGACY", raising=False)

    out = execute_payload(
        {
            "selected_source": "chia_keys",
            "key_id": "fingerprint:123456789",
            "network": "testnet11",
            "market_id": "m1",
            "asset_id": "xch",
            "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 2, "reason": "r"},
            "signer_selection": {
                "selected_source": "chia_keys",
                "key_id": "fingerprint:123456789",
                "network": "testnet11",
                "keyring_yaml_path": str(keyring),
            },
        }
    )
    assert out["status"] == "skipped"
    assert out["reason"].startswith("coin_selection_failed:")


def test_wallet_executor_full_default_chain_propagates_broadcast_failed_reason(
    tmp_path: Path, monkeypatch
) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")

    fake_sdk = tmp_path / "chia_wallet_sdk.py"
    fake_sdk.write_text(
        (
            "class Address:\n"
            "    @staticmethod\n"
            "    def decode(_address):\n"
            "        class _A:\n"
            "            puzzle_hash = b'\\x02'\n"
            "        return _A()\n"
            "\n"
            "class Coin:\n"
            "    def __init__(self, parent_coin_info, puzzle_hash, amount):\n"
            "        self.parent_coin_info = parent_coin_info\n"
            "        self.puzzle_hash = puzzle_hash\n"
            "        self.amount = amount\n"
            "    def coin_id(self):\n"
            "        return b'\\x03'\n"
            "\n"
            "class _CoinRecord:\n"
            "    def __init__(self, coin):\n"
            "        self.coin = coin\n"
            "\n"
            "class _GetCoinRecordsResp:\n"
            "    success = True\n"
            "    coin_records = [_CoinRecord(Coin(b'\\x01', b'\\x02', 50))]\n"
            "\n"
            "class _PushResp:\n"
            "    success = False\n"
            "    status = 'FAILED'\n"
            "    error = 'mempool_rejected'\n"
            "\n"
            "class _RpcClient:\n"
            "    async def get_coin_records_by_puzzle_hash(self, _puzzle_hash, includeSpentCoins=False):\n"
            "        _ = includeSpentCoins\n"
            "        return _GetCoinRecordsResp()\n"
            "    async def push_tx(self, _spend_bundle):\n"
            "        return _PushResp()\n"
            "\n"
            "class RpcClient:\n"
            "    def __init__(self, _url):\n"
            "        pass\n"
            "    @staticmethod\n"
            "    def testnet11():\n"
            "        return _RpcClient()\n"
            "    @staticmethod\n"
            "    def mainnet():\n"
            "        return _RpcClient()\n"
            "\n"
            "class SpendBundle:\n"
            "    @staticmethod\n"
            "    def from_bytes(_b):\n"
            "        class _Bundle:\n"
            "            @staticmethod\n"
            "            def hash():\n"
            "                return b'\\x12\\x34'\n"
            "        return _Bundle()\n"
            "\n"
            "def select_coins(coins, _amount):\n"
            "    return [coins[0]]\n"
            "\n"
            "def to_hex(value):\n"
            "    return value.hex()\n"
        ),
        encoding="utf-8",
    )

    submit = tmp_path / "submit.py"
    submit.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","spend_bundle_hex":"00ff"}))\n'
        ),
        encoding="utf-8",
    )

    existing_pythonpath = os.environ.get("PYTHONPATH", "")
    pythonpath_parts = [str(tmp_path)]
    if existing_pythonpath:
        pythonpath_parts.append(existing_pythonpath)
    monkeypatch.setenv("PYTHONPATH", os.pathsep.join(pythonpath_parts))
    monkeypatch.setenv(
        "GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_SDK_SUBMIT_CMD",
        f"{sys.executable} {submit}",
    )
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_SIGN_CMD", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_RAW_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_CMD_LEGACY", raising=False)
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_RAW_ENGINE_SIGN_IMPL_CMD_LEGACY", raising=False)

    out = execute_payload(
        {
            "selected_source": "chia_keys",
            "key_id": "fingerprint:123456789",
            "network": "testnet11",
            "market_id": "m1",
            "asset_id": "xch",
            "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 2, "reason": "r"},
            "signer_selection": {
                "selected_source": "chia_keys",
                "key_id": "fingerprint:123456789",
                "network": "testnet11",
                "keyring_yaml_path": str(keyring),
            },
        }
    )
    assert out["status"] == "skipped"
    assert out["reason"].startswith("broadcast_failed:")


def test_wallet_executor_propagates_signer_backend_invalid_json_reason(
    tmp_path: Path, monkeypatch
) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")

    script = tmp_path / "bad_signer_backend.py"
    script.write_text("print('not-json')\n", encoding="utf-8")

    monkeypatch.setenv(
        "GREENFLOOR_CHIA_KEYS_SIGNER_BACKEND_CMD",
        f"{sys.executable} {script}",
    )
    monkeypatch.delenv("GREENFLOOR_CHIA_KEYS_EXECUTOR_CMD", raising=False)

    out = execute_payload(
        {
            "selected_source": "chia_keys",
            "key_id": "fingerprint:123456789",
            "network": "testnet11",
            "market_id": "m1",
            "asset_id": "xch",
            "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            "plan": {"op_type": "split", "size_base_units": 10, "op_count": 2, "reason": "r"},
            "signer_selection": {
                "selected_source": "chia_keys",
                "key_id": "fingerprint:123456789",
                "network": "testnet11",
                "keyring_yaml_path": str(keyring),
            },
        }
    )
    assert out["status"] == "skipped"
    assert out["reason"] == "signer_backend_invalid_json"
