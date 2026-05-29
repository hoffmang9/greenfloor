"""Tests for runtime offer-action build helpers."""

from __future__ import annotations

from dataclasses import replace
from pathlib import Path

import pytest

from greenfloor.config.io import load_program_config
from greenfloor.config.models import invalidate_signer_runtime_cache
from greenfloor.core.engine_bridge import import_engine
from greenfloor.runtime.offer_action_build import resolve_action_assets_for_build_context
from greenfloor.runtime.offer_build_context import prepare_offer_build_context
from tests.helpers.msp_mock_server import write_signer_program_yaml
from tests.helpers.offer_runtime_fixtures import (
    market_config_for_local_offer,
    program_config_for_local_offer,
)

_CAT = "c" * 64


def _require_engine() -> None:
    try:
        import_engine()
    except ImportError as exc:
        pytest.skip(f"greenfloor_engine unavailable: {exc}")


def test_resolve_action_assets_normalizes_canonical_market_assets(tmp_path: Path) -> None:
    _require_engine()
    market = replace(
        market_config_for_local_offer(),
        base_asset=_CAT.upper(),
        quote_asset="XCH",
        pricing={"fixed_quote_per_base": 0.5, "strategy_offer_expiry_minutes": 12},
    )
    build_ctx = prepare_offer_build_context(
        program=program_config_for_local_offer(),
        market=market,
        program_path=tmp_path / "program.yaml",
        network="mainnet",
        keyring_yaml_path=str(tmp_path / "keyring.yaml"),
    )

    base, quote = resolve_action_assets_for_build_context(build_ctx)

    assert base == _CAT
    assert quote == "xch"


def test_resolve_action_assets_reaches_coinset_for_ticker_symbols(tmp_path: Path) -> None:
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
    market = replace(
        market_config_for_local_offer(),
        base_asset="HOA",
        pricing={"fixed_quote_per_base": 0.5, "strategy_offer_expiry_minutes": 12},
    )
    build_ctx = prepare_offer_build_context(
        program=program,
        market=market,
        program_path=program_path,
        network="mainnet",
        keyring_yaml_path=str(tmp_path / "keyring.yaml"),
    )

    with pytest.raises(ValueError, match="asset_resolution_failed:HOA"):
        resolve_action_assets_for_build_context(build_ctx)
