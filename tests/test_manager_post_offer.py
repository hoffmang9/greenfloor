from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any, cast

import pytest
import yaml

import greenfloor.cli.manager as manager_mod
from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.cli.manager import (
    _build_and_post_offer,
    _offers_cancel,
    _resolve_dexie_base_url,
    _resolve_offer_publish_settings,
    _resolve_splash_base_url,
)
from greenfloor.runtime.cloud_wallet.assets import (
    recent_market_resolved_asset_id_hints,
    resolve_cloud_wallet_asset_id,
    resolve_cloud_wallet_offer_asset_ids,
)


def test_resolve_dexie_base_url_by_network() -> None:
    assert _resolve_dexie_base_url("mainnet", None) == "https://api.dexie.space"
    assert _resolve_dexie_base_url("testnet11", None) == "https://api-testnet.dexie.space"
    assert _resolve_dexie_base_url("testnet", None) == "https://api-testnet.dexie.space"


def test_resolve_splash_base_url_defaults_when_not_explicit() -> None:
    assert _resolve_splash_base_url(None) == "http://john-deere.hoffmang.com:4000"


def test_dexie_lookup_token_for_cat_id_falls_back_to_v3_tickers(monkeypatch) -> None:
    target = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"
    calls: list[str] = []

    class _Resp:
        def __init__(self, payload: object):
            self._payload = payload

        def read(self) -> bytes:
            return json.dumps(self._payload).encode("utf-8")

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb):
            _ = exc_type, exc, tb
            return False

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        url = req.full_url if hasattr(req, "full_url") else str(req)
        calls.append(url)
        if url.endswith("/v1/swap/tokens"):
            return _Resp({"tokens": [{"id": "fa4a...a99d", "code": "wUSDC.b"}]})
        if url.endswith("/v3/prices/tickers"):
            return _Resp({"tickers": [{"ticker_id": f"{target}_xch", "base_currency": target}]})
        raise AssertionError(f"unexpected url: {url}")

    monkeypatch.setattr("greenfloor.cli.manager.urllib.request.urlopen", _fake_urlopen)
    row = manager_mod._dexie_lookup_token_for_cat_id(
        canonical_cat_id_hex=target,
        network="mainnet",
    )
    assert row is not None
    assert str(row.get("ticker_id", "")).startswith(target)
    assert any(url.endswith("/v1/swap/tokens") for url in calls)
    assert any(url.endswith("/v3/prices/tickers") for url in calls)


def test_resolve_cloud_wallet_offer_asset_ids_maps_distinct_cat_assets(monkeypatch) -> None:
    base_cat = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"
    quote_cat = "fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d"

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
                                    "assetId": "Asset_wusdc",
                                    "type": "CAT2",
                                    "displayName": "Base Warped USDC",
                                    "symbol": "",
                                }
                            },
                        ]
                    }
                }
            }

    def _fake_lookup_by_cat(*, canonical_cat_id_hex: str, network: str):
        _ = network
        if canonical_cat_id_hex == base_cat:
            return {"ticker_id": f"{base_cat}_xch", "base_code": "ECO.181.2022"}
        if canonical_cat_id_hex == quote_cat:
            return {"id": quote_cat, "code": "wUSDC.b", "name": "Base warp.green USDC"}
        return None

    monkeypatch.setattr(
        "greenfloor.cli.manager._dexie_lookup_token_for_cat_id", _fake_lookup_by_cat
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_cat_id",
        _fake_lookup_by_cat,
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._dexie_lookup_token_for_symbol",
        lambda *, asset_ref, network: (
            {"id": quote_cat, "code": "wUSDC.b"} if asset_ref == "wUSDC.b" else None
        ),
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_symbol",
        lambda *, asset_ref, network: (
            {"id": quote_cat, "code": "wUSDC.b"} if asset_ref == "wUSDC.b" else None
        ),
    )

    base_asset, quote_asset = resolve_cloud_wallet_offer_asset_ids(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        base_asset_id=base_cat,
        quote_asset_id="wUSDC.b",
        base_symbol_hint="ECO.181.2022",
        quote_symbol_hint="wUSDC.b",
    )
    assert base_asset == "Asset_carbon"
    assert quote_asset == "Asset_wusdc"
    assert base_asset != quote_asset


