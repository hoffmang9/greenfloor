from __future__ import annotations

import json
from pathlib import Path
from typing import cast

import pytest

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.cli.manager import (
    _coins_list,
)
from greenfloor.runtime.cloud_wallet.assets import (
    resolve_cloud_wallet_asset_id,
)
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program_with_cloud_wallet,
)


def test_coins_list_returns_minimal_fields(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)

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
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)

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
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
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
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
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
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)

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
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
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
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)

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
    assert resolver_calls[0]["program_home_dir"] == str(tmp_path)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["count"] == 0


def test_coins_list_rejects_non_hex_cat_id(monkeypatch, tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)

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
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
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
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
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
