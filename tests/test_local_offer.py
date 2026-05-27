from __future__ import annotations

from dataclasses import replace
from pathlib import Path

from greenfloor.runtime.local_offer import build_local_offer_payload
from greenfloor.runtime.offer_build_context import prepare_offer_build_context
from tests.helpers.offer_runtime_fixtures import (
    market_config_for_local_offer,
    program_config_for_local_offer,
)


def test_build_local_offer_payload_includes_expiry() -> None:
    market = replace(
        market_config_for_local_offer(),
        pricing={"fixed_quote_per_base": 0.5, "strategy_offer_expiry_minutes": 12},
    )
    build_ctx = prepare_offer_build_context(
        program=program_config_for_local_offer(),
        market=market,
        program_path=Path("/tmp/program.yaml"),
        network="mainnet",
        keyring_yaml_path="/tmp/keyring.yaml",
    )
    payload = build_local_offer_payload(build_ctx, size_base_units=10, quote_price=0.5)
    assert payload["expiry_unit"] == "minutes"
    assert payload["expiry_value"] == 12
    assert payload["quote_price_quote_per_base"] == 0.5
