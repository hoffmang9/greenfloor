import os
import sys
from pathlib import Path

from greenfloor.adapters.wallet import WalletAdapter


def test_wallet_adapter_fake_coin_env_path() -> None:
    adapter = WalletAdapter()
    old = os.getenv("GREENFLOOR_FAKE_COINS_JSON")
    try:
        os.environ["GREENFLOOR_FAKE_COINS_JSON"] = '{"asset1":[1,10,100]}'
        got = adapter.list_asset_coins_base_units(
            asset_id="asset1",
            key_id="fingerprint:123456789",
            receive_address="xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            network="mainnet",
        )
        assert got == [1, 10, 100]
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_FAKE_COINS_JSON", None)
        else:
            os.environ["GREENFLOOR_FAKE_COINS_JSON"] = old


def test_wallet_adapter_cat_defaults_to_empty_without_cat_mapping() -> None:
    adapter = WalletAdapter()
    old_all = os.getenv("GREENFLOOR_FAKE_COINS_JSON")
    old_cat = os.getenv("GREENFLOOR_FAKE_CAT_COINS_JSON")
    try:
        os.environ.pop("GREENFLOOR_FAKE_COINS_JSON", None)
        os.environ.pop("GREENFLOOR_FAKE_CAT_COINS_JSON", None)
        got = adapter.list_asset_coins_base_units(
            asset_id="byc-cat-id",
            key_id="fingerprint:123456789",
            receive_address="xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            network="mainnet",
        )
        assert got == []
    finally:
        if old_all is None:
            os.environ.pop("GREENFLOOR_FAKE_COINS_JSON", None)
        else:
            os.environ["GREENFLOOR_FAKE_COINS_JSON"] = old_all
        if old_cat is None:
            os.environ.pop("GREENFLOOR_FAKE_CAT_COINS_JSON", None)
        else:
            os.environ["GREENFLOOR_FAKE_CAT_COINS_JSON"] = old_cat


def test_wallet_adapter_cat_uses_cat_mapping_when_set() -> None:
    adapter = WalletAdapter()
    old = os.getenv("GREENFLOOR_FAKE_CAT_COINS_JSON")
    try:
        os.environ["GREENFLOOR_FAKE_CAT_COINS_JSON"] = '{"byc-cat-id":[10,10,100]}'
        got = adapter.list_asset_coins_base_units(
            asset_id="byc-cat-id",
            key_id="fingerprint:123456789",
            receive_address="xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            network="mainnet",
        )
        assert got == [10, 10, 100]
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_FAKE_CAT_COINS_JSON", None)
        else:
            os.environ["GREENFLOOR_FAKE_CAT_COINS_JSON"] = old


def test_wallet_adapter_execute_coin_ops_items() -> None:
    from greenfloor.core.coin_ops import CoinOpPlan

    adapter = WalletAdapter()
    result = adapter.execute_coin_ops(
        plans=[CoinOpPlan(op_type="split", size_base_units=10, op_count=2, reason="r")],
        dry_run=True,
        key_id="fingerprint:123456789",
        network="testnet11",
    )
    assert result["planned_count"] == 1
    assert result["executed_count"] == 0
    assert result["items"][0]["status"] == "planned"


def test_wallet_adapter_non_dry_run_requires_signer_selection(tmp_path: Path) -> None:
    from greenfloor.core.coin_ops import CoinOpPlan

    adapter = WalletAdapter()
    result = adapter.execute_coin_ops(
        plans=[CoinOpPlan(op_type="split", size_base_units=10, op_count=1, reason="r")],
        dry_run=False,
        key_id="fingerprint:123456789",
        network="testnet11",
        onboarding_selection_path=tmp_path / "missing.json",
    )
    assert result["executed_count"] == 0
    assert result["items"][0]["status"] == "skipped"
    assert result["items"][0]["reason"] == "missing_signer_selection"