def test_resolve_cloud_wallet_asset_id_uses_local_catalog_hints_when_dexie_missing(
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

    monkeypatch.setattr("greenfloor.cli.manager._dexie_lookup_token_for_cat_id", lambda **_: None)
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_cat_id",
        lambda **_: None,
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._local_catalog_label_hints_for_asset_id",
        lambda *, canonical_asset_id: ["ECO.181.2022"] if canonical_asset_id == base_cat else [],
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._local_catalog_label_hints_for_asset_id",
        lambda *, canonical_asset_id: ["ECO.181.2022"] if canonical_asset_id == base_cat else [],
    )

    resolved = resolve_cloud_wallet_asset_id(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        canonical_asset_id=base_cat,
        symbol_hint=base_cat,
    )
    assert resolved == "Asset_carbon"


def test_resolve_cloud_wallet_offer_asset_ids_uses_global_hints_without_label_match(
    monkeypatch,
) -> None:
    base_cat = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"
    quote_cat = "fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d"

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
                                    "displayName": "Legacy Carbon Label",
                                    "symbol": "",
                                }
                            },
                            {
                                "node": {
                                    "assetId": "Asset_wusdc",
                                    "type": "CAT2",
                                    "displayName": "USD Coin",
                                    "symbol": "",
                                }
                            },
                        ]
                    }
                }
            }

    monkeypatch.setattr("greenfloor.cli.manager._dexie_lookup_token_for_cat_id", lambda **_: None)
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_cat_id",
        lambda **_: None,
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._local_catalog_label_hints_for_asset_id", lambda **_: []
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._local_catalog_label_hints_for_asset_id",
        lambda **_: [],
    )

    resolved_base, resolved_quote = resolve_cloud_wallet_offer_asset_ids(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        base_asset_id=base_cat,
        quote_asset_id=quote_cat,
        base_symbol_hint="ECO.181.2022",
        quote_symbol_hint="wUSDC.b",
        base_global_id_hint="Asset_carbon",
        quote_global_id_hint="Asset_wusdc",
    )
    assert resolved_base == "Asset_carbon"
    assert resolved_quote == "Asset_wusdc"


def test_resolve_cloud_wallet_asset_id_uses_identifier_lookup(monkeypatch) -> None:
    """When the Cloud Wallet asset(identifier:) query returns a match, use it directly."""
    cat_hex = "9720fcb8333984c72f914fc5090509ae9f7b1ff72eff2ed6825d944d7a571066"

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        @staticmethod
        def _graphql(*, query: str, variables: dict):
            if "resolveAssetByIdentifier" in query:
                return {
                    "asset": {
                        "id": "Asset_eco49",
                        "type": "CAT2",
                    }
                }
            return {
                "wallet": {
                    "assets": {
                        "edges": [
                            {
                                "node": {
                                    "assetId": "Asset_eco181",
                                    "type": "CAT2",
                                    "displayName": "Agricultural Reforestation 2022",
                                    "symbol": "",
                                }
                            },
                            {
                                "node": {
                                    "assetId": "Asset_eco49",
                                    "type": "CAT2",
                                    "displayName": "Antioquia and Caldas 2022",
                                    "symbol": "",
                                }
                            },
                        ]
                    }
                }
            }

    monkeypatch.setattr("greenfloor.cli.manager._dexie_lookup_token_for_cat_id", lambda **_: None)
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_cat_id",
        lambda **_: None,
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._local_catalog_label_hints_for_asset_id", lambda **_: []
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._local_catalog_label_hints_for_asset_id",
        lambda **_: [],
    )

    resolved = resolve_cloud_wallet_asset_id(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        canonical_asset_id=cat_hex,
        symbol_hint="ECO.49.2022",
        allow_dexie_lookup=False,
    )
    assert resolved == "Asset_eco49"


def test_resolve_cloud_wallet_asset_id_identifier_miss_falls_through(monkeypatch) -> None:
    """When asset(identifier:) returns null, fall through to label matching."""
    cat_hex = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        @staticmethod
        def _graphql(*, query: str, variables: dict):
            if "resolveAssetByIdentifier" in query:
                return {"asset": None}
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
                        ]
                    }
                }
            }

    monkeypatch.setattr(
        "greenfloor.cli.manager._dexie_lookup_token_for_cat_id",
        lambda *, canonical_cat_id_hex, network: (
            {"ticker_id": f"{cat_hex}_xch", "base_code": "ECO.181.2022"}
            if canonical_cat_id_hex == cat_hex
            else None
        ),
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_cat_id",
        lambda *, canonical_cat_id_hex, network: (
            {"ticker_id": f"{cat_hex}_xch", "base_code": "ECO.181.2022"}
            if canonical_cat_id_hex == cat_hex
            else None
        ),
    )

    resolved = resolve_cloud_wallet_asset_id(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        canonical_asset_id=cat_hex,
        symbol_hint="ECO.181.2022",
    )
    assert resolved == "Asset_carbon"


