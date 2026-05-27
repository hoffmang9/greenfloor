from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any, cast

import pytest
import yaml

import greenfloor.cli.manager as manager_mod
from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.cli.manager import (
    _coin_combine,
    _coin_split,
    _coins_list,
)
from greenfloor.runtime.cloud_wallet.assets import (
    resolve_cloud_wallet_asset_id,
)
from tests.helpers.offer_runtime_fixtures import (
    write_markets,
    write_markets_with_ladder,
    write_program,
    write_program_with_cloud_wallet,
)

def test_coins_list_returns_minimal_fields(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    write_program_with_cloud_wallet(program)

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = asset_id, include_pending
            return [
                {
                    "id": "coin-1",
                    "name": "coin-1",
                    "amount": 123,
                    "state": "PENDING",
                    "asset": {"id": "xch"},
                }
            ]

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    code = _coins_list(program_path=program, asset=None, vault_id=None)
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["count"] == 1
    assert payload["items"][0]["coin_id"] == "coin-1"
    assert payload["items"][0]["pending"] is True
    assert payload["items"][0]["spendable"] is False


def test_coins_list_resolves_asset_filter_before_listing(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    write_program_with_cloud_wallet(program)

    calls = {"list_asset_id": None}

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = include_pending
            calls["list_asset_id"] = asset_id
            return []

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_resolved",
    )
    code = _coins_list(program_path=program, asset="BYC", vault_id=None)
    assert code == 0
    assert calls["list_asset_id"] == "Asset_resolved"
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["count"] == 0


def test_coins_list_keeps_asset_scoped_rows_when_wallet_reports_mixed_asset_metadata(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    write_program_with_cloud_wallet(program)
    warning_calls: list[tuple[object, ...]] = []

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = asset_id, include_pending
            return [
                {
                    "name": "coin-byc-1",
                    "amount": 10,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_kg8byr1jz72w12g9tjchiypp"},
                },
                {
                    "name": "coin-xch-1",
                    "amount": 10000,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_huun64oh7dbt9f1f9ie8khuw"},
                },
            ]

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._manager_logger.warning",
        lambda *args, **kwargs: warning_calls.append(args),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_kg8byr1jz72w12g9tjchiypp",
    )
    code = _coins_list(program_path=program, asset="BYC", vault_id=None)
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["count"] == 2
    assert {item["coin_id"] for item in payload["items"]} == {"coin-byc-1", "coin-xch-1"}
    assert {item["asset"] for item in payload["items"]} == {"Asset_kg8byr1jz72w12g9tjchiypp"}
    assert {item["reported_asset"] for item in payload["items"]} == {
        "Asset_kg8byr1jz72w12g9tjchiypp",
        "Asset_huun64oh7dbt9f1f9ie8khuw",
    }
    assert {item["scoped_asset"] for item in payload["items"]} == {"Asset_kg8byr1jz72w12g9tjchiypp"}
    assert payload["asset_total_amount"] is None
    assert payload["asset_spendable_amount"] is None
    assert payload["asset_locked_amount"] is None
    assert payload["asset_totals_withheld_reason"] == "mixed_reported_asset_ids_detected"
    assert payload["warnings"][0]["code"] == "mixed_reported_asset_ids_detected"
    assert warning_calls
    assert "coins_list_mixed_asset_metadata" in str(warning_calls[0][0])


def test_coins_list_keeps_asset_totals_when_scoped_rows_omit_reported_asset(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    write_program_with_cloud_wallet(program)
    warning_calls: list[tuple[object, ...]] = []

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = asset_id, include_pending
            return [
                {
                    "name": "coin-byc-1",
                    "amount": 20000,
                    "state": "SETTLED",
                },
                {
                    "name": "coin-byc-2",
                    "amount": 30000,
                    "state": "SETTLED",
                },
            ]

        @staticmethod
        def _graphql(*, query, variables):
            _ = query, variables
            return {
                "wallet": {
                    "assets": {
                        "edges": [
                            {
                                "node": {
                                    "assetId": "Asset_kg8byr1jz72w12g9tjchiypp",
                                    "totalAmount": 50000,
                                    "spendableAmount": 50000,
                                    "lockedAmount": 0,
                                }
                            }
                        ]
                    }
                }
            }

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._manager_logger.warning",
        lambda *args, **kwargs: warning_calls.append(args),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_kg8byr1jz72w12g9tjchiypp",
    )
    code = _coins_list(program_path=program, asset="BYC", vault_id=None)
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["count"] == 2
    assert {item["asset"] for item in payload["items"]} == {"Asset_kg8byr1jz72w12g9tjchiypp"}
    assert {item["reported_asset"] for item in payload["items"]} == {None}
    assert {item["scoped_asset"] for item in payload["items"]} == {"Asset_kg8byr1jz72w12g9tjchiypp"}
    assert payload["asset_total_amount"] == 50000
    assert payload["asset_spendable_amount"] == 50000
    assert payload["asset_locked_amount"] == 0
    assert payload["asset_totals_withheld_reason"] is None
    assert payload["warnings"] == []
    assert warning_calls == []


def test_coins_list_keeps_row_level_spendability_separate_from_wallet_asset_totals(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    write_program_with_cloud_wallet(program)

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = asset_id, include_pending
            return [
                {
                    "name": "coin-a",
                    "amount": 10000,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_kg8"},
                },
                {
                    "name": "coin-b",
                    "amount": 10000,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_kg8"},
                },
                {
                    "name": "coin-c",
                    "amount": 480,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_kg8"},
                },
            ]

        @staticmethod
        def _graphql(*, query, variables):
            _ = query, variables
            return {
                "wallet": {
                    "assets": {
                        "edges": [
                            {
                                "node": {
                                    "assetId": "Asset_kg8",
                                    "totalAmount": 20480,
                                    "spendableAmount": 10480,
                                    "lockedAmount": 10000,
                                }
                            }
                        ]
                    }
                }
            }

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_kg8",
    )
    code = _coins_list(program_path=program, asset="BYC", vault_id=None)
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["asset_total_amount"] == 20480
    assert payload["asset_spendable_amount"] == 10480
    assert payload["asset_locked_amount"] == 10000
    assert payload["asset_totals_withheld_reason"] is None
    assert payload["warnings"] == []
    spendable_total = sum(int(item["amount"]) for item in payload["items"] if item["spendable"])
    assert spendable_total == 20480


def test_coins_list_warns_when_item_amount_sum_differs_from_wallet_asset_total(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    write_program_with_cloud_wallet(program)
    warning_calls: list[tuple[object, ...]] = []

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = asset_id, include_pending
            return [
                {
                    "name": "coin-a",
                    "amount": 10000,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_kg8"},
                },
                {
                    "name": "coin-b",
                    "amount": 10000,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_kg8"},
                },
            ]

        @staticmethod
        def _graphql(*, query, variables):
            _ = query, variables
            return {
                "wallet": {
                    "assets": {
                        "edges": [
                            {
                                "node": {
                                    "assetId": "Asset_kg8",
                                    "totalAmount": 20480,
                                    "spendableAmount": 20480,
                                    "lockedAmount": 0,
                                }
                            }
                        ]
                    }
                }
            }

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._manager_logger.warning",
        lambda *args, **kwargs: warning_calls.append(args),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_kg8",
    )
    code = _coins_list(program_path=program, asset="BYC", vault_id=None)
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["item_amount_sum"] == 20000
    assert payload["asset_total_amount"] == 20480
    assert payload["warnings"] == [
        {
            "code": "item_amount_sum_mismatch",
            "message": "sum(items.amount) does not match wallet asset total amount",
            "resolved_asset_id": "Asset_kg8",
            "items_amount_sum": 20000,
            "wallet_asset_total_amount": 20480,
            "difference_amount": -480,
        }
    ]
    assert warning_calls
    assert "coins_list_amount_mismatch" in str(warning_calls[0][0])


def test_coins_list_cat_id_uses_wallet_resolution_without_dexie(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    write_program_with_cloud_wallet(program)

    calls = {"list_asset_id": None}
    cat_id = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = include_pending
            calls["list_asset_id"] = asset_id
            return []

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    resolver_calls: list[dict[str, object]] = []
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kwargs: resolver_calls.append(kwargs) or "Asset_resolved",
    )
    code = _coins_list(program_path=program, asset="BYC", vault_id=None, cat_id=cat_id)
    assert code == 0
    assert calls["list_asset_id"] == "Asset_resolved"
    assert len(resolver_calls) == 1
    assert resolver_calls[0]["canonical_asset_id"] == cat_id
    assert resolver_calls[0]["allow_dexie_lookup"] is False
    assert str(resolver_calls[0].get("program_home_dir", "")).endswith(".greenfloor")
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["count"] == 0


def test_coins_list_rejects_non_hex_cat_id(monkeypatch, tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    write_program_with_cloud_wallet(program)

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = asset_id, include_pending
            return []

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)

    with pytest.raises(ValueError, match="--cat-id must be a 64-character hex CAT asset id"):
        _coins_list(program_path=program, asset=None, vault_id=None, cat_id="not-a-cat-id")


def test_coins_list_cat_id_works_when_dexie_metadata_absent(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    write_program_with_cloud_wallet(program)
    cat_id = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"
    calls = {"list_asset_id": None}

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        @staticmethod
        def _graphql(*, query: str, variables: dict):
            _ = query, variables
            return {
                "wallet": {
                    "assets": {
                        "edges": [
                            {
                                "node": {
                                    "assetId": "Asset_carbon",
                                    "type": "CAT2",
                                    "displayName": "ECO.181.2022",
                                    "symbol": "",
                                }
                            }
                        ]
                    }
                }
            }

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = include_pending
            calls["list_asset_id"] = asset_id
            return []

    monkeypatch.setattr(
        "greenfloor.cli.manager._new_cloud_wallet_adapter", lambda _program: _FakeWallet()
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._local_catalog_label_hints_for_asset_id",
        lambda *, canonical_asset_id: ["ECO.181.2022"] if canonical_asset_id == cat_id else [],
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._local_catalog_label_hints_for_asset_id",
        lambda *, canonical_asset_id: ["ECO.181.2022"] if canonical_asset_id == cat_id else [],
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._dexie_lookup_token_for_cat_id",
        lambda **kwargs: (_ for _ in ()).throw(AssertionError("dexie should not be called")),
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_cat_id",
        lambda **kwargs: (_ for _ in ()).throw(AssertionError("dexie should not be called")),
    )
    code = _coins_list(program_path=program, asset=None, vault_id=None, cat_id=cat_id)
    assert code == 0
    assert calls["list_asset_id"] == "Asset_carbon"
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["vault_id"] == "wallet-1"
    assert payload["count"] == 0


def test_coins_list_vault_id_override_uses_override_wallet_end_to_end(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    write_program_with_cloud_wallet(program)
    resolver_wallet_ids: list[str] = []
    override_init: dict[str, str] = {}
    list_calls = {"asset_id": None}

    class _BaseWallet:
        vault_id = "Wallet_original"
        network = "mainnet"

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = asset_id, include_pending
            raise AssertionError(
                "base wallet list_coins should not be called when vault_id override is set"
            )

    class _OverrideWallet:
        def __init__(self, config):
            override_init["vault_id"] = config.vault_id
            override_init["base_url"] = config.base_url
            self.vault_id = config.vault_id
            self.network = config.network

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = include_pending
            list_calls["asset_id"] = asset_id
            return [
                {
                    "name": "coin-override-1",
                    "amount": 77,
                    "state": "SETTLED",
                    "asset": {"id": "Asset_resolved"},
                }
            ]

    monkeypatch.setattr(
        "greenfloor.cli.manager._new_cloud_wallet_adapter", lambda _program: _BaseWallet()
    )
    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _OverrideWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: (resolver_wallet_ids.append(kw["wallet"].vault_id) or "Asset_resolved"),
    )

    code = _coins_list(
        program_path=program,
        asset="BYC",
        vault_id="Wallet_override",
        cat_id=None,
    )
    assert code == 0
    assert override_init["vault_id"] == "Wallet_override"
    assert resolver_wallet_ids == ["Wallet_override"]
    assert list_calls["asset_id"] == "Asset_resolved"
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["vault_id"] == "Wallet_override"
    assert payload["count"] == 1
    assert payload["items"][0]["coin_id"] == "coin-override-1"


def test_resolve_cloud_wallet_asset_id_hex_without_dexie_uses_local_catalog_hints(
    monkeypatch,
) -> None:
    base_cat = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        @staticmethod
        def _graphql(*, query: str, variables: dict):
            _ = query, variables
            return {
                "wallet": {
                    "assets": {
                        "edges": [
                            {
                                "node": {
                                    "assetId": "Asset_carbon",
                                    "type": "CAT2",
                                    "displayName": "ECO.181.2022",
                                    "symbol": "",
                                }
                            },
                            {
                                "node": {
                                    "assetId": "Asset_other",
                                    "type": "CAT2",
                                    "displayName": "Unrelated Token",
                                    "symbol": "",
                                }
                            },
                        ]
                    }
                }
            }

    monkeypatch.setattr(
        "greenfloor.cli.manager._local_catalog_label_hints_for_asset_id",
        lambda *, canonical_asset_id: ["ECO.181.2022"] if canonical_asset_id == base_cat else [],
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._local_catalog_label_hints_for_asset_id",
        lambda *, canonical_asset_id: ["ECO.181.2022"] if canonical_asset_id == base_cat else [],
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._dexie_lookup_token_for_cat_id",
        lambda **kwargs: (_ for _ in ()).throw(AssertionError("dexie should not be called")),
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_cat_id",
        lambda **kwargs: (_ for _ in ()).throw(AssertionError("dexie should not be called")),
    )

    resolved = resolve_cloud_wallet_asset_id(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        canonical_asset_id=base_cat,
        symbol_hint=None,
        allow_dexie_lookup=False,
    )
    assert resolved == "Asset_carbon"


def test_resolve_taker_or_coin_operation_fee_uses_coinset_value(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [5, 15]}

        @staticmethod
        def get_conservative_fee_estimate():
            return 15

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    fee, source = manager_mod._resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=0,
    )
    assert fee == 15
    assert source == "coinset_conservative"


def test_resolve_taker_or_coin_operation_fee_applies_minimum_floor(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [2]}

        @staticmethod
        def get_conservative_fee_estimate():
            return 2

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    fee, source = manager_mod._resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=5,
    )
    assert fee == 5
    assert source == "coinset_conservative_minimum_floor"


