from __future__ import annotations

import json
from pathlib import Path

import pytest

from greenfloor.cli.coin_ops import coins_list
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program,
    write_manager_program_with_signer,
    write_markets,
)
from tests.helpers.signer_coin_op_cli_fixtures import (
    patch_signer_coins_list_backend,
    write_manager_markets_home,
)


def test_coins_list_returns_minimal_fields(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_manager_markets_home(tmp_path, write_markets)

    class _FakeWallet:
        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = asset_id, include_pending
            return [
                {
                    "id": "coin-1",
                    "name": "coin-1",
                    "amount": 123,
                    "state": "PENDING",
                }
            ]

    patch_signer_coins_list_backend(monkeypatch, wallet_factory=_FakeWallet)
    code = coins_list(program_path=program, asset=None, vault_id=None)
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_count"] == 1
    assert payload["coins"][0]["coin_id"] == "coin-1"
    assert payload["coins"][0]["pending"] is True
    assert payload["coins"][0]["spendable"] is False
    assert payload["execution_backend"] == "signer"


def test_coins_list_resolves_asset_filter_before_listing(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_manager_markets_home(tmp_path, write_markets)
    calls = {"asset_id": None}

    class _FakeWallet:
        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = include_pending
            calls["asset_id"] = asset_id
            return []

    patch_signer_coins_list_backend(
        monkeypatch,
        wallet_factory=_FakeWallet,
        resolved_asset_id="Asset_resolved",
    )
    code = coins_list(program_path=program, asset="BYC", vault_id=None)
    assert code == 0
    assert calls["asset_id"] == "Asset_resolved"
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_count"] == 0
    assert payload["resolved_asset_id"] == "Asset_resolved"


def test_coins_list_requires_signer_backend(tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    with pytest.raises(ValueError, match="offer execution requires signer"):
        coins_list(program_path=program, asset=None, vault_id=None)


def test_coins_list_cat_id_uses_signer_resolution(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_manager_markets_home(tmp_path, write_markets)
    cat_id = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"
    resolver_calls: list[dict[str, object]] = []

    class _FakeWallet:
        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = include_pending, asset_id
            return []

    patch_signer_coins_list_backend(monkeypatch, wallet_factory=_FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.coin_ops_list.resolve_signer_asset_id",
        lambda *_args, **kwargs: resolver_calls.append(dict(kwargs)) or "Asset_resolved",
    )
    code = coins_list(program_path=program, asset="BYC", vault_id=None, cat_id=cat_id)
    assert code == 0
    assert len(resolver_calls) == 1
    assert resolver_calls[0]["canonical_asset_id"] == cat_id
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_count"] == 0


def test_coins_list_rejects_invalid_cat_id_resolution(monkeypatch, tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_manager_markets_home(tmp_path, write_markets)

    class _FakeWallet:
        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = asset_id, include_pending
            return []

    patch_signer_coins_list_backend(monkeypatch, wallet_factory=_FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.coin_ops_list.resolve_signer_asset_id",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(
            ValueError("asset_resolution_failed:not-a-cat-id")
        ),
    )

    with pytest.raises(ValueError, match="asset_resolution_failed:not-a-cat-id"):
        coins_list(program_path=program, asset=None, vault_id=None, cat_id="not-a-cat-id")