def test_resolve_cloud_wallet_asset_id_identifier_error_falls_through(monkeypatch) -> None:
    """When asset(identifier:) raises, fall through to label matching."""
    cat_hex = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        @staticmethod
        def _graphql(*, query: str, variables: dict):
            if "resolveAssetByIdentifier" in query:
                raise RuntimeError("network_error")
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
                        ]
                    }
                }
            }

    monkeypatch.setattr(
        "greenfloor.cli.manager._dexie_lookup_token_for_cat_id",
        lambda *, canonical_cat_id_hex, network: (
            {"ticker_id": f"{cat_hex}_xch", "base_code": "ECO.181.2022"}
            if canonical_cat_id_hex == cat_hex
            else None
        ),
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_cat_id",
        lambda *, canonical_cat_id_hex, network: (
            {"ticker_id": f"{cat_hex}_xch", "base_code": "ECO.181.2022"}
            if canonical_cat_id_hex == cat_hex
            else None
        ),
    )

    resolved = resolve_cloud_wallet_asset_id(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        canonical_asset_id=cat_hex,
        symbol_hint="ECO.181.2022",
    )
    assert resolved == "Asset_carbon"


def test_recent_market_resolved_asset_id_hints_reads_strategy_execution(tmp_path: Path) -> None:
    from greenfloor.storage.sqlite import SqliteStore

    home_dir = tmp_path / "home"
    db_path = home_dir / "db" / "greenfloor.sqlite"
    db_path.parent.mkdir(parents=True, exist_ok=True)
    store = SqliteStore(db_path)
    try:
        store.add_audit_event(
            "strategy_offer_execution",
            {
                "market_id": "m1",
                "resolved_base_asset_id": "Asset_base",
                "resolved_quote_asset_id": "Asset_quote",
            },
            market_id="m1",
        )
    finally:
        store.close()

    base_hint, quote_hint = recent_market_resolved_asset_id_hints(
        program_home_dir=str(home_dir),
        market_id="m1",
    )
    assert base_hint == "Asset_base"
    assert quote_hint == "Asset_quote"


def test_format_json_output_pretty_mode_has_indentation() -> None:
    original = manager_mod._JSON_OUTPUT_COMPACT
    try:
        manager_mod._JSON_OUTPUT_COMPACT = False
        output = manager_mod._format_json_output({"alpha": 1, "beta": {"gamma": 2}})
    finally:
        manager_mod._JSON_OUTPUT_COMPACT = original
    assert output.startswith("{\n")
    assert '\n  "alpha": 1' in output


def test_format_json_output_compact_mode_is_single_line() -> None:
    original = manager_mod._JSON_OUTPUT_COMPACT
    try:
        manager_mod._JSON_OUTPUT_COMPACT = True
        output = manager_mod._format_json_output({"alpha": 1, "beta": {"gamma": 2}})
    finally:
        manager_mod._JSON_OUTPUT_COMPACT = original
    assert output == '{"alpha":1,"beta":{"gamma":2}}'


def _write_program(path: Path, *, provider: str = "dexie", home_dir: str | None = None) -> None:
    home_yaml = "~/.greenfloor" if home_dir is None else str(home_dir).replace("\\", "/")
    path.write_text(
        "\n".join(
            [
                "app:",
                '  network: "mainnet"',
                f'  home_dir: "{home_yaml}"',
                "runtime:",
                "  loop_interval_seconds: 30",
                "cloud_wallet:",
                '  base_url: ""',
                '  user_key_id: ""',
                '  private_key_pem_path: ""',
                '  vault_id: ""',
                "chain_signals:",
                "  tx_block_trigger:",
                "    webhook_enabled: true",
                '    webhook_listen_addr: "127.0.0.1:8787"',
                "dev:",
                "  python:",
                '    min_version: "3.11"',
                "notifications:",
                "  low_inventory_alerts:",
                "    enabled: true",
                '    threshold_mode: "absolute_base_units"',
                "    default_threshold_base_units: 0",
                "    dedup_cooldown_seconds: 60",
                "    clear_hysteresis_percent: 10",
                "  providers:",
                "    - type: pushover",
                "      enabled: true",
                '      user_key_env: "PUSHOVER_USER_KEY"',
                '      app_token_env: "PUSHOVER_APP_TOKEN"',
                '      recipient_key_env: "PUSHOVER_RECIPIENT_KEY"',
                "venues:",
                "  dexie:",
                '    api_base: "https://api.dexie.space"',
                "  splash:",
                '    api_base: "http://localhost:4000"',
                "  offer_publish:",
                f'    provider: "{provider}"',
            ]
        ),
        encoding="utf-8",
    )