def test_resolve_taker_or_coin_operation_fee_falls_back_to_config_minimum(monkeypatch) -> None:
    class _FakeCoinset:
        _calls = 0

        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [0]}

        @classmethod
        def get_conservative_fee_estimate(cls):
            cls._calls += 1
            if cls._calls == 1:
                return 1
            return None

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    monkeypatch.setattr("time.sleep", lambda _seconds: None)

    fee, source = manager_mod._resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=0,
    )
    assert fee == 0
    assert source == "config_minimum_fee_fallback"


def test_resolve_taker_or_coin_operation_fee_fails_on_endpoint_preflight(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            raise RuntimeError("coinset_network_error:timed_out")

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    try:
        manager_mod._resolve_taker_or_coin_operation_fee(network="mainnet", minimum_fee_mojos=0)
    except manager_mod._CoinsetFeeLookupPreflightError as exc:
        assert exc.failure_kind == "endpoint_validation_failed"
        assert "coinset_network_error" in exc.detail
    else:
        raise AssertionError("expected _CoinsetFeeLookupPreflightError")


def test_resolve_taker_or_coin_operation_fee_fails_on_temporary_advice_unavailable(
    monkeypatch,
) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": False, "error": "backend_overloaded"}

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    try:
        manager_mod._resolve_taker_or_coin_operation_fee(network="mainnet", minimum_fee_mojos=0)
    except manager_mod._CoinsetFeeLookupPreflightError as exc:
        assert exc.failure_kind == "temporary_fee_advice_unavailable"
        assert "backend_overloaded" in exc.detail
    else:
        raise AssertionError("expected _CoinsetFeeLookupPreflightError")


def test_effective_coin_split_fee_for_cat_keeps_default_fee() -> None:
    fee, source = manager_mod._effective_coin_split_fee_for_asset(
        canonical_asset_id="a1",
        resolved_asset_id="Asset_cat_a1",
        fee_mojos=42,
        fee_source="coinset_conservative",
    )
    assert fee == 42
    assert source == "coinset_conservative"


def test_effective_coin_split_fee_for_xch_keeps_default_fee() -> None:
    fee, source = manager_mod._effective_coin_split_fee_for_asset(
        canonical_asset_id="xch",
        resolved_asset_id="Asset_xch",
        fee_mojos=42,
        fee_source="coinset_conservative",
    )
    assert fee == 42
    assert source == "coinset_conservative"


def test_resolve_maker_offer_fee_is_zero() -> None:
    from greenfloor.runtime.coinset_runtime import resolve_maker_offer_fee

    fee, source = resolve_maker_offer_fee(network="mainnet")
    assert fee == 0
    assert source == "maker_default_zero"


def test_coin_split_no_wait_uses_advised_fee(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-1"],
        amount_per_coin=10,
        number_of_coins=2,
        no_wait=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_abc123"], 10, 2, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] is None
    assert payload["waited"] is False
    assert payload["fee_mojos"] == 42
    assert payload["fee_source"] == "coinset_conservative"
    assert payload["coin_selection_mode"] == "explicit"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_split_auto_selects_largest_spendable_asset_coin(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    calls: dict[str, tuple[list[str], int, int, int] | None] = {"split": None}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [
                    {"id": "Coin_small", "name": "small", "amount": 100, "state": "SETTLED"},
                    {"id": "Coin_big", "name": "big", "amount": 1500, "state": "SETTLED"},
                    {"id": "Coin_reserve", "name": "reserve", "amount": 1100, "state": "SETTLED"},
                    {"id": "Coin_pending", "name": "pending", "amount": 999, "state": "PENDING"},
                ]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-auto", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_big"], 10, 10, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_split_guardrail_blocks_when_it_would_lock_all_spendable_coins(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    split_called = [False]

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [{"id": "Coin_only", "name": "only", "amount": 1500, "state": "SETTLED"}]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            split_called[0] = True
            return {"signature_request_id": "sr-guard", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
    )
    assert code == 2
    assert split_called[0] is False
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["error"] == "coin_split_guardrail_would_lock_all_spendable_coins"
    assert payload["spendable_asset_coin_count"] == 1
    assert payload["selected_spendable_coin_count"] == 1


def test_coin_split_guardrail_override_allows_lock_all_spendable(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    calls: dict[str, tuple[list[str], int, int, int] | None] = {"split": None}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [{"id": "Coin_only", "name": "only", "amount": 1500, "state": "SETTLED"}]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-override", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
        allow_lock_all_spendable=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_only"], 10, 10, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_split_guardrail_prompt_override_allows_continue(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    calls: dict[str, tuple[list[str], int, int, int] | None] = {"split": None}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [{"id": "Coin_only", "name": "only", "amount": 1500, "state": "SETTLED"}]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-prompt", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    monkeypatch.setattr("builtins.input", lambda _prompt: "y")

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
        prompt_for_override=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_only"], 10, 10, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_combine_no_wait_uses_advised_fee(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-2", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_huun64oh7dbt9f1f9ie8khuw",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (77, "coinset_conservative"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=3,
        asset_id="xch",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 0
    assert calls["combine"] == (3, 77, True, "Asset_huun64oh7dbt9f1f9ie8khuw", None)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] is None
    assert payload["waited"] is False
    assert payload["fee_mojos"] == 77
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["asset_id"] == "xch"
    assert payload["resolved_asset_id"] == "Asset_huun64oh7dbt9f1f9ie8khuw"


def test_coin_split_returns_structured_error_when_fee_resolution_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            RuntimeError("coinset_unavailable")
        ),
    )
    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-1"],
        amount_per_coin=10,
        number_of_coins=2,
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"].startswith("fee_resolution_failed:")
    assert "coin_ops.minimum_fee_mojos" in payload["operator_guidance"]


def test_coin_combine_returns_structured_error_when_fee_resolution_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return []

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            RuntimeError("coinset_unavailable")
        ),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=3,
        asset_id="xch",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"].startswith("fee_resolution_failed:")
    assert "coin_ops.minimum_fee_mojos" in payload["operator_guidance"]


