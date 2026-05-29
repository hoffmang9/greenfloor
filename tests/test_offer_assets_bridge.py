"""End-to-end tests for offer asset resolution bridge (real engine, no monkeypatch)."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

from greenfloor.config.io import load_program_config
from greenfloor.config.models import invalidate_signer_runtime_cache
from greenfloor.core.engine_bridge import import_engine
from greenfloor.core.offer_assets_bridge import (
    resolve_offer_assets,
    resolve_offer_assets_via_coinset_config_path,
    try_normalize_offer_asset_ids,
)
from tests.helpers.msp_mock_server import write_signer_program_yaml

_CAT_A = "a" * 64
_ENGINE_CRATE = Path(__file__).resolve().parents[1] / "greenfloor-engine"


def _require_engine():
    try:
        return import_engine()
    except ImportError as exc:
        pytest.skip(f"greenfloor_engine unavailable: {exc}")


def test_try_normalize_offer_asset_ids_accepts_canonical_pair() -> None:
    _require_engine()
    result = try_normalize_offer_asset_ids(_CAT_A.upper(), "XCH")
    assert result == (_CAT_A, "xch")


def test_try_normalize_offer_asset_ids_returns_none_for_ticker_symbols() -> None:
    _require_engine()
    assert try_normalize_offer_asset_ids("HOA", "xch") is None


def test_try_normalize_offer_asset_ids_raises_on_non_xch_collision() -> None:
    _require_engine()
    with pytest.raises(
        ValueError,
        match="resolved_assets_collide_for_non_xch_pair",
    ):
        try_normalize_offer_asset_ids(_CAT_A, _CAT_A)


def test_engine_try_normalize_matches_bridge_contract() -> None:
    engine = _require_engine()
    assert engine.try_normalize_offer_asset_ids(_CAT_A, "xch") == (_CAT_A, "xch")
    assert engine.try_normalize_offer_asset_ids("HOA", "xch") is None


def test_resolve_offer_assets_uses_normalize_without_coinset(tmp_path: Path) -> None:
    _require_engine()
    home = tmp_path / "home"
    home.mkdir()
    program_path = tmp_path / "program.yaml"
    write_signer_program_yaml(
        program_path,
        home_dir=str(home),
        msp_base_url="http://127.0.0.1:1/unreachable",
    )
    invalidate_signer_runtime_cache(home_dir=str(home))
    program = load_program_config(program_path)

    base, quote = resolve_offer_assets(_CAT_A, "xch", program=program)

    assert base == _CAT_A
    assert quote == "xch"


def test_resolve_offer_assets_reaches_coinset_for_ticker_symbols(tmp_path: Path) -> None:
    _require_engine()
    home = tmp_path / "home"
    home.mkdir()
    program_path = tmp_path / "program.yaml"
    write_signer_program_yaml(
        program_path,
        home_dir=str(home),
        msp_base_url="http://127.0.0.1:1/unreachable",
    )
    invalidate_signer_runtime_cache(home_dir=str(home))
    program = load_program_config(program_path)

    with pytest.raises(ValueError, match="asset_resolution_failed:HOA"):
        resolve_offer_assets("HOA", "xch", program=program)


def test_resolve_offer_assets_via_coinset_config_path_is_coinset_only(tmp_path: Path) -> None:
    _require_engine()
    home = tmp_path / "home"
    home.mkdir()
    program_path = tmp_path / "program.yaml"
    write_signer_program_yaml(
        program_path,
        home_dir=str(home),
        msp_base_url="http://127.0.0.1:1/unreachable",
    )

    with pytest.raises(ValueError, match="asset_resolution_failed:HOA"):
        resolve_offer_assets_via_coinset_config_path(str(program_path), "HOA", "xch")


def test_rust_coinset_resolution_for_ticker_symbols() -> None:
    """Successful MSP lookup runs in-engine (reqwest + in-process http.server deadlock in PyO3)."""
    result = subprocess.run(
        ["cargo", "test", "resolve_via_coinset_looks_up_ticker_symbols", "--", "--nocapture"],
        cwd=_ENGINE_CRATE,
        check=False,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert result.returncode == 0, result.stderr or result.stdout