def test_resolve_offer_publish_settings_from_program(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    _write_program(program, provider="splash")
    venue, dexie_base, splash_base = _resolve_offer_publish_settings(
        program_path=program,
        network="mainnet",
        venue_override=None,
        dexie_base_url=None,
        splash_base_url=None,
    )
    assert venue == "splash"
    assert dexie_base == "https://api.dexie.space"
    assert splash_base == "http://localhost:4000"


def test_set_log_level_updates_program_yaml(tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    _write_program(program)
    code = manager_mod._set_log_level(program_path=program, log_level="warning")
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["updated"] is True
    assert payload["previous_log_level"] == "INFO"
    assert payload["log_level"] == "WARNING"
    assert "log_level: WARNING" in program.read_text(encoding="utf-8")


def test_main_dispatches_set_log_level_command(monkeypatch, tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    _write_program(program)
    captured: dict[str, object] = {}

    def _fake_set_log_level(*, program_path: Path, log_level: str) -> int:
        captured["program_path"] = program_path
        captured["log_level"] = log_level
        return 0

    monkeypatch.setattr("greenfloor.cli.manager._set_log_level", _fake_set_log_level)
    monkeypatch.setattr(
        "sys.argv",
        [
            "greenfloor-manager",
            "--program-config",
            str(program),
            "set-log-level",
            "--log-level",
            "ERROR",
        ],
    )
    with pytest.raises(SystemExit) as exc:
        manager_mod.main()
    assert exc.value.code == 0
    assert captured["program_path"] == program
    assert captured["log_level"] == "ERROR"


def test_main_dispatches_coin_status_command(monkeypatch, tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    _write_program(program)
    captured: dict[str, object] = {}

    def _fake_coin_status(
        *, program_path: Path, asset: str | None, vault_id: str | None, cat_id: str | None
    ) -> int:
        captured["program_path"] = program_path
        captured["asset"] = asset
        captured["vault_id"] = vault_id
        captured["cat_id"] = cat_id
        return 0

    monkeypatch.setattr("greenfloor.cli.manager._coin_status", _fake_coin_status)
    monkeypatch.setattr(
        "sys.argv",
        [
            "greenfloor-manager",
            "--program-config",
            str(program),
            "coin-status",
            "--asset",
            "BYC",
        ],
    )
    with pytest.raises(SystemExit) as exc:
        manager_mod.main()
    assert exc.value.code == 0
    assert captured["program_path"] == program
    assert captured["asset"] == "BYC"
    assert captured["vault_id"] is None
    assert captured["cat_id"] is None


def _write_markets(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                '    base_asset: "a1"',
                '    base_symbol: "A1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "k1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "    pricing:",
                "      min_price_quote_per_base: 0.0031",
                "      max_price_quote_per_base: 0.0038",
            ]
        ),
        encoding="utf-8",
    )


def _write_markets_with_ladder(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                '    base_asset: "a1"',
                '    base_symbol: "A1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "k1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "    pricing:",
                "      min_price_quote_per_base: 0.0031",
                "      max_price_quote_per_base: 0.0038",
                "    ladders:",
                "      sell:",
                "        - size_base_units: 10",
                "          target_count: 3",
                "          split_buffer_count: 1",
                "          combine_when_excess_factor: 2.0",
            ]
        ),
        encoding="utf-8",
    )


def _write_program_with_cloud_wallet(
    path: Path,
    *,
    provider: str = "dexie",
    with_kms: bool = False,
    home_dir: str | None = None,
) -> None:
    """Write a program.yaml with valid Cloud Wallet credentials populated."""
    _write_program(path, provider=provider, home_dir=home_dir)
    text = path.read_text(encoding="utf-8")
    text = text.replace('  base_url: ""', '  base_url: "https://wallet.example.com"')
    text = text.replace('  user_key_id: ""', '  user_key_id: "key-1"')
    text = text.replace('  private_key_pem_path: ""', '  private_key_pem_path: "/tmp/key.pem"')
    text = text.replace('  vault_id: ""', '  vault_id: "wallet-1"')
    if with_kms:
        text = text.replace(
            '  kms_key_id: ""', '  kms_key_id: "arn:aws:kms:us-west-2:123:key/demo"'
        )
        text = text.replace('  kms_region: ""', '  kms_region: "us-west-2"')
        text = text.replace('  kms_public_key_hex: ""', '  kms_public_key_hex: "02abc123"')
        if "signer:" not in text:
            text = text.replace(
                "coin_ops:",
                "\n".join(
                    [
                        "signer:",
                        '  kms_key_id: "arn:aws:kms:us-west-2:123:key/demo"',
                        '  kms_region: "us-west-2"',
                        '  kms_public_key_hex: "02abc123"',
                        "vault:",
                        '  launcher_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"',
                        "  custody_threshold: 1",
                        "  recovery_threshold: 1",
                        "  recovery_clawback_timelock: 3600",
                        "  custody_keys:",
                        '    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"',
                        "      curve: SECP256R1",
                        "  recovery_keys:",
                        '    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"',
                        "      curve: BLS12_381",
                        "",
                        "coin_ops:",
                    ]
                ),
            )
    path.write_text(text, encoding="utf-8")