def test_coin_split_distinguishes_coinset_endpoint_preflight_failure(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            manager_mod._CoinsetFeeLookupPreflightError(
                failure_kind="endpoint_validation_failed",
                detail="coinset_network_error:timed_out",
                diagnostics={
                    "coinset_network": "mainnet",
                    "coinset_base_url": "https://coinset.org",
                },
            )
        ),
    )
    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-1"],
        amount_per_coin=10,
        number_of_coins=2,
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coinset_fee_preflight_failed:endpoint_validation_failed"
    assert payload["coinset_fee_lookup"]["failure_kind"] == "endpoint_validation_failed"
    assert "endpoint routing" in payload["operator_guidance"]


def test_coin_combine_distinguishes_temporary_fee_advice_unavailability(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            manager_mod._CoinsetFeeLookupPreflightError(
                failure_kind="temporary_fee_advice_unavailable",
                detail="backend_overloaded",
                diagnostics={
                    "coinset_network": "mainnet",
                    "coinset_base_url": "https://coinset.org",
                },
            )
        ),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=3,
        asset_id="xch",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coinset_fee_preflight_failed:temporary_fee_advice_unavailable"
    assert payload["coinset_fee_lookup"]["failure_kind"] == "temporary_fee_advice_unavailable"
    assert "temporarily unavailable" in payload["operator_guidance"]


