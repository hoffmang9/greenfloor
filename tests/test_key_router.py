from greenfloor.config.models import MarketConfig, MarketInventoryConfig, SignerKeyConfig
from greenfloor.keys.router import resolve_market_key


def _market(key_id: str) -> MarketConfig:
    return MarketConfig(
        market_id="m1",
        enabled=True,
        base_asset="asset",
        base_symbol="TICK",
        quote_asset="xch",
        quote_asset_type="unstable",
        receive_address="xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        mode="sell_only",
        signer_key_id=key_id,
        inventory=MarketInventoryConfig(
            low_watermark_base_units=10, current_available_base_units=10
        ),
    )


def test_resolve_market_key_ok() -> None:
    got = resolve_market_key(_market("key-a"), {"key-a", "key-b"})
    assert got.key_id == "key-a"


def test_resolve_market_key_rejects_unknown() -> None:
    try:
        resolve_market_key(_market("key-z"), {"key-a"})
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert "not allowed" in str(exc)


def test_resolve_market_key_requires_registry_entry_when_registry_present() -> None:
    try:
        resolve_market_key(
            _market("key-z"),
            signer_key_registry={
                "key-a": SignerKeyConfig(key_id="key-a", fingerprint=111),
            },
            required_network="mainnet",
        )
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert "not present in signer key registry" in str(exc)


def test_resolve_market_key_returns_fingerprint_from_registry() -> None:
    got = resolve_market_key(
        _market("key-a"),
        signer_key_registry={
            "key-a": SignerKeyConfig(
                key_id="key-a",
                fingerprint=222,
                network="mainnet",
                keyring_yaml_path="~/.chia_keys/keyring.yaml",
            )
        },
        required_network="mainnet",
    )
    assert got.key_id == "key-a"
    assert got.fingerprint == 222
