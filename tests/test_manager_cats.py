from __future__ import annotations

from pathlib import Path

import pytest

from tests.helpers.manager_cli import parse_json_output, run_manager


def _cats_list(cats_path: Path) -> dict:
    code, stdout, _stderr = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-list",
        ]
    )
    assert code == 0
    return parse_json_output(stdout)


def test_cats_add_manual_without_dexie_lookup(tmp_path: Path) -> None:
    cats_path = tmp_path / "cats.yaml"
    code, stdout, _stderr = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "--name",
            "Manual CAT",
            "--base-symbol",
            "MCAT",
            "--ticker-id",
            "manualcat_xch",
            "--pool-id",
            "pool-manual",
            "--last-price-xch",
            "0.42",
            "--target-usd-per-unit",
            "4.2",
            "--no-dexie-lookup",
        ]
    )
    assert code == 0
    payload = _cats_list(cats_path)
    rows = payload["cats"]
    assert len(rows) == 1
    row = rows[0]
    assert row["name"] == "Manual CAT"
    assert row["base_symbol"] == "MCAT"
    assert row["asset_id"] == ("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
    assert row["dexie"]["ticker_id"] == "manualcat_xch"
    assert row["dexie"]["pool_id"] == "pool-manual"
    assert row["dexie"]["last_price_xch"] == 0.42
    assert row["target_usd_per_unit"] == 4.2
    assert parse_json_output(stdout)["added"] is True


@pytest.mark.skip(reason="requires Dexie HTTP mocking unavailable via native subprocess")
def test_cats_add_uses_dexie_lookup_when_available(tmp_path: Path) -> None:
    from tests.helpers.dexie_http_mock import DexieHttpMock

    cats_path = tmp_path / "cats.yaml"
    cat_id = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"
    dexie = DexieHttpMock()
    dexie.set_swap_tokens(
        [
            {
                "assetId": cat_id,
                "code": "wUSDC.b",
                "name": "Wrapped USDC",
            }
        ]
    )
    dexie.set_price_tickers(
        [
            {
                "ticker_id": f"{cat_id}_xch",
                "base_currency": cat_id,
                "target_currency": "xch",
            }
        ]
    )
    dexie.start()
    try:
        code, stdout, _stderr = run_manager(
            [
                "--cats-config",
                str(cats_path),
                "--dexie-base-url",
                dexie.base_url,
                "cats-add",
                "--network",
                "mainnet",
                "--cat-id",
                cat_id,
            ]
        )
        assert code == 0
        payload = parse_json_output(stdout)
        assert payload["added"] is True
        assert payload["asset_id"] == cat_id
        row = _cats_list(cats_path)["cats"][0]
        assert row["base_symbol"] == "wUSDC.b"
    finally:
        dexie.stop()


def test_cats_add_replace_required_for_existing_asset(tmp_path: Path) -> None:
    cats_path = tmp_path / "cats.yaml"
    cat_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    first_code, _, _ = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--name",
            "First Name",
            "--base-symbol",
            "CAT1",
            "--no-dexie-lookup",
        ]
    )
    assert first_code == 0
    second_code, _, _ = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--name",
            "Updated Name",
            "--base-symbol",
            "CAT1",
            "--no-dexie-lookup",
        ]
    )
    assert second_code == 2
    third_code, _, _ = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--name",
            "Updated Name",
            "--base-symbol",
            "CAT1",
            "--no-dexie-lookup",
            "--replace",
        ]
    )
    assert third_code == 0
    assert _cats_list(cats_path)["cats"][0]["name"] == "Updated Name"


def test_cats_delete_by_cat_id(tmp_path: Path) -> None:
    cats_path = tmp_path / "cats.yaml"
    cat_id = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    added_code, _, _ = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--name",
            "Delete Me",
            "--base-symbol",
            "DEL",
            "--no-dexie-lookup",
        ]
    )
    assert added_code == 0
    deleted_code, _, _ = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-delete",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--yes",
        ]
    )
    assert deleted_code == 0
    assert _cats_list(cats_path)["cats"] == []


def test_cats_delete_by_ticker_uses_local_catalog_match(tmp_path: Path) -> None:
    cats_path = tmp_path / "cats.yaml"
    cat_id = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
    added_code, _, _ = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--name",
            "Catalog Cat",
            "--base-symbol",
            "CATX",
            "--no-dexie-lookup",
        ]
    )
    assert added_code == 0
    deleted_code, stdout, _stderr = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-delete",
            "--network",
            "mainnet",
            "--ticker",
            "CATX",
            "--no-dexie-lookup",
            "--yes",
        ]
    )
    assert deleted_code == 0
    payload = parse_json_output(stdout)
    assert payload["deleted"] is True
    assert _cats_list(cats_path)["cats"] == []


def test_cats_delete_requires_confirmation_when_not_yes(tmp_path: Path) -> None:
    cats_path = tmp_path / "cats.yaml"
    cat_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
    added_code, _, _ = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--name",
            "Needs Confirm",
            "--base-symbol",
            "CNF",
            "--no-dexie-lookup",
        ]
    )
    assert added_code == 0
    deleted_code, stdout, _stderr = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-delete",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
        ]
    )
    assert deleted_code == 2
    payload = parse_json_output(stdout)
    assert payload["error"] == "confirmation_required"
    assert _cats_list(cats_path)["cats"]


def test_cats_delete_preflight_only_does_not_delete(tmp_path: Path) -> None:
    cats_path = tmp_path / "cats.yaml"
    cat_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
    added_code, _, _ = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--name",
            "Preflight Only",
            "--base-symbol",
            "PFL",
            "--no-dexie-lookup",
        ]
    )
    assert added_code == 0
    deleted_code, _, _ = run_manager(
        [
            "--cats-config",
            str(cats_path),
            "cats-delete",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--preflight-only",
        ]
    )
    assert deleted_code == 0
    assert len(_cats_list(cats_path)["cats"]) == 1