def test_coin_split_returns_structured_error_when_coin_id_not_found(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_known", "name": "known-coin-name"}]

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["missing-coin-name"],
        amount_per_coin=10,
        number_of_coins=2,
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coin_id_resolution_failed"
    assert payload["unknown_coin_ids"] == ["missing-coin-name"]


def test_coin_combine_with_coin_ids_resolves_to_global_ids(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [
                {"id": "Coin_a", "name": "coin-a"},
                {"id": "Coin_b", "name": "coin-b"},
            ]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-2", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (7, "coinset_conservative"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=2,
        asset_id="xch",
        coin_ids=["coin-a", "coin-b"],
        no_wait=True,
    )
    assert code == 0
    assert calls["combine"] == (2, 7, True, "xch", ["Coin_a", "Coin_b"])
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["waited"] is False


def test_coin_combine_returns_structured_error_when_coin_id_not_found(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_known", "name": "known-coin-name"}]

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=1 + 1,
        asset_id="xch",
        coin_ids=["missing-coin-name", "known-coin-name"],
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coin_id_resolution_failed"
    assert payload["unknown_coin_ids"] == ["missing-coin-name"]


def test_coin_combine_rejects_mixed_asset_coin_ids_before_api_call(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [
                {"id": "Coin_xch", "name": "coin-xch", "asset": {"id": "xch"}},
                {"id": "Coin_cat", "name": "coin-cat", "asset": {"id": "Asset_cat"}},
            ]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            _ = number_of_coins, fee, largest_first, asset_id, input_coin_ids
            raise AssertionError("combine_coins should not be called for mixed assets")

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=2,
        asset_id="xch",
        coin_ids=["coin-xch", "coin-cat"],
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coin_id_asset_mismatch"
    assert payload["resolved_asset_id"] == "xch"
    assert payload["mismatched_coin_ids"] == ["Coin_cat"]
    assert payload["mismatched_coin_assets"] == [
        {"coin_id": "Coin_cat", "coin_asset_id": "asset_cat"}
    ]


def test_coin_split_uses_market_ladder_target_when_size_is_provided(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program, provider="splash")
    write_markets_with_ladder(markets)
    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-1"],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=True,
        venue="splash",
        size_base_units=10,
    )
    assert code == 0
    assert calls["split"] == (["Coin_abc123"], 10, 4, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] == "splash"
    assert payload["denomination_target"]["required_count"] == 4


def test_coin_combine_uses_market_ladder_threshold_when_size_is_provided(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program, provider="splash")
    write_markets_with_ladder(markets)
    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "a1":
                return [
                    {"id": f"Coin_{i}", "name": f"coin-{i}", "amount": 1500 + i, "state": "SETTLED"}
                    for i in range(6)
                ] + [
                    {"id": "Coin_dust_1", "name": "dust-1", "amount": 100, "state": "SETTLED"},
                    {"id": "Coin_dust_2", "name": "dust-2", "amount": 999, "state": "SETTLED"},
                ]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-2", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (77, "coinset_conservative"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=0,
        asset_id=None,
        coin_ids=[],
        no_wait=True,
        venue="splash",
        size_base_units=10,
    )
    assert code == 0
    assert calls["combine"] == (
        6,
        77,
        True,
        "a1",
        ["Coin_5", "Coin_4", "Coin_3", "Coin_2", "Coin_1", "Coin_0"],
    )
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] == "splash"
    assert payload["denomination_target"]["combine_threshold_count"] == 6


