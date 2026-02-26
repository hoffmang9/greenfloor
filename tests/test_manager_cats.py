from __future__ import annotations

from pathlib import Path

from greenfloor.cli.manager import _cats_add, _cats_delete, _load_cats_catalog


def test_cats_add_manual_without_dexie_lookup(tmp_path: Path) -> None:
    cats_path = tmp_path / "cats.yaml"
    code = _cats_add(
        cats_path=cats_path,
        network="mainnet",
        cat_id="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ticker=None,
        name="Manual CAT",
        base_symbol="MCAT",
        ticker_id="manualcat_xch",
        pool_id="pool-manual",
        last_price_xch="0.42",
        target_usd_per_unit="4.2",
        use_dexie_lookup=False,
        replace=False,
    )
    assert code == 0
    payload = _load_cats_catalog(cats_path)
    rows = payload["cats"]
    assert len(rows) == 1
    row = rows[0]
    assert row["name"] == "Manual CAT"
    assert row["base_symbol"] == "MCAT"
    assert row["asset_id"] == ("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
    assert row["dexie"]["ticker_id"] == "manualcat_xch"
    assert row["dexie"]["pool_id"] == "pool-manual"
    assert row["dexie"]["last_price_xch"] == "0.42"
    assert row["target_usd_per_unit"] == 4.2


def test_cats_add_uses_dexie_lookup_when_available(tmp_path: Path, monkeypatch) -> None:
    cats_path = tmp_path / "cats.yaml"
    cat_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

    def _fake_lookup_by_ticker(*, asset_ref: str, network: str) -> dict:
        assert asset_ref == "TESTCAT"
        assert network == "mainnet"
        return {"id": cat_id, "code": "TCAT", "name": "Test CAT"}

    def _fake_lookup_by_id(*, canonical_cat_id_hex: str, network: str) -> dict:
        assert canonical_cat_id_hex == cat_id
        assert network == "mainnet"
        return {
            "id": cat_id,
            "code": "TCAT",
            "name": "Test CAT",
            "ticker_id": f"{cat_id}_xch",
            "pool_id": "pool-123",
            "last_price_xch": "1.23",
        }

    monkeypatch.setattr(
        "greenfloor.cli.manager._dexie_lookup_token_for_symbol",
        _fake_lookup_by_ticker,
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._dexie_lookup_token_for_cat_id",
        _fake_lookup_by_id,
    )
    code = _cats_add(
        cats_path=cats_path,
        network="mainnet",
        cat_id=None,
        ticker="TESTCAT",
        name=None,
        base_symbol=None,
        ticker_id=None,
        pool_id=None,
        last_price_xch=None,
        target_usd_per_unit=None,
        use_dexie_lookup=True,
        replace=False,
    )
    assert code == 0
    row = _load_cats_catalog(cats_path)["cats"][0]
    assert row["name"] == "Test CAT"
    assert row["base_symbol"] == "TCAT"
    assert row["asset_id"] == cat_id
    assert row["dexie"]["ticker_id"] == f"{cat_id}_xch"
    assert row["dexie"]["pool_id"] == "pool-123"


def test_cats_add_replace_required_for_existing_asset(tmp_path: Path) -> None:
    cats_path = tmp_path / "cats.yaml"
    cat_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    first = _cats_add(
        cats_path=cats_path,
        network="mainnet",
        cat_id=cat_id,
        ticker=None,
        name="First Name",
        base_symbol="CAT1",
        ticker_id=None,
        pool_id=None,
        last_price_xch=None,
        target_usd_per_unit=None,
        use_dexie_lookup=False,
        replace=False,
    )
    assert first == 0
    second = _cats_add(
        cats_path=cats_path,
        network="mainnet",
        cat_id=cat_id,
        ticker=None,
        name="Updated Name",
        base_symbol="CAT1",
        ticker_id=None,
        pool_id=None,
        last_price_xch=None,
        target_usd_per_unit=None,
        use_dexie_lookup=False,
        replace=False,
    )
    assert second == 2
    third = _cats_add(
        cats_path=cats_path,
        network="mainnet",
        cat_id=cat_id,
        ticker=None,
        name="Updated Name",
        base_symbol="CAT1",
        ticker_id=None,
        pool_id=None,
        last_price_xch=None,
        target_usd_per_unit=None,
        use_dexie_lookup=False,
        replace=True,
    )
    assert third == 0
    assert _load_cats_catalog(cats_path)["cats"][0]["name"] == "Updated Name"


def test_cats_delete_by_cat_id(tmp_path: Path) -> None:
    cats_path = tmp_path / "cats.yaml"
    cat_id = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    added = _cats_add(
        cats_path=cats_path,
        network="mainnet",
        cat_id=cat_id,
        ticker=None,
        name="Delete Me",
        base_symbol="DEL",
        ticker_id=None,
        pool_id=None,
        last_price_xch=None,
        target_usd_per_unit=None,
        use_dexie_lookup=False,
        replace=False,
    )
    assert added == 0
    deleted = _cats_delete(
        cats_path=cats_path,
        network="mainnet",
        cat_id=cat_id,
        ticker=None,
        use_dexie_lookup=False,
        confirm_delete=True,
        preflight_only=False,
    )
    assert deleted == 0
    assert _load_cats_catalog(cats_path)["cats"] == []


def test_cats_delete_by_ticker_uses_local_catalog_match(tmp_path: Path) -> None:
    cats_path = tmp_path / "cats.yaml"
    cat_id = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
    added = _cats_add(
        cats_path=cats_path,
        network="mainnet",
        cat_id=cat_id,
        ticker=None,
        name="Ticker Delete",
        base_symbol="TDEL",
        ticker_id=None,
        pool_id=None,
        last_price_xch=None,
        target_usd_per_unit=None,
        use_dexie_lookup=False,
        replace=False,
    )
    assert added == 0
    deleted = _cats_delete(
        cats_path=cats_path,
        network="mainnet",
        cat_id=None,
        ticker="TDEL",
        use_dexie_lookup=False,
        confirm_delete=True,
        preflight_only=False,
    )
    assert deleted == 0
    assert _load_cats_catalog(cats_path)["cats"] == []


def test_cats_delete_requires_confirmation_when_not_yes(tmp_path: Path, monkeypatch) -> None:
    cats_path = tmp_path / "cats.yaml"
    cat_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
    added = _cats_add(
        cats_path=cats_path,
        network="mainnet",
        cat_id=cat_id,
        ticker=None,
        name="Confirm Me",
        base_symbol="CONF",
        ticker_id=None,
        pool_id=None,
        last_price_xch=None,
        target_usd_per_unit=None,
        use_dexie_lookup=False,
        replace=False,
    )
    assert added == 0
    monkeypatch.setattr("greenfloor.cli.manager._prompt_yes_no", lambda *args, **kwargs: False)
    deleted = _cats_delete(
        cats_path=cats_path,
        network="mainnet",
        cat_id=cat_id,
        ticker=None,
        use_dexie_lookup=False,
        confirm_delete=False,
        preflight_only=False,
    )
    assert deleted == 2
    assert len(_load_cats_catalog(cats_path)["cats"]) == 1


def test_cats_delete_preflight_only_does_not_delete(tmp_path: Path) -> None:
    cats_path = tmp_path / "cats.yaml"
    cat_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
    added = _cats_add(
        cats_path=cats_path,
        network="mainnet",
        cat_id=cat_id,
        ticker=None,
        name="Preflight Only",
        base_symbol="PFL",
        ticker_id=None,
        pool_id=None,
        last_price_xch=None,
        target_usd_per_unit=None,
        use_dexie_lookup=False,
        replace=False,
    )
    assert added == 0
    deleted = _cats_delete(
        cats_path=cats_path,
        network="mainnet",
        cat_id=cat_id,
        ticker=None,
        use_dexie_lookup=False,
        confirm_delete=False,
        preflight_only=True,
    )
    assert deleted == 0
    assert len(_load_cats_catalog(cats_path)["cats"]) == 1