def _write_markets_with_duplicate_pair(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                '    base_asset: "a1"',
                '    base_symbol: "A1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "k1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "    pricing:",
                "      min_price_quote_per_base: 0.0031",
                "      max_price_quote_per_base: 0.0038",
                "  - id: m2",
                "    enabled: true",
                '    base_asset: "a1"',
                '    base_symbol: "A1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "k1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "    pricing:",
                "      min_price_quote_per_base: 0.0031",
                "      max_price_quote_per_base: 0.0038",
            ]
        ),
        encoding="utf-8",
    )


def test_build_and_post_offer_defaults_to_mainnet(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)
    captured: dict = {}

    class _FakeDexie:
        def __init__(self, base_url: str) -> None:
            captured["base_url"] = base_url

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            captured["offer"] = offer
            captured["drop_only"] = drop_only
            captured["claim_rewards"] = claim_rewards
            return {"success": True, "id": "offer-123"}

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        lambda _payload: "offer1abc",
    )
    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.cli.manager.verify_offer_text_for_dexie",
        lambda _offer: None,
    )

    code = _build_and_post_offer(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 0
    assert captured["base_url"] == "https://api.dexie.space"
    assert captured["offer"] == "offer1abc"
    assert captured["drop_only"] is True
    assert captured["claim_rewards"] is False

    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["results"][0]["venue"] == "dexie"
    assert payload["results"][0]["result"]["id"] == "offer-123"
    assert (
        payload["results"][0]["result"]["offer_view_url"] == "https://dexie.space/offers/offer-123"
    )


def test_build_and_post_offer_uses_market_configured_expiry_override(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)
    raw = yaml.safe_load(markets.read_text(encoding="utf-8"))
    pricing = dict(raw["markets"][0].get("pricing") or {})
    pricing["strategy_offer_expiry_minutes"] = 12
    raw["markets"][0]["pricing"] = pricing
    markets.write_text(yaml.safe_dump(raw, sort_keys=False), encoding="utf-8")

    captured_payload: dict[str, object] = {}

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        @staticmethod
        def post_offer(offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = offer, drop_only, claim_rewards
            return {"success": True, "id": "offer-expiry-1"}

    def _fake_build(payload: dict) -> str:
        captured_payload.update(payload)
        return "offer1expiryoverride"

    monkeypatch.setattr("greenfloor.cli.manager._build_offer_text_for_request", _fake_build)
    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)
    monkeypatch.setattr("greenfloor.cli.manager.verify_offer_text_for_dexie", lambda _offer: None)

    code = _build_and_post_offer(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 0
    assert captured_payload["expiry_unit"] == "minutes"
    assert captured_payload["expiry_value"] == 12
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 0
    assert payload["results"][0]["result"]["id"] == "offer-expiry-1"


def test_build_and_post_offer_dry_run_builds_but_does_not_post(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)

    class _FailDexie:
        def __init__(self, _base_url: str) -> None:
            raise AssertionError("DexieAdapter should not be constructed in dry_run")

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        lambda _payload: "offer1dryrun",
    )
    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FailDexie)

    code = _build_and_post_offer(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=1,
        repeat=2,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=True,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["dry_run"] is True
    assert len(payload["built_offers_preview"]) == 2
    assert payload["results"] == []


def test_build_and_post_offer_dry_run_can_capture_full_offer_text(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)
    capture_dir = tmp_path / "offer-capture"

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        lambda _payload: "offer1captureme",
    )
    monkeypatch.setenv("GREENFLOOR_DEBUG_DRY_RUN_OFFER_CAPTURE_DIR", str(capture_dir))
    try:
        code = _build_and_post_offer(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id="m1",
            pair=None,
            size_base_units=1,
            repeat=1,
            publish_venue="dexie",
            dexie_base_url="https://api.dexie.space",
            splash_base_url="http://localhost:4000",
            drop_only=True,
            claim_rewards=False,
            dry_run=True,
        )
    finally:
        monkeypatch.delenv("GREENFLOOR_DEBUG_DRY_RUN_OFFER_CAPTURE_DIR", raising=False)

    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    capture_path = Path(payload["built_offers_preview"][0]["offer_capture_path"])
    assert capture_path.exists()
    assert capture_path.read_text(encoding="utf-8") == "offer1captureme"