def test_coin_combine_ladder_threshold_uses_ceil(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program, provider="dexie")
    markets.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                '    base_asset: "a1"',
                '    base_symbol: "A1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "k1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "    pricing:",
                "      min_price_quote_per_base: 0.0031",
                "      max_price_quote_per_base: 0.0038",
                "    ladders:",
                "      sell:",
                "        - size_base_units: 10",
                "          target_count: 3",
                "          split_buffer_count: 1",
                "          combine_when_excess_factor: 1.5",
            ]
        ),
        encoding="utf-8",
    )
    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "a1":
                return [
                    {"id": f"Coin_{i}", "name": f"coin-{i}", "amount": 2000 + i, "state": "SETTLED"}
                    for i in range(5)
                ]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-2", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (77, "coinset_conservative"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=0,
        asset_id=None,
        coin_ids=[],
        no_wait=True,
        size_base_units=10,
    )
    assert code == 0
    assert calls["combine"][0] == 5


def test_coin_combine_auto_selection_ignores_cat_dust_under_one_unit(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)
    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [
                    {"id": "Coin_big_1", "name": "big-1", "amount": 2000, "state": "SETTLED"},
                    {"id": "Coin_big_2", "name": "big-2", "amount": 1500, "state": "SETTLED"},
                    {"id": "Coin_dust_1", "name": "dust-1", "amount": 999, "state": "SETTLED"},
                    {"id": "Coin_dust_2", "name": "dust-2", "amount": 100, "state": "SETTLED"},
                ]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-combine", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (77, "coinset_conservative"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=2,
        asset_id="a1",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 0
    assert calls["combine"] == (
        2,
        77,
        True,
        "Asset_split_base",
        ["Coin_big_1", "Coin_big_2"],
    )
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_combine_auto_selection_directly_filters_cross_asset_scoped_rows(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)
    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [
                    {"id": "Coin_good_1", "name": "good-1", "amount": 1000, "state": "SETTLED"},
                    {"id": "Coin_bad", "name": "bad", "amount": 1000, "state": "SETTLED"},
                    {"id": "Coin_good_2", "name": "good-2", "amount": 1000, "state": "SETTLED"},
                ]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def get_coin_record(*, coin_id):
            mapping = {
                "Coin_good_1": {
                    "id": "Coin_good_1",
                    "amount": 1000,
                    "state": "SETTLED",
                    "isLocked": False,
                    "isLinkedToOpenOffer": False,
                    "asset": {"id": "Asset_split_base"},
                },
                "Coin_good_2": {
                    "id": "Coin_good_2",
                    "amount": 1000,
                    "state": "SETTLED",
                    "isLocked": False,
                    "isLinkedToOpenOffer": False,
                    "asset": {"id": "Asset_split_base"},
                },
                "Coin_bad": {
                    "id": "Coin_bad",
                    "amount": 1000,
                    "state": "SETTLED",
                    "isLocked": False,
                    "isLinkedToOpenOffer": False,
                    "asset": {"id": "Asset_huun64oh7dbt9f1f9ie8khuw"},
                },
            }
            return mapping[coin_id]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-combine", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **kw: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (77, "coinset_conservative"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=2,
        asset_id="a1",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 0
    assert calls["combine"] == (
        2,
        77,
        True,
        "Asset_split_base",
        ["Coin_good_1", "Coin_good_2"],
    )
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_selection_mode"] == "adapter_auto_select"


