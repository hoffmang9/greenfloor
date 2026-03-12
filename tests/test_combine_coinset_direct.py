from __future__ import annotations

import importlib.util
import sys
from pathlib import Path
from typing import Any


def _load_script_module() -> Any:
    script_path = Path(__file__).resolve().parents[1] / "scripts" / "combine_coinset_direct.py"
    spec = importlib.util.spec_from_file_location("combine_coinset_direct", script_path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _required_argv() -> list[str]:
    return [
        "--coin-name",
        "0x" + ("a" * 64),
        "--coin-name",
        "0x" + ("b" * 64),
        "--key-id",
        "key-1",
        "--keyring-yaml-path",
        "/tmp/keyring.yaml",
        "--receive-address",
        "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w",
        "--cloud-wallet-base-url",
        "https://wallet.example",
        "--cloud-wallet-user-key-id",
        "user-key",
        "--cloud-wallet-private-key-pem-path",
        "/tmp/key.pem",
        "--vault-id",
        "Vault_123",
        "--cloud-wallet-kms-key-id",
        "arn:aws:kms:us-west-2:123:key/abc",
    ]


def test_normalize_coin_names_dedupes_and_preserves_order() -> None:
    mod = _load_script_module()
    values = [
        "0x" + ("b" * 64),
        "0x" + ("a" * 64),
        "0x" + ("b" * 64),
        "not-a-coin",
    ]
    assert mod._normalize_coin_names(values) == [("b" * 64), ("a" * 64)]


def test_select_input_coin_ids_takes_first_n() -> None:
    mod = _load_script_module()
    source = [f"{idx:064x}" for idx in range(1, 6)]
    assert mod._select_input_coin_ids(source, 3) == source[:3]


def test_kms_resolution_check_skips_live_probe_by_default() -> None:
    mod = _load_script_module()
    calls = {"pubkey": 0, "sign": 0}

    def _pubkey(_key_id: str, _region: str) -> str:
        calls["pubkey"] += 1
        return "02" + ("1" * 64)

    def _sign(_key_id: str, _region: str, _message_hex: str) -> str:
        calls["sign"] += 1
        return "0" * 128

    result = mod._kms_resolution_check(
        kms_key_id="arn:aws:kms:us-west-2:123:key/abc",
        kms_region="us-west-2",
        kms_live_probe=False,
        live_probe_message_hex="00" * 32,
        kms_pubkey_resolver=_pubkey,
        kms_signer=_sign,
    )
    assert result["ok"] is True
    assert result["live_probe_ran"] is False
    assert calls == {"pubkey": 1, "sign": 0}


def test_kms_resolution_check_runs_live_probe_when_requested() -> None:
    mod = _load_script_module()
    calls = {"pubkey": 0, "sign": 0}

    def _pubkey(_key_id: str, _region: str) -> str:
        calls["pubkey"] += 1
        return "03" + ("2" * 64)

    def _sign(_key_id: str, _region: str, _message_hex: str) -> str:
        calls["sign"] += 1
        return "a" * 128

    result = mod._kms_resolution_check(
        kms_key_id="arn:aws:kms:us-west-2:123:key/abc",
        kms_region="us-west-2",
        kms_live_probe=True,
        live_probe_message_hex="11" * 32,
        kms_pubkey_resolver=_pubkey,
        kms_signer=_sign,
    )
    assert result["ok"] is True
    assert result["live_probe_ran"] is True
    assert calls == {"pubkey": 1, "sign": 1}


def test_run_preflight_only_does_not_broadcast(monkeypatch) -> None:
    mod = _load_script_module()
    parser = mod._build_parser()
    args = parser.parse_args(_required_argv() + ["--preflight-only"])

    class _FakeCoinset:
        network = "mainnet"
        base_url = "https://api.coinset.org"

        def __init__(self, *args, **kwargs) -> None:
            _ = args, kwargs

        @staticmethod
        def get_coin_records_by_names(*, coin_names_hex: list[str], include_spent_coins: bool):
            _ = include_spent_coins
            rows = []
            for name in coin_names_hex:
                raw = name.replace("0x", "")
                rows.append(
                    {
                        "coin": {
                            "name": f"0x{raw}",
                            "amount": 1000,
                        },
                        "spent_block_index": 0,
                    }
                )
            return rows

        @staticmethod
        def get_blockchain_state():
            return {"peak_height": 1}

    class _FakeWallet:
        @staticmethod
        def get_vault_custody_snapshot():
            return {"vaultLauncherId": "0x" + ("c" * 64)}

    def _fake_wallet_factory(_config):
        return _FakeWallet()

    monkeypatch.setattr(
        mod,
        "_resolve_cat_asset_id_for_coin_ids",
        lambda **_kwargs: ("d" * 64, {"ok": True, "asset_id": "d" * 64}),
    )

    called = {"broadcast": 0}

    def _broadcast(_payload: dict[str, Any]) -> dict[str, Any]:
        called["broadcast"] += 1
        return {"status": "executed", "reason": "unexpected"}

    exit_code, payload = mod.run(
        args,
        coinset_factory=_FakeCoinset,
        cloud_wallet_factory=_fake_wallet_factory,
        sign_and_broadcast_fn=_broadcast,
        kms_pubkey_resolver=lambda *_args: "02" + ("1" * 64),
        kms_signer=lambda *_args: "f" * 128,
    )
    assert exit_code == 0
    assert payload["status"] == "preflight_ok"
    assert called["broadcast"] == 0


def test_wait_until_inputs_spent_uses_additive_warning_cadence() -> None:
    mod = _load_script_module()

    class _FakeCoinset:
        def __init__(self) -> None:
            self.calls = 0

        def get_coin_records_by_names(
            self, *, coin_names_hex: list[str], include_spent_coins: bool
        ):
            _ = coin_names_hex, include_spent_coins
            self.calls += 1
            if self.calls < 4:
                spent = 0
            else:
                spent = 12
            return [
                {
                    "coin": {"name": "0x" + ("a" * 64), "amount": 1000},
                    "spent_block_index": spent,
                }
            ]

    now = {"t": 0.0}

    def _sleep(seconds: int) -> None:
        now["t"] += float(seconds)

    def _monotonic() -> float:
        return now["t"]

    result = mod._wait_until_inputs_spent(
        coinset=_FakeCoinset(),
        input_coin_ids=["a" * 64],
        timeout_seconds=30,
        poll_seconds=5,
        warning_interval_seconds=10,
        sleep_fn=_sleep,
        monotonic_fn=_monotonic,
    )
    assert result["status"] == "spent"
    warning_elapsed = [item["elapsed_seconds"] for item in result["warnings"]]
    assert warning_elapsed == [10]


def test_run_includes_broadcast_diagnostics_on_failure_when_flagged(monkeypatch) -> None:
    mod = _load_script_module()
    parser = mod._build_parser()
    args = parser.parse_args(_required_argv() + ["--debug-broadcast-diagnostics"])

    class _FakeCoinset:
        network = "mainnet"
        base_url = "https://api.coinset.org"

        def __init__(self, *args, **kwargs) -> None:
            _ = args, kwargs

        @staticmethod
        def get_coin_records_by_names(*, coin_names_hex: list[str], include_spent_coins: bool):
            _ = include_spent_coins
            rows = []
            for name in coin_names_hex:
                raw = name.replace("0x", "")
                rows.append(
                    {
                        "coin": {
                            "name": f"0x{raw}",
                            "amount": 1000,
                        },
                        "spent_block_index": 0,
                    }
                )
            return rows

        @staticmethod
        def get_coin_record_by_name(*, coin_name_hex: str):
            raw = coin_name_hex.replace("0x", "")
            return {"coin": {"name": f"0x{raw}", "amount": 1000}, "spent_block_index": 0}

        @staticmethod
        def get_blockchain_state():
            return {"peak_height": 1}

    class _FakeWallet:
        @staticmethod
        def get_vault_custody_snapshot():
            return {"vaultLauncherId": "0x" + ("c" * 64)}

    def _fake_wallet_factory(_config):
        return _FakeWallet()

    monkeypatch.setattr(
        mod,
        "_resolve_cat_asset_id_for_coin_ids",
        lambda **_kwargs: ("d" * 64, {"ok": True, "asset_id": "d" * 64}),
    )
    monkeypatch.setattr(
        mod,
        "_build_broadcast_diagnostics",
        lambda **_kwargs: {"diag": "present"},
    )

    def _broadcast(_payload: dict[str, Any]) -> dict[str, Any]:
        return {"status": "skipped", "reason": "rejected"}

    exit_code, payload = mod.run(
        args,
        coinset_factory=_FakeCoinset,
        cloud_wallet_factory=_fake_wallet_factory,
        sign_and_broadcast_fn=_broadcast,
        kms_pubkey_resolver=lambda *_args: "02" + ("1" * 64),
        kms_signer=lambda *_args: "f" * 128,
    )
    assert exit_code == 1
    assert payload["status"] == "error"
    assert payload["broadcast_diagnostics"] == {"diag": "present"}