def test_wallet_adapter_non_dry_run_uses_external_executor(tmp_path: Path) -> None:
    from greenfloor.core.coin_ops import CoinOpPlan
    from greenfloor.keys.onboarding import KeyOnboardingSelection, save_key_onboarding_selection

    onboarding_path = tmp_path / "key_onboarding.json"
    save_key_onboarding_selection(
        onboarding_path,
        KeyOnboardingSelection(
            selected_source="chia_keys",
            key_id="fingerprint:123456789",
            network="testnet11",
            chia_keys_dir=str(tmp_path / ".chia_keys"),
            keyring_yaml_path=str(tmp_path / ".chia_keys/keyring.yaml"),
        ),
    )
    script = tmp_path / "exec.py"
    script.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"executor_success","operation_id":"tx-1"}))\n'
        ),
        encoding="utf-8",
    )

    old_cmd = os.getenv("GREENFLOOR_WALLET_EXECUTOR_CMD")
    try:
        os.environ["GREENFLOOR_WALLET_EXECUTOR_CMD"] = f"{sys.executable} {script}"
        adapter = WalletAdapter()
        result = adapter.execute_coin_ops(
            plans=[CoinOpPlan(op_type="combine", size_base_units=50, op_count=1, reason="r")],
            dry_run=False,
            key_id="fingerprint:123456789",
            network="testnet11",
            onboarding_selection_path=onboarding_path,
        )
        assert result["executed_count"] == 1
        assert result["items"][0]["status"] == "executed"
        assert result["items"][0]["operation_id"] == "tx-1"
    finally:
        if old_cmd is None:
            os.environ.pop("GREENFLOOR_WALLET_EXECUTOR_CMD", None)
        else:
            os.environ["GREENFLOOR_WALLET_EXECUTOR_CMD"] = old_cmd


def test_wallet_adapter_non_dry_run_direct_signing(tmp_path: Path, monkeypatch) -> None:
    """When no GREENFLOOR_WALLET_EXECUTOR_CMD is set, uses signing.sign_and_broadcast directly."""
    from greenfloor.core.coin_ops import CoinOpPlan
    from greenfloor.keys.onboarding import KeyOnboardingSelection, save_key_onboarding_selection

    onboarding_path = tmp_path / "key_onboarding.json"
    save_key_onboarding_selection(
        onboarding_path,
        KeyOnboardingSelection(
            selected_source="chia_keys",
            key_id="fingerprint:123456789",
            network="testnet11",
            chia_keys_dir=str(tmp_path / ".chia_keys"),
            keyring_yaml_path=str(tmp_path / ".chia_keys/keyring.yaml"),
        ),
    )

    monkeypatch.delenv("GREENFLOOR_WALLET_EXECUTOR_CMD", raising=False)

    import greenfloor.signing as signing_mod

    captured: dict = {}

    def _fake_sign_and_broadcast(payload):
        captured["payload"] = payload
        return {"status": "executed", "reason": "ok", "operation_id": "tx-direct"}

    monkeypatch.setattr(
        signing_mod,
        "sign_and_broadcast",
        _fake_sign_and_broadcast,
    )

    monkeypatch.setenv("GREENFLOOR_KEY_ID_FINGERPRINT_MAP_JSON", "{}")

    adapter = WalletAdapter()
    result = adapter.execute_coin_ops(
        plans=[CoinOpPlan(op_type="split", size_base_units=10, op_count=1, reason="r")],
        dry_run=False,
        key_id="fingerprint:123456789",
        network="testnet11",
        onboarding_selection_path=onboarding_path,
        signer_fingerprint=123456789,
    )
    assert result["executed_count"] == 1
    assert result["items"][0]["status"] == "executed"
    assert result["items"][0]["operation_id"] == "tx-direct"
    assert captured["payload"]["key_id_fingerprint_map"] == {
        "fingerprint:123456789": "123456789"
    }
    assert os.getenv("GREENFLOOR_KEY_ID_FINGERPRINT_MAP_JSON") == "{}"