def test_coin_split_until_ready_ignores_unknown_states_and_string_asset(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program, provider="dexie")
    write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"
        _calls = 0

        def __init__(self, _config):
            pass

        @classmethod
        def list_coins(cls, *, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            cls._calls += 1
            return [
                {"id": "Coin_a", "name": "coin-a", "amount": 10, "state": "LOCKED", "asset": "a1"},
                {
                    "id": "Coin_b",
                    "name": "coin-b",
                    "amount": 10,
                    "state": "MYSTERY",
                    "asset": {"id": "a1"},
                },
            ]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            return {"signature_request_id": "sr-1", "status": "SIGNED"}

        @staticmethod
        def get_signature_request(signature_request_id):
            _ = signature_request_id
            return {"status": "SIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.wait_for_mempool_then_confirmation",
        lambda **kwargs: [],
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=False,
        size_base_units=10,
        until_ready=True,
        max_iterations=1,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["error"] == "no_spendable_split_coin_available"
    assert payload["resolved_asset_id"] == "a1"


def test_coin_split_until_ready_requires_size_base_units(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program)
    write_markets(markets)
    try:
        _coin_split(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id="m1",
            pair=None,
            coin_ids=[],
            amount_per_coin=10,
            number_of_coins=2,
            no_wait=False,
            until_ready=True,
            size_base_units=None,
        )
    except ValueError as exc:
        assert "--size-base-units" in str(exc)
    else:
        raise AssertionError("expected ValueError")


def test_coin_split_until_ready_disallows_no_wait(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program)
    write_markets_with_ladder(markets)
    try:
        _coin_split(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id="m1",
            pair=None,
            coin_ids=[],
            amount_per_coin=10,
            number_of_coins=4,
            no_wait=True,
            until_ready=True,
            size_base_units=10,
        )
    except ValueError as exc:
        assert "requires wait mode" in str(exc)
    else:
        raise AssertionError("expected ValueError")


def test_coin_split_until_ready_reports_not_ready(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program, provider="dexie")
    write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"
        _calls = 0

        def __init__(self, _config):
            pass

        @classmethod
        def list_coins(cls, *, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            cls._calls += 1
            # Never reaches target 4 coins of size 10.
            return [
                {
                    "id": "Coin_a",
                    "name": "coin-a",
                    "amount": 10,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                },
                {
                    "id": "Coin_b",
                    "name": "coin-b",
                    "amount": 9,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                },
            ]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            return {"signature_request_id": "sr-1", "status": "SIGNED"}

        @staticmethod
        def get_signature_request(signature_request_id):
            _ = signature_request_id
            return {"status": "SIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SIGNED", []),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.wait_for_mempool_then_confirmation",
        lambda **kwargs: [],
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-a"],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=False,
        venue="dexie",
        size_base_units=10,
        until_ready=True,
        max_iterations=2,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["until_ready"] is True
    assert payload["stop_reason"] == "requires_new_coin_selection"
    assert payload["denomination_readiness"]["ready"] is False
    assert len(payload["operations"]) == 1

def test_coin_split_until_ready_succeeds_when_denominations_met(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program, provider="dexie")
    write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            # 4 confirmed coins of size 10 + one larger reserve coin for asset a1.
            rows = [
                {
                    "id": f"Coin_{i}",
                    "name": f"coin-{i}",
                    "amount": 10,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
                for i in range(4)
            ]
            rows.append(
                {
                    "id": "Coin_reserve",
                    "name": "coin-reserve",
                    "amount": 20,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
            )
            return rows

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            return {"signature_request_id": "sr-ok", "status": "SIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SIGNED", []),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.wait_for_mempool_then_confirmation",
        lambda **kwargs: [],
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=False,
        size_base_units=10,
        until_ready=True,
        max_iterations=3,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["stop_reason"] == "ready"
    assert payload["denomination_readiness"]["ready"] is True
    assert payload["split_gate"]["reserve_ready"] is True


def test_coin_split_gate_ready_skips_split_in_non_interactive_mode(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program, provider="dexie")
    write_markets_with_ladder(markets)

    split_called = [False]

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [
                {
                    "id": f"Coin_{i}",
                    "name": f"coin-{i}",
                    "amount": 10,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
                for i in range(4)
            ] + [
                {
                    "id": "Coin_reserve",
                    "name": "coin-reserve",
                    "amount": 50,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
            ]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            split_called[0] = True
            return {"signature_request_id": "sr-should-not-run", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=True,
        size_base_units=10,
        until_ready=False,
        max_iterations=1,
        prompt_for_override=False,
    )
    assert code == 0
    assert split_called[0] is False
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["stop_reason"] == "ready"
    assert payload["split_gate"]["ready"] is True


# ---------------------------------------------------------------------------
# coin-combine until_ready requires_new_coin_selection path
# ---------------------------------------------------------------------------


def test_coin_combine_until_ready_with_coin_ids_stops_with_requires_new_coin_selection(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program, provider="dexie")
    write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            # 8 coins > combine_threshold=6 → not ready after combine
            return [
                {
                    "id": f"Coin_{i}",
                    "name": f"coin-{i}",
                    "amount": 10,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
                for i in range(8)
            ]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            return {"signature_request_id": "sr-combine", "status": "SIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SIGNED", []),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.wait_for_mempool_then_confirmation",
        lambda **kwargs: [],
    )

    # Provide explicit coin IDs so loop cannot auto-select new candidates
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=6,  # matches combine_threshold and len(coin_ids)
        asset_id="a1",
        coin_ids=[f"coin-{i}" for i in range(6)],
        no_wait=False,
        size_base_units=10,
        until_ready=True,
        max_iterations=3,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["stop_reason"] == "requires_new_coin_selection"
    assert payload["denomination_readiness"]["ready"] is False