def test_offers_cancel_cancel_open_uses_cloud_wallet(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    _write_program_with_cloud_wallet(program)

    cancelled: list[tuple[str, bool]] = []

    class _FakeWallet:
        vault_id = "wallet-1"

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {
                "offers": [
                    {
                        "id": "WalletOffer_1",
                        "offerId": "Offer_1",
                        "state": "OPEN",
                        "expiresAt": "2026-02-26T01:13:22.000Z",
                    },
                    {
                        "id": "WalletOffer_2",
                        "offerId": "Offer_2",
                        "state": "EXPIRED",
                        "expiresAt": "2026-02-25T21:13:22.000Z",
                    },
                ]
            }

        @staticmethod
        def cancel_offer(*, offer_id: str, cancel_off_chain: bool = False):
            cancelled.append((offer_id, cancel_off_chain))
            return {"signature_request_id": f"SignatureRequest_{offer_id}", "status": "SUBMITTED"}

    monkeypatch.setattr(
        "greenfloor.cli.manager._new_cloud_wallet_adapter", lambda _p: _FakeWallet()
    )

    code = _offers_cancel(
        program_path=program,
        offer_ids=[],
        cancel_open=True,
    )
    assert code == 0
    assert cancelled == [("Offer_1", False)]
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["selected_count"] == 1
    assert payload["cancelled_count"] == 1
    assert payload["items"][0]["offer_id"] == "Offer_1"
    assert (
        payload["items"][0]["url"]
        == "https://wallet.example.com/wallet/wallet-1/offers/WalletOffer_1"
    )


def test_offers_cancel_pending_offer_uses_off_chain_cancel(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    _write_program_with_cloud_wallet(program)

    cancelled: list[tuple[str, bool]] = []

    class _FakeWallet:
        vault_id = "wallet-1"

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {
                "offers": [
                    {
                        "id": "WalletOffer_pending",
                        "offerId": "Offer_pending",
                        "state": "PENDING",
                        "expiresAt": "2026-02-26T01:13:22.000Z",
                    }
                ]
            }

        @staticmethod
        def cancel_offer(*, offer_id: str, cancel_off_chain: bool = False):
            cancelled.append((offer_id, cancel_off_chain))
            # Off-chain cancel may not return a signature request.
            return {"signature_request_id": "", "status": ""}

    monkeypatch.setattr(
        "greenfloor.cli.manager._new_cloud_wallet_adapter", lambda _p: _FakeWallet()
    )

    code = _offers_cancel(
        program_path=program,
        offer_ids=["Offer_pending"],
        cancel_open=False,
    )
    assert code == 0
    assert cancelled == [("Offer_pending", True)]
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["selected_count"] == 1
    assert payload["cancelled_count"] == 1
    assert payload["failed_count"] == 0
    assert payload["items"][0]["cancel_off_chain"] is True
    assert payload["items"][0]["result"]["success"] is True
    assert payload["items"][0]["result"]["reason"] == "cancel_off_chain_requested"


def test_offers_cancel_can_submit_onchain_refresh_after_offchain(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    cancelled: list[tuple[str, bool]] = []
    split_calls: list[dict[str, Any]] = []

    class _Program:
        app_network = "mainnet"
        cloud_wallet_base_url = "https://wallet.example.com"
        signer_key_registry = {}
        home_dir = str(tmp_path / "gf_home")

    class _FakeWallet:
        vault_id = "wallet-1"

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {
                "offers": [
                    {
                        "id": "WalletOffer_pending",
                        "offerId": "Offer_pending",
                        "state": "PENDING",
                        "expiresAt": "2026-02-26T01:13:22.000Z",
                        "bech32": "offer1dummy",
                    }
                ]
            }

        @staticmethod
        def cancel_offer(*, offer_id: str, cancel_off_chain: bool = False):
            cancelled.append((offer_id, cancel_off_chain))
            return {"signature_request_id": "", "status": ""}

        @staticmethod
        def list_coins(*, asset_id: str | None = None, include_pending: bool = True):
            _ = asset_id, include_pending
            return [
                {
                    "id": "Coin_target",
                    "name": "ab" * 32,
                    "amount": 1000,
                    "state": "CONFIRMED",
                    "asset": {"id": "Asset_a1"},
                }
            ]

        @staticmethod
        def split_coins(
            *,
            coin_ids: list[str],
            amount_per_coin: int,
            number_of_coins: int,
            fee: int,
        ):
            split_calls.append(
                {
                    "coin_ids": coin_ids,
                    "amount_per_coin": amount_per_coin,
                    "number_of_coins": number_of_coins,
                    "fee": fee,
                }
            )
            return {"signature_request_id": "SignatureRequest_refresh", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.load_program_config", lambda _p: _Program())
    monkeypatch.setattr(
        "greenfloor.cli.manager._new_cloud_wallet_adapter", lambda _p: _FakeWallet()
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.resolve_cloud_wallet_asset_id",
        lambda **_kw: "Asset_a1",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda **_kw: (0, "coinset_conservative"),
    )

    code = _offers_cancel(
        program_path=program,
        offer_ids=["Offer_pending"],
        cancel_open=False,
        markets_path=markets,
        submit_onchain_after_offchain=True,
        onchain_market_id="m1",
    )
    assert code == 0
    assert cancelled == [("Offer_pending", True)]
    assert len(split_calls) == 1
    assert split_calls[0]["coin_ids"] == ["Coin_target"]
    assert split_calls[0]["amount_per_coin"] == 1000
    assert split_calls[0]["number_of_coins"] == 1
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["submit_onchain_after_offchain"] is True
    assert payload["onchain_market_id"] == "m1"
    assert payload["items"][0]["result"]["onchain_refresh"]["status"] == "executed"
    assert (
        payload["items"][0]["result"]["onchain_refresh"]["signature_request_id"]
        == "SignatureRequest_refresh"
    )


def test_offers_cancel_submit_onchain_requires_market_selection(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    class _Program:
        app_network = "mainnet"
        cloud_wallet_base_url = "https://wallet.example.com"
        signer_key_registry = {}

    monkeypatch.setattr("greenfloor.cli.manager.load_program_config", lambda _p: _Program())
    monkeypatch.setattr(
        "greenfloor.cli.manager._new_cloud_wallet_adapter",
        lambda _p: type(
            "_Wallet",
            (),
            {"vault_id": "wallet-1", "get_wallet": staticmethod(lambda: {"offers": []})},
        )(),
    )

    try:
        _offers_cancel(
            program_path=program,
            offer_ids=["Offer_pending"],
            cancel_open=False,
            markets_path=markets,
            submit_onchain_after_offchain=True,
            onchain_market_id=None,
            onchain_pair=None,
        )
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert str(exc) == "provide exactly one of --market-id or --pair"


def test_build_and_post_offer_resolves_market_by_pair(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = offer, drop_only, claim_rewards
            return {"success": True, "id": "offer-xyz"}

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        lambda _payload: "offer1pair",
    )
    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.cli.manager.verify_offer_text_for_dexie",
        lambda _offer: None,
    )

    code = _build_and_post_offer(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id=None,
        pair="A1:xch",
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["market_id"] == "m1"
    assert payload["results"][0]["venue"] == "dexie"
    assert payload["results"][0]["result"]["id"] == "offer-xyz"


def test_build_and_post_offer_accepts_txch_pair_on_testnet11(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = offer, drop_only, claim_rewards
            return {"success": True, "id": "offer-txch"}

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        lambda _payload: "offer1pair",
    )
    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.cli.manager.verify_offer_text_for_dexie",
        lambda _offer: None,
    )

    code = _build_and_post_offer(
        program_path=program,
        markets_path=markets,
        network="testnet11",
        market_id=None,
        pair="A1:txch",
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api-testnet.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["market_id"] == "m1"
    assert payload["results"][0]["result"]["id"] == "offer-txch"
    assert payload["results"][0]["result"]["offer_view_url"] == (
        "https://testnet.dexie.space/offers/offer-txch"
    )


def test_build_and_post_offer_rejects_txch_pair_on_mainnet(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)

    try:
        _build_and_post_offer(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id=None,
            pair="A1:txch",
            size_base_units=10,
            repeat=1,
            publish_venue="dexie",
            dexie_base_url="https://api.dexie.space",
            splash_base_url="http://localhost:4000",
            drop_only=True,
            claim_rewards=False,
            dry_run=False,
        )
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert "no enabled market found for pair" in str(exc)


def test_build_and_post_offer_pair_ambiguous_requires_market_id(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets_with_duplicate_pair(markets)
    try:
        _build_and_post_offer(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id=None,
            pair="a1:xch",
            size_base_units=10,
            repeat=1,
            publish_venue="dexie",
            dexie_base_url="https://api.dexie.space",
            splash_base_url="http://localhost:4000",
            drop_only=True,
            claim_rewards=False,
            dry_run=False,
        )
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert "ambiguous" in str(exc)


def test_build_and_post_offer_rejects_unknown_market(monkeypatch, tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)
    try:
        _build_and_post_offer(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id="missing",
            pair=None,
            size_base_units=10,
            repeat=1,
            publish_venue="dexie",
            dexie_base_url="https://api.dexie.space",
            splash_base_url="http://localhost:4000",
            drop_only=True,
            claim_rewards=False,
            dry_run=False,
        )
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert "market_id not found" in str(exc)


def test_build_and_post_offer_posts_to_splash_when_selected(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)

    class _FakeSplash:
        def __init__(self, base_url: str) -> None:
            self.base_url = base_url

        def post_offer(self, offer: str):
            _ = offer
            return {"success": True, "id": "splash-1"}

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        lambda _payload: "offer1pair",
    )
    monkeypatch.setattr("greenfloor.cli.manager.SplashAdapter", _FakeSplash)

    code = _build_and_post_offer(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=1,
        repeat=1,
        publish_venue="splash",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["results"][0]["venue"] == "splash"
    assert payload["results"][0]["result"]["id"] == "splash-1"


def test_build_and_post_offer_returns_nonzero_when_offer_verification_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        lambda _payload: "offer1bad",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.verify_offer_text_for_dexie",
        lambda _offer: "wallet_sdk_offer_verify_false",
    )

    code = _build_and_post_offer(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=1,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_attempts"] == 1
    assert payload["publish_failures"] == 1
    assert payload["results"][0]["result"]["success"] is False


def test_build_and_post_offer_blocks_publish_when_offer_has_no_expiry(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)
    called: dict[str, bool] = {"post_offer_called": False}

    class _ConditionNoExpiry:
        @staticmethod
        def parse_assert_before_seconds_relative():
            return None

        @staticmethod
        def parse_assert_before_seconds_absolute():
            return None

        @staticmethod
        def parse_assert_before_height_relative():
            return None

        @staticmethod
        def parse_assert_before_height_absolute():
            return None

    class _CoinSpendNoExpiry:
        @staticmethod
        def conditions():
            return [_ConditionNoExpiry()]

    class _SpendBundleNoExpiry:
        coin_spends = [_CoinSpendNoExpiry()]

    class _Sdk:
        @staticmethod
        def validate_offer(_offer: str) -> None:
            return None

        @staticmethod
        def decode_offer(_offer: str):
            return _SpendBundleNoExpiry()

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = offer, drop_only, claim_rewards
            called["post_offer_called"] = True
            return {"success": True, "id": "should-not-post"}

    def _import_module(name: str):
        if name == "greenfloor_native":
            raise ImportError("disable native path for this test")
        return __import__(name)

    monkeypatch.setattr(
        "greenfloor.runtime.offer_publish.importlib.import_module",
        _import_module,
    )
    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        lambda _payload: "offer1noexpiry",
    )
    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)

    code = _build_and_post_offer(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=1,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )

    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_attempts"] == 1
    assert payload["publish_failures"] == 1
    assert payload["results"][0]["result"]["success"] is False
    assert payload["results"][0]["result"]["error"] == "wallet_sdk_offer_missing_expiration"
    assert called["post_offer_called"] is False


def test_build_and_post_offer_returns_nonzero_when_publish_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = offer, drop_only, claim_rewards
            return {"success": False, "error": "dexie_http_error:500"}

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        lambda _payload: "offer1abc",
    )
    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.cli.manager.verify_offer_text_for_dexie",
        lambda _offer: None,
    )

    code = _build_and_post_offer(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=1,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_attempts"] == 1
    assert payload["publish_failures"] == 1
    assert payload["results"][0]["result"]["success"] is False


def test_build_and_post_offer_dry_run_returns_nonzero_when_build_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)

    def _raise_build_error(_payload):
        raise RuntimeError("signing_failed:no_agg_sig_targets_found")

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        _raise_build_error,
    )

    code = _build_and_post_offer(
        program_path=program,
        markets_path=markets,
        network="testnet11",
        market_id="m1",
        pair=None,
        size_base_units=1,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api-testnet.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_attempts"] == 1
    assert payload["publish_failures"] == 1
    assert payload["results"][0]["result"]["success"] is False
    assert payload["results"][0]["result"]["error"].startswith("offer_builder_failed:")


def test_build_offer_text_for_request_direct_call(monkeypatch) -> None:
    """Verify that _build_offer_text_for_request calls shared offer_builder.build_offer."""
    from greenfloor.cli import manager

    monkeypatch.setattr(
        "greenfloor.offer_builder.build_offer",
        lambda _payload: "offer1direct",
    )
    result = manager._build_offer_text_for_request({"test": True})
    assert result == "offer1direct"
