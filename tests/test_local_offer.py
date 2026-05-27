from __future__ import annotations

from pathlib import Path

from greenfloor.runtime.local_offer import LocalOfferBuildParams, build_local_offer_payload
from tests.helpers.offer_runtime_fixtures import market_config_for_local_offer, program_config_for_local_offer


def test_build_local_offer_payload_includes_expiry_and_cloud_wallet_fields() -> None:
    params = LocalOfferBuildParams(
        program=program_config_for_local_offer(),
        market=market_config_for_local_offer(),
        program_path=Path("/tmp/program.yaml"),
        network="mainnet",
        resolved_quote_asset="xch",
        expiry_unit="minutes",
        expiry_value=12,
        base_unit_mojo_multiplier=1000,
        quote_unit_mojo_multiplier=1,
        keyring_yaml_path="/tmp/keyring.yaml",
        dry_run=False,
    )
    payload = build_local_offer_payload(params, size_base_units=10, quote_price=0.5)
    assert payload["expiry_unit"] == "minutes"
    assert payload["expiry_value"] == 12
    assert payload["quote_price_quote_per_base"] == 0.5
    assert payload["cloud_wallet_base_url"] == "https://wallet.example"
