"""Tests for runtime offer-action build helpers."""

from __future__ import annotations

from dataclasses import replace
from pathlib import Path

from greenfloor.runtime.offer_action_build import resolve_action_assets_for_build_context
from greenfloor.runtime.offer_build_context import prepare_offer_build_context
from tests.helpers.offer_runtime_fixtures import (
    market_config_for_local_offer,
    program_config_for_local_offer,
)


def test_resolve_action_assets_uses_engine_normalization_for_canonical_assets(monkeypatch) -> None:
    market = replace(
        market_config_for_local_offer(),
        base_asset="AA" * 32,
        quote_asset="XCH",
        pricing={"fixed_quote_per_base": 0.5, "strategy_offer_expiry_minutes": 12},
    )
    build_ctx = prepare_offer_build_context(
        program=program_config_for_local_offer(),
        market=market,
        program_path=Path("/tmp/program.yaml"),
        network="mainnet",
        keyring_yaml_path="/tmp/keyring.yaml",
    )

    def _fail_prepare(_program):
        raise AssertionError("canonical asset normalization should not prepare signer runtime")

    def _fail_resolve(_path: str, _base: str, _quote: str) -> dict[str, str]:
        raise AssertionError("canonical asset normalization should not call Coinset resolution")

    monkeypatch.setattr(
        "greenfloor.runtime.offer_action_build.rust_signer.try_normalize_offer_asset_ids",
        lambda _base, _quote: {"base_asset_id": "aa" * 32, "quote_asset_id": "xch"},
    )
    monkeypatch.setattr(
        "greenfloor.runtime.offer_action_build.rust_signer.resolve_offer_asset_ids",
        _fail_resolve,
    )
    monkeypatch.setattr(
        "greenfloor.runtime.offer_action_build.prepare_signer_runtime",
        _fail_prepare,
    )

    base, quote = resolve_action_assets_for_build_context(build_ctx)

    assert base == "aa" * 32
    assert quote == "xch"


def test_resolve_action_assets_uses_engine_for_ticker_symbols(monkeypatch) -> None:
    market = replace(
        market_config_for_local_offer(),
        base_asset="HOA",
        pricing={"fixed_quote_per_base": 0.5, "strategy_offer_expiry_minutes": 12},
    )
    build_ctx = prepare_offer_build_context(
        program=program_config_for_local_offer(),
        market=market,
        program_path=Path("/tmp/program.yaml"),
        network="mainnet",
        keyring_yaml_path="/tmp/keyring.yaml",
    )
    captured: dict[str, str] = {}

    def _fake_resolve(_path: str, base: str, quote: str) -> dict[str, str]:
        captured["base"] = base
        captured["quote"] = quote
        return {"base_asset_id": "aa" * 32, "quote_asset_id": "xch"}

    monkeypatch.setattr(
        "greenfloor.runtime.offer_action_build.rust_signer.try_normalize_offer_asset_ids",
        lambda _base, _quote: None,
    )
    monkeypatch.setattr(
        "greenfloor.runtime.offer_action_build.rust_signer.resolve_offer_asset_ids",
        _fake_resolve,
    )
    monkeypatch.setattr(
        "greenfloor.runtime.offer_action_build.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )

    base, quote = resolve_action_assets_for_build_context(build_ctx)

    assert captured["base"] == "HOA"
    assert base == "aa" * 32
    assert quote == "xch"
