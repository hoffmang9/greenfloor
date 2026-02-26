from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any, cast

import greenfloor.cli.manager as manager_mod
from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.cli.manager import (
    _build_and_post_offer,
    _coin_combine,
    _coin_split,
    _coins_list,
    _offers_cancel,
    _resolve_dexie_base_url,
    _resolve_offer_publish_settings,
    _resolve_splash_base_url,
    _verify_offer_text_for_dexie,
)
from tests.logging_helpers import reset_concurrent_log_handlers


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
                                    "displayName": "CARBON22",
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
        "greenfloor.cli.manager._dexie_lookup_token_for_symbol",
        lambda *, asset_ref, network: (
            {"id": quote_cat, "code": "wUSDC.b"} if asset_ref == "wUSDC.b" else None
        ),
    )

    base_asset, quote_asset = manager_mod._resolve_cloud_wallet_offer_asset_ids(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        base_asset_id=base_cat,
        quote_asset_id="wUSDC.b",
        base_symbol_hint="CARBON22",
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
                                    "displayName": "CARBON22",
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
        "greenfloor.cli.manager._local_catalog_label_hints_for_asset_id",
        lambda *, canonical_asset_id: ["CARBON22"] if canonical_asset_id == base_cat else [],
    )

    resolved = manager_mod._resolve_cloud_wallet_asset_id(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        canonical_asset_id=base_cat,
        symbol_hint=base_cat,
    )
    assert resolved == "Asset_carbon"


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


def _write_program(path: Path, *, provider: str = "dexie") -> None:
    path.write_text(
        "\n".join(
            [
                "app:",
                '  network: "mainnet"',
                '  home_dir: "~/.greenfloor"',
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
    import pytest

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


def _write_program_with_cloud_wallet(path: Path, *, provider: str = "dexie") -> None:
    """Write a program.yaml with valid Cloud Wallet credentials populated."""
    _write_program(path, provider=provider)
    text = path.read_text(encoding="utf-8")
    text = text.replace('  base_url: ""', '  base_url: "https://wallet.example.com"')
    text = text.replace('  user_key_id: ""', '  user_key_id: "key-1"')
    text = text.replace('  private_key_pem_path: ""', '  private_key_pem_path: "/tmp/key.pem"')
    text = text.replace('  vault_id: ""', '  vault_id: "wallet-1"')
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
        "greenfloor.cli.manager._verify_offer_text_for_dexie",
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
        def get_wallet():
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
        def get_wallet():
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

    class _FakeWallet:
        vault_id = "wallet-1"

        @staticmethod
        def get_wallet():
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
        "greenfloor.cli.manager._resolve_cloud_wallet_asset_id",
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
        "greenfloor.cli.manager._verify_offer_text_for_dexie",
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
        "greenfloor.cli.manager._verify_offer_text_for_dexie",
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
        "greenfloor.cli.manager._verify_offer_text_for_dexie",
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

    monkeypatch.setattr("greenfloor.cli.manager.importlib.import_module", _import_module)
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
        "greenfloor.cli.manager._verify_offer_text_for_dexie",
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
    """Verify that _build_offer_text_for_request calls offer_builder_sdk.build_offer directly."""
    from greenfloor.cli import manager

    monkeypatch.delenv("GREENFLOOR_OFFER_BUILDER_CMD", raising=False)
    monkeypatch.setattr(
        "greenfloor.cli.offer_builder_sdk.build_offer",
        lambda _payload: "offer1direct",
    )
    result = manager._build_offer_text_for_request({"test": True})
    assert result == "offer1direct"


def test_verify_offer_text_for_dexie_uses_validate_offer_when_available(monkeypatch) -> None:
    def _import_module(name: str):
        if name == "greenfloor_native":
            raise ImportError("disable native path for this test")
        return __import__(name)

    monkeypatch.setattr("greenfloor.cli.manager.importlib.import_module", _import_module)

    class _ConditionWithExpiry:
        @staticmethod
        def parse_assert_before_seconds_relative():
            return object()

    class _CoinSpendWithExpiry:
        @staticmethod
        def conditions():
            return [_ConditionWithExpiry()]

    class _SpendBundleWithExpiry:
        coin_spends = [_CoinSpendWithExpiry()]

    class _Sdk:
        @staticmethod
        def validate_offer(offer: str) -> None:
            assert offer == "offer1ok"

        @staticmethod
        def decode_offer(_offer: str):
            return _SpendBundleWithExpiry()

    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    assert _verify_offer_text_for_dexie("offer1ok") is None


def test_verify_offer_text_for_dexie_falls_back_to_verify_offer(monkeypatch) -> None:
    def _import_module(name: str):
        if name == "greenfloor_native":
            raise ImportError("disable native path for this test")
        return __import__(name)

    monkeypatch.setattr("greenfloor.cli.manager.importlib.import_module", _import_module)

    class _ConditionWithExpiry:
        @staticmethod
        def parse_assert_before_height_absolute():
            return object()

    class _CoinSpendWithExpiry:
        @staticmethod
        def conditions():
            return [_ConditionWithExpiry()]

    class _SpendBundleWithExpiry:
        coin_spends = [_CoinSpendWithExpiry()]

    class _Sdk:
        @staticmethod
        def verify_offer(offer: str) -> bool:
            return offer == "offer1ok"

        @staticmethod
        def decode_offer(_offer: str):
            return _SpendBundleWithExpiry()

    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    assert _verify_offer_text_for_dexie("offer1ok") is None
    assert _verify_offer_text_for_dexie("offer1bad") == "wallet_sdk_offer_verify_false"


def test_verify_offer_text_for_dexie_rejects_offer_without_expiration_condition(
    monkeypatch,
) -> None:
    def _import_module(name: str):
        if name == "greenfloor_native":
            raise ImportError("disable native path for this test")
        return __import__(name)

    monkeypatch.setattr("greenfloor.cli.manager.importlib.import_module", _import_module)

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

    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    assert _verify_offer_text_for_dexie("offer1noexpiry") == "wallet_sdk_offer_missing_expiration"


def test_verify_offer_text_for_dexie_uses_greenfloor_native_before_sdk(monkeypatch) -> None:
    calls = {}

    class _Native:
        @staticmethod
        def validate_offer(offer: str) -> None:
            calls["offer"] = offer

    class _Sdk:
        @staticmethod
        def validate_offer(_offer: str) -> None:
            raise AssertionError("sdk path should not run when native is available")

    monkeypatch.setitem(sys.modules, "greenfloor_native", _Native)
    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)

    assert _verify_offer_text_for_dexie("offer1native") is None
    assert calls["offer"] == "offer1native"


def test_verify_offer_text_for_dexie_returns_native_validation_error(monkeypatch) -> None:
    class _Native:
        @staticmethod
        def validate_offer(_offer: str) -> None:
            raise ValueError("native_invalid_offer")

    monkeypatch.setitem(sys.modules, "greenfloor_native", _Native)
    assert _verify_offer_text_for_dexie("offer1bad") == (
        "wallet_sdk_offer_validate_failed:native_invalid_offer"
    )


def test_coins_list_returns_minimal_fields(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    _write_program_with_cloud_wallet(program)

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = asset_id, include_pending
            return [
                {
                    "id": "coin-1",
                    "name": "coin-1",
                    "amount": 123,
                    "state": "PENDING",
                    "asset": {"id": "xch"},
                }
            ]

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    code = _coins_list(program_path=program, asset=None, vault_id=None)
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["count"] == 1
    assert payload["items"][0]["coin_id"] == "coin-1"
    assert payload["items"][0]["pending"] is True
    assert payload["items"][0]["spendable"] is False


def test_coins_list_resolves_asset_filter_before_listing(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    _write_program_with_cloud_wallet(program)

    calls = {"list_asset_id": None}

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, asset_id=None, include_pending=True):
            _ = include_pending
            calls["list_asset_id"] = asset_id
            return []

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_cloud_wallet_asset_id",
        lambda *, wallet, canonical_asset_id, symbol_hint=None: "Asset_resolved",
    )
    code = _coins_list(program_path=program, asset="BYC", vault_id=None)
    assert code == 0
    assert calls["list_asset_id"] == "Asset_resolved"
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["count"] == 0


def test_resolve_taker_or_coin_operation_fee_uses_coinset_value(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [5, 15]}

        @staticmethod
        def get_conservative_fee_estimate():
            return 15

    monkeypatch.setattr("greenfloor.cli.manager.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    fee, source = manager_mod._resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=0,
    )
    assert fee == 15
    assert source == "coinset_conservative"


def test_resolve_taker_or_coin_operation_fee_applies_minimum_floor(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [2]}

        @staticmethod
        def get_conservative_fee_estimate():
            return 2

    monkeypatch.setattr("greenfloor.cli.manager.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    fee, source = manager_mod._resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=5,
    )
    assert fee == 5
    assert source == "coinset_conservative_minimum_floor"


def test_resolve_taker_or_coin_operation_fee_falls_back_to_config_minimum(monkeypatch) -> None:
    class _FakeCoinset:
        _calls = 0

        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [0]}

        @classmethod
        def get_conservative_fee_estimate(cls):
            cls._calls += 1
            if cls._calls == 1:
                return 1
            return None

    monkeypatch.setattr("greenfloor.cli.manager.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    monkeypatch.setattr("greenfloor.cli.manager.time.sleep", lambda _seconds: None)

    fee, source = manager_mod._resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=0,
    )
    assert fee == 0
    assert source == "config_minimum_fee_fallback"


def test_resolve_taker_or_coin_operation_fee_fails_on_endpoint_preflight(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            raise RuntimeError("coinset_network_error:timed_out")

    monkeypatch.setattr("greenfloor.cli.manager.CoinsetAdapter", _FakeCoinset)
    try:
        manager_mod._resolve_taker_or_coin_operation_fee(network="mainnet", minimum_fee_mojos=0)
    except manager_mod._CoinsetFeeLookupPreflightError as exc:
        assert exc.failure_kind == "endpoint_validation_failed"
        assert "coinset_network_error" in exc.detail
    else:
        raise AssertionError("expected _CoinsetFeeLookupPreflightError")


def test_resolve_taker_or_coin_operation_fee_fails_on_temporary_advice_unavailable(
    monkeypatch,
) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": False, "error": "backend_overloaded"}

    monkeypatch.setattr("greenfloor.cli.manager.CoinsetAdapter", _FakeCoinset)
    try:
        manager_mod._resolve_taker_or_coin_operation_fee(network="mainnet", minimum_fee_mojos=0)
    except manager_mod._CoinsetFeeLookupPreflightError as exc:
        assert exc.failure_kind == "temporary_fee_advice_unavailable"
        assert "backend_overloaded" in exc.detail
    else:
        raise AssertionError("expected _CoinsetFeeLookupPreflightError")


def test_resolve_maker_offer_fee_is_zero() -> None:
    fee, source = manager_mod._resolve_maker_offer_fee(network="mainnet")
    assert fee == 0
    assert source == "maker_default_zero"


def test_coin_split_no_wait_uses_advised_fee(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_cloud_wallet_asset_id",
        lambda *, wallet, canonical_asset_id, symbol_hint=None: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-1"],
        amount_per_coin=10,
        number_of_coins=2,
        no_wait=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_abc123"], 10, 2, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] is None
    assert payload["waited"] is False
    assert payload["fee_mojos"] == 42
    assert payload["coin_selection_mode"] == "explicit"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_split_auto_selects_largest_spendable_asset_coin(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    calls: dict[str, tuple[list[str], int, int, int] | None] = {"split": None}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [
                    {"id": "Coin_small", "name": "small", "amount": 100, "state": "SETTLED"},
                    {"id": "Coin_big", "name": "big", "amount": 500, "state": "SETTLED"},
                    {"id": "Coin_pending", "name": "pending", "amount": 999, "state": "PENDING"},
                ]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-auto", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_cloud_wallet_asset_id",
        lambda *, wallet, canonical_asset_id, symbol_hint=None: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_big"], 10, 10, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_split_guardrail_blocks_when_it_would_lock_all_spendable_coins(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    split_called = [False]

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [{"id": "Coin_only", "name": "only", "amount": 500, "state": "SETTLED"}]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            split_called[0] = True
            return {"signature_request_id": "sr-guard", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_cloud_wallet_asset_id",
        lambda *, wallet, canonical_asset_id, symbol_hint=None: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
    )
    assert code == 2
    assert split_called[0] is False
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["error"] == "coin_split_guardrail_would_lock_all_spendable_coins"
    assert payload["spendable_asset_coin_count"] == 1
    assert payload["selected_spendable_coin_count"] == 1


def test_coin_split_guardrail_override_allows_lock_all_spendable(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    calls: dict[str, tuple[list[str], int, int, int] | None] = {"split": None}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [{"id": "Coin_only", "name": "only", "amount": 500, "state": "SETTLED"}]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-override", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_cloud_wallet_asset_id",
        lambda *, wallet, canonical_asset_id, symbol_hint=None: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
        allow_lock_all_spendable=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_only"], 10, 10, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_split_guardrail_prompt_override_allows_continue(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    calls: dict[str, tuple[list[str], int, int, int] | None] = {"split": None}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending
            if asset_id == "Asset_split_base":
                return [{"id": "Coin_only", "name": "only", "amount": 500, "state": "SETTLED"}]
            return [{"id": "Coin_old", "name": "old", "amount": 1, "state": "SETTLED"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-prompt", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_cloud_wallet_asset_id",
        lambda *, wallet, canonical_asset_id, symbol_hint=None: "Asset_split_base",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    monkeypatch.setattr("builtins.input", lambda _prompt: "y")

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=10,
        number_of_coins=10,
        no_wait=True,
        prompt_for_override=True,
    )
    assert code == 0
    assert calls["split"] == (["Coin_only"], 10, 10, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["resolved_asset_id"] == "Asset_split_base"


def test_coin_combine_no_wait_uses_advised_fee(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-2", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_cloud_wallet_asset_id",
        lambda *, wallet, canonical_asset_id, symbol_hint=None: "Asset_huun64oh7dbt9f1f9ie8khuw",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (77, "coinset_conservative"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=3,
        asset_id="xch",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 0
    assert calls["combine"] == (3, 77, True, "Asset_huun64oh7dbt9f1f9ie8khuw", None)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] is None
    assert payload["waited"] is False
    assert payload["fee_mojos"] == 77
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["asset_id"] == "xch"
    assert payload["resolved_asset_id"] == "Asset_huun64oh7dbt9f1f9ie8khuw"


def test_coin_split_returns_structured_error_when_fee_resolution_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            RuntimeError("coinset_unavailable")
        ),
    )
    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-1"],
        amount_per_coin=10,
        number_of_coins=2,
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"].startswith("fee_resolution_failed:")
    assert "coin_ops.minimum_fee_mojos" in payload["operator_guidance"]


def test_coin_combine_returns_structured_error_when_fee_resolution_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return []

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            RuntimeError("coinset_unavailable")
        ),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=3,
        asset_id="xch",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"].startswith("fee_resolution_failed:")
    assert "coin_ops.minimum_fee_mojos" in payload["operator_guidance"]


def test_coin_split_distinguishes_coinset_endpoint_preflight_failure(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            manager_mod._CoinsetFeeLookupPreflightError(
                failure_kind="endpoint_validation_failed",
                detail="coinset_network_error:timed_out",
                diagnostics={
                    "coinset_network": "mainnet",
                    "coinset_base_url": "https://coinset.org",
                },
            )
        ),
    )
    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-1"],
        amount_per_coin=10,
        number_of_coins=2,
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coinset_fee_preflight_failed:endpoint_validation_failed"
    assert payload["coinset_fee_lookup"]["failure_kind"] == "endpoint_validation_failed"
    assert "endpoint routing" in payload["operator_guidance"]


def test_coin_combine_distinguishes_temporary_fee_advice_unavailability(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (_ for _ in ()).throw(
            manager_mod._CoinsetFeeLookupPreflightError(
                failure_kind="temporary_fee_advice_unavailable",
                detail="backend_overloaded",
                diagnostics={
                    "coinset_network": "mainnet",
                    "coinset_base_url": "https://coinset.org",
                },
            )
        ),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=3,
        asset_id="xch",
        coin_ids=[],
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coinset_fee_preflight_failed:temporary_fee_advice_unavailable"
    assert payload["coinset_fee_lookup"]["failure_kind"] == "temporary_fee_advice_unavailable"
    assert "temporarily unavailable" in payload["operator_guidance"]


def test_coin_split_returns_structured_error_when_coin_id_not_found(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_known", "name": "known-coin-name"}]

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["missing-coin-name"],
        amount_per_coin=10,
        number_of_coins=2,
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coin_id_resolution_failed"
    assert payload["unknown_coin_ids"] == ["missing-coin-name"]


def test_coin_combine_with_coin_ids_resolves_to_global_ids(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [
                {"id": "Coin_a", "name": "coin-a"},
                {"id": "Coin_b", "name": "coin-b"},
            ]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-2", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (7, "coinset_conservative"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=2,
        asset_id="xch",
        coin_ids=["coin-a", "coin-b"],
        no_wait=True,
    )
    assert code == 0
    assert calls["combine"] == (2, 7, True, "xch", ["Coin_a", "Coin_b"])
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["waited"] is False


def test_coin_combine_returns_structured_error_when_coin_id_not_found(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_known", "name": "known-coin-name"}]

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=1 + 1,
        asset_id="xch",
        coin_ids=["missing-coin-name", "known-coin-name"],
        no_wait=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["success"] is False
    assert payload["error"] == "coin_id_resolution_failed"
    assert payload["unknown_coin_ids"] == ["missing-coin-name"]


def test_coin_split_uses_market_ladder_target_when_size_is_provided(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program, provider="splash")
    _write_markets_with_ladder(markets)
    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            calls["split"] = (coin_ids, amount_per_coin, number_of_coins, fee)
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=["coin-1"],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=True,
        venue="splash",
        size_base_units=10,
    )
    assert code == 0
    assert calls["split"] == (["Coin_abc123"], 10, 4, 42)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] == "splash"
    assert payload["denomination_target"]["required_count"] == 4


def test_coin_combine_uses_market_ladder_threshold_when_size_is_provided(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program, provider="splash")
    _write_markets_with_ladder(markets)
    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-2", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (77, "coinset_conservative"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=0,
        asset_id=None,
        coin_ids=[],
        no_wait=True,
        venue="splash",
        size_base_units=10,
    )
    assert code == 0
    assert calls["combine"] == (6, 77, True, "a1", None)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] == "splash"
    assert payload["denomination_target"]["combine_threshold_count"] == 6


def test_coin_combine_ladder_threshold_uses_ceil(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program, provider="dexie")
    markets.write_text(
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
                "          combine_when_excess_factor: 1.5",
            ]
        ),
        encoding="utf-8",
    )
    calls = {}

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [{"id": "Coin_abc123", "name": "coin-1"}]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            calls["combine"] = (number_of_coins, fee, largest_first, asset_id, input_coin_ids)
            return {"signature_request_id": "sr-2", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (77, "coinset_conservative"),
    )
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=0,
        asset_id=None,
        coin_ids=[],
        no_wait=True,
        size_base_units=10,
    )
    assert code == 0
    assert calls["combine"][0] == 5


def test_coin_split_until_ready_ignores_unknown_states_and_string_asset(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program, provider="dexie")
    _write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"
        _calls = 0

        def __init__(self, _config):
            pass

        @classmethod
        def list_coins(cls, *, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            cls._calls += 1
            return [
                {"id": "Coin_a", "name": "coin-a", "amount": 10, "state": "LOCKED", "asset": "a1"},
                {
                    "id": "Coin_b",
                    "name": "coin-b",
                    "amount": 10,
                    "state": "MYSTERY",
                    "asset": {"id": "a1"},
                },
            ]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            return {"signature_request_id": "sr-1", "status": "SIGNED"}

        @staticmethod
        def get_signature_request(signature_request_id):
            _ = signature_request_id
            return {"status": "SIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._wait_for_mempool_then_confirmation",
        lambda **kwargs: [],
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=False,
        size_base_units=10,
        until_ready=True,
        max_iterations=1,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["error"] == "no_spendable_split_coin_available"
    assert payload["resolved_asset_id"] == "a1"


def test_coin_split_until_ready_requires_size_base_units(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets(markets)
    try:
        _coin_split(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id="m1",
            pair=None,
            coin_ids=[],
            amount_per_coin=10,
            number_of_coins=2,
            no_wait=False,
            until_ready=True,
            size_base_units=None,
        )
    except ValueError as exc:
        assert "--size-base-units" in str(exc)
    else:
        raise AssertionError("expected ValueError")


def test_coin_split_until_ready_disallows_no_wait(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program)
    _write_markets_with_ladder(markets)
    try:
        _coin_split(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id="m1",
            pair=None,
            coin_ids=[],
            amount_per_coin=10,
            number_of_coins=4,
            no_wait=True,
            until_ready=True,
            size_base_units=10,
        )
    except ValueError as exc:
        assert "requires wait mode" in str(exc)
    else:
        raise AssertionError("expected ValueError")


def test_coin_split_until_ready_reports_not_ready(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program, provider="dexie")
    _write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"
        _calls = 0

        def __init__(self, _config):
            pass

        @classmethod
        def list_coins(cls, *, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            cls._calls += 1
            # Never reaches target 4 coins of size 10.
            return [
                {
                    "id": "Coin_a",
                    "name": "coin-a",
                    "amount": 10,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                },
                {
                    "id": "Coin_b",
                    "name": "coin-b",
                    "amount": 9,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                },
            ]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            return {"signature_request_id": "sr-1", "status": "SIGNED"}

        @staticmethod
        def get_signature_request(signature_request_id):
            _ = signature_request_id
            return {"status": "SIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (42, "coinset_conservative"),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._wait_for_mempool_then_confirmation",
        lambda **kwargs: [],
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=False,
        venue="dexie",
        size_base_units=10,
        until_ready=True,
        max_iterations=2,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["until_ready"] is True
    assert payload["stop_reason"] == "max_iterations_reached"
    assert payload["denomination_readiness"]["ready"] is False
    assert len(payload["operations"]) == 2


# ---------------------------------------------------------------------------
# _is_spendable_coin unit tests
# ---------------------------------------------------------------------------


def test_is_spendable_coin_allowlist_states_are_spendable() -> None:
    from greenfloor.cli.manager import _is_spendable_coin

    for state in ("CONFIRMED", "UNSPENT", "SPENDABLE", "AVAILABLE", "SETTLED"):
        assert _is_spendable_coin({"state": state}) is True, state


def test_is_spendable_coin_known_non_spendable_states() -> None:
    from greenfloor.cli.manager import _is_spendable_coin

    for state in ("PENDING", "MEMPOOL", "SPENT", "SPENDING", "LOCKED", "RESERVED", "UNCONFIRMED"):
        assert _is_spendable_coin({"state": state}) is False, state


def test_is_spendable_coin_unknown_state_is_not_spendable() -> None:
    from greenfloor.cli.manager import _is_spendable_coin

    assert _is_spendable_coin({"state": "MYSTERY"}) is False
    assert _is_spendable_coin({"state": "TRANSITIONING"}) is False


def test_is_spendable_coin_missing_state_is_not_spendable() -> None:
    from greenfloor.cli.manager import _is_spendable_coin

    assert _is_spendable_coin({}) is False
    assert _is_spendable_coin({"state": ""}) is False


# ---------------------------------------------------------------------------
# _resolve_coin_global_ids unit tests
# ---------------------------------------------------------------------------


def test_resolve_coin_global_ids_maps_name_to_global_id() -> None:
    from greenfloor.cli.manager import _resolve_coin_global_ids

    wallet_coins = [{"id": "Coin_abc", "name": "hexname123"}]
    resolved, unresolved = _resolve_coin_global_ids(wallet_coins, ["hexname123"])
    assert resolved == ["Coin_abc"]
    assert unresolved == []


def test_resolve_coin_global_ids_passes_through_coin_prefix_ids() -> None:
    from greenfloor.cli.manager import _resolve_coin_global_ids

    resolved, unresolved = _resolve_coin_global_ids([], ["Coin_direct"])
    assert resolved == ["Coin_direct"]
    assert unresolved == []


def test_resolve_coin_global_ids_reports_unresolved_names() -> None:
    from greenfloor.cli.manager import _resolve_coin_global_ids

    wallet_coins = [{"id": "Coin_known", "name": "known"}]
    resolved, unresolved = _resolve_coin_global_ids(wallet_coins, ["known", "unknown"])
    assert resolved == ["Coin_known"]
    assert unresolved == ["unknown"]


def test_resolve_coin_global_ids_empty_inputs() -> None:
    from greenfloor.cli.manager import _resolve_coin_global_ids

    resolved, unresolved = _resolve_coin_global_ids([], [])
    assert resolved == []
    assert unresolved == []


# ---------------------------------------------------------------------------
# _evaluate_denomination_readiness unit tests
# ---------------------------------------------------------------------------


def test_evaluate_denomination_readiness_counts_only_spendable_matching_coins() -> None:
    from greenfloor.cli.manager import _evaluate_denomination_readiness

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            return [
                # ✓ spendable, right asset, right amount
                {"id": "c1", "state": "CONFIRMED", "amount": 10, "asset": {"id": "a1"}},
                # ✗ not spendable
                {"id": "c2", "state": "PENDING", "amount": 10, "asset": {"id": "a1"}},
                # ✗ wrong asset
                {"id": "c3", "state": "CONFIRMED", "amount": 10, "asset": {"id": "other"}},
                # ✗ wrong amount
                {"id": "c4", "state": "CONFIRMED", "amount": 20, "asset": {"id": "a1"}},
                # ✓ string-form asset id
                {"id": "c5", "state": "CONFIRMED", "amount": 10, "asset": "a1"},
            ]

    result = _evaluate_denomination_readiness(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        asset_id="a1",
        size_base_units=10,
        required_min_count=2,
    )
    assert result["current_count"] == 2
    assert result["ready"] is True


def test_evaluate_denomination_readiness_returns_not_ready_below_min() -> None:
    from greenfloor.cli.manager import _evaluate_denomination_readiness

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            return [{"id": "c1", "state": "CONFIRMED", "amount": 10, "asset": {"id": "a1"}}]

    result = _evaluate_denomination_readiness(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        asset_id="a1",
        size_base_units=10,
        required_min_count=3,
    )
    assert result["current_count"] == 1
    assert result["ready"] is False


def test_evaluate_denomination_readiness_max_allowed_count() -> None:
    from greenfloor.cli.manager import _evaluate_denomination_readiness

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            return [
                {"id": f"c{i}", "state": "CONFIRMED", "amount": 10, "asset": {"id": "a1"}}
                for i in range(8)
            ]

    # 8 coins > max_allowed_count of 6 → not ready
    result = _evaluate_denomination_readiness(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        asset_id="a1",
        size_base_units=10,
        max_allowed_count=6,
    )
    assert result["current_count"] == 8
    assert result["ready"] is False


def test_evaluate_denomination_readiness_asset_id_match_is_case_insensitive() -> None:
    from greenfloor.cli.manager import _evaluate_denomination_readiness

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            return [{"id": "c1", "state": "CONFIRMED", "amount": 5, "asset": {"id": "A1"}}]

    result = _evaluate_denomination_readiness(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        asset_id="a1",
        size_base_units=5,
        required_min_count=1,
    )
    assert result["current_count"] == 1
    assert result["ready"] is True


# ---------------------------------------------------------------------------
# _poll_signature_request_until_not_unsigned tests
# ---------------------------------------------------------------------------


def test_poll_signature_request_returns_immediately_when_already_signed(monkeypatch) -> None:
    import time as time_module

    from greenfloor.cli.manager import _poll_signature_request_until_not_unsigned

    class _FakeWallet:
        @staticmethod
        def get_signature_request(*, signature_request_id):
            _ = signature_request_id
            return {"status": "SUBMITTED"}

    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: 0.0)

    status, events = _poll_signature_request_until_not_unsigned(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        signature_request_id="sr-1",
        timeout_seconds=900,
        warning_interval_seconds=600,
    )
    assert status == "SUBMITTED"
    assert events == []


def test_poll_signature_request_emits_warning_event_after_interval(monkeypatch) -> None:
    import time as time_module

    from greenfloor.cli.manager import _poll_signature_request_until_not_unsigned

    call_count = [0]

    class _FakeWallet:
        @staticmethod
        def get_signature_request(*, signature_request_id):
            _ = signature_request_id
            call_count[0] += 1
            return {"status": "UNSIGNED" if call_count[0] < 2 else "SUBMITTED"}

    # monotonic: start=0, then elapsed=601 (triggers warning), then not called again
    elapsed_seq = iter([0.0, 601.0, 601.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))

    status, events = _poll_signature_request_until_not_unsigned(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        signature_request_id="sr-1",
        timeout_seconds=900,
        warning_interval_seconds=600,
    )
    assert status == "SUBMITTED"
    warning_events = [e for e in events if e["event"] == "signature_wait_warning"]
    assert len(warning_events) == 1
    assert warning_events[0]["message"] == "still_waiting_on_user_signature"


def test_poll_signature_request_raises_on_timeout(monkeypatch) -> None:
    import time as time_module

    import pytest

    from greenfloor.cli.manager import _poll_signature_request_until_not_unsigned

    class _FakeWallet:
        @staticmethod
        def get_signature_request(*, signature_request_id):
            _ = signature_request_id
            return {"status": "UNSIGNED"}

    # start=0, then elapsed=901 immediately exceeds timeout=900
    elapsed_seq = iter([0.0, 901.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))

    with pytest.raises(RuntimeError, match="signature_request_timeout"):
        _poll_signature_request_until_not_unsigned(
            wallet=_FakeWallet(),  # type: ignore[arg-type]
            signature_request_id="sr-1",
            timeout_seconds=900,
            warning_interval_seconds=600,
        )


def test_poll_signature_request_emits_escalation_on_repeated_warnings(monkeypatch) -> None:
    import time as time_module

    from greenfloor.cli.manager import _poll_signature_request_until_not_unsigned

    call_count = [0]

    class _FakeWallet:
        @staticmethod
        def get_signature_request(*, signature_request_id):
            _ = signature_request_id
            call_count[0] += 1
            if call_count[0] < 3:
                return {"status": "UNSIGNED"}
            return {"status": "SUBMITTED"}

    elapsed_seq = iter([0.0, 601.0, 1201.0, 1201.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))

    _status, events = _poll_signature_request_until_not_unsigned(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        signature_request_id="sr-1",
        timeout_seconds=1800,
        warning_interval_seconds=600,
    )
    escalations = [e for e in events if e["event"] == "signature_wait_escalation"]
    assert len(escalations) == 1
    assert escalations[0]["warning_count"] == "2"


def test_poll_signature_request_retries_transient_errors(monkeypatch) -> None:
    import time as time_module

    from greenfloor.cli.manager import _poll_signature_request_until_not_unsigned

    call_count = [0]

    class _FakeWallet:
        @staticmethod
        def get_signature_request(*, signature_request_id):
            _ = signature_request_id
            call_count[0] += 1
            if call_count[0] == 1:
                raise RuntimeError("temporary")
            return {"status": "SUBMITTED"}

    elapsed_seq = iter([0.0, 0.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))

    _status, events = _poll_signature_request_until_not_unsigned(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        signature_request_id="sr-1",
        timeout_seconds=900,
        warning_interval_seconds=600,
    )
    retries = [e for e in events if e["event"] == "poll_retry"]
    assert len(retries) == 1
    assert retries[0]["action"] == "wallet_get_signature_request"


# ---------------------------------------------------------------------------
# _wait_for_mempool_then_confirmation tests
# ---------------------------------------------------------------------------


def test_wait_for_mempool_emits_in_mempool_event_with_coinset_url(monkeypatch) -> None:
    import time as time_module

    from greenfloor.cli.manager import _wait_for_mempool_then_confirmation

    lc_call = [0]

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            lc_call[0] += 1
            if lc_call[0] == 1:
                return [{"id": "new-id", "name": "abc123hex", "state": "PENDING"}]
            return [{"id": "new-id", "name": "abc123hex", "state": "CONFIRMED"}]

    elapsed_seq = iter([0.0, 0.0, 0.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.cli.manager._coinset_reconcile_coin_state",
        lambda **kwargs: {"reconcile": "ok", "confirmed_block_index": "10"},
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._watch_reorg_risk_with_coinset",
        lambda **kwargs: [{"event": "reorg_watch_complete"}],
    )

    events = _wait_for_mempool_then_confirmation(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        network="mainnet",
        initial_coin_ids=set(),
        mempool_warning_seconds=300,
        confirmation_warning_seconds=900,
    )
    in_mempool = [e for e in events if e["event"] == "in_mempool"]
    assert len(in_mempool) == 1
    assert in_mempool[0]["coinset_url"] == "https://coinset.org/coin/abc123hex"


def test_wait_for_mempool_returns_when_confirmed_coin_appears(monkeypatch) -> None:
    import time as time_module

    from greenfloor.cli.manager import _wait_for_mempool_then_confirmation

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            return [{"id": "new-id", "name": "confirmed-hex", "state": "CONFIRMED"}]

    elapsed_seq = iter([0.0, 0.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.cli.manager._coinset_reconcile_coin_state",
        lambda **kwargs: {"reconcile": "ok", "confirmed_block_index": "10"},
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._watch_reorg_risk_with_coinset",
        lambda **kwargs: [{"event": "reorg_watch_complete"}],
    )

    events = _wait_for_mempool_then_confirmation(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        network="mainnet",
        initial_coin_ids=set(),
        mempool_warning_seconds=300,
        confirmation_warning_seconds=900,
    )
    # Returns successfully (no in_mempool event since we went straight to confirmed)
    assert all(e["event"] != "in_mempool" for e in events)


def test_wait_for_mempool_emits_warning_when_no_mempool_entry(monkeypatch) -> None:
    import time as time_module

    from greenfloor.cli.manager import _wait_for_mempool_then_confirmation

    lc_call = [0]

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            lc_call[0] += 1
            if lc_call[0] == 1:
                return []  # no new coins yet
            return [{"id": "new-id", "state": "CONFIRMED"}]

    # start=0, iteration-1 elapsed=300 (triggers mempool warning), iteration-2 returns
    elapsed_seq = iter([0.0, 300.0, 300.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.cli.manager._coinset_reconcile_coin_state",
        lambda **kwargs: {"reconcile": "ok", "confirmed_block_index": "10"},
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._watch_reorg_risk_with_coinset",
        lambda **kwargs: [{"event": "reorg_watch_complete"}],
    )

    events = _wait_for_mempool_then_confirmation(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        network="mainnet",
        initial_coin_ids=set(),
        mempool_warning_seconds=300,
        confirmation_warning_seconds=900,
    )
    warning_events = [e for e in events if e["event"] == "mempool_wait_warning"]
    assert len(warning_events) == 1


def test_wait_for_mempool_ignores_coins_in_initial_set(monkeypatch) -> None:
    import time as time_module

    from greenfloor.cli.manager import _wait_for_mempool_then_confirmation

    lc_call = [0]

    class _FakeWallet:
        @staticmethod
        def list_coins(*, include_pending=True):
            lc_call[0] += 1
            if lc_call[0] == 1:
                # only known initial coins — should not trigger pending or confirmed
                return [{"id": "old-id", "state": "CONFIRMED"}]
            return [
                {"id": "old-id", "state": "CONFIRMED"},
                {"id": "new-id", "state": "CONFIRMED"},
            ]

    elapsed_seq = iter([0.0, 0.0, 0.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.cli.manager._coinset_reconcile_coin_state",
        lambda **kwargs: {"reconcile": "ok", "confirmed_block_index": "10"},
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._watch_reorg_risk_with_coinset",
        lambda **kwargs: [{"event": "reorg_watch_complete"}],
    )

    _wait_for_mempool_then_confirmation(
        wallet=_FakeWallet(),  # type: ignore[arg-type]
        network="mainnet",
        initial_coin_ids={"old-id"},
        mempool_warning_seconds=300,
        confirmation_warning_seconds=900,
    )
    # Returns because new-id appeared in confirmed, but old-id was ignored
    assert lc_call[0] == 2


def test_watch_reorg_risk_waits_until_additional_blocks(monkeypatch) -> None:
    import time as time_module

    from greenfloor.cli.manager import _watch_reorg_risk_with_coinset

    peak_seq = iter([100, 102, 106])
    elapsed_seq = iter([0.0, 0.0, 10.0, 20.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr(
        "greenfloor.cli.manager._coinset_peak_height", lambda **kwargs: next(peak_seq)
    )

    events = _watch_reorg_risk_with_coinset(
        network="mainnet",
        confirmed_block_index=100,
        additional_blocks=6,
        warning_interval_seconds=300,
    )
    assert events[0]["event"] == "reorg_watch_started"
    assert events[-1]["event"] == "reorg_watch_complete"


def test_watch_reorg_risk_times_out_when_chain_stalls(monkeypatch) -> None:
    import time as time_module

    from greenfloor.cli.manager import _watch_reorg_risk_with_coinset

    elapsed_seq = iter([0.0, 0.0, 61.0])
    monkeypatch.setattr(time_module, "sleep", lambda _: None)
    monkeypatch.setattr(time_module, "monotonic", lambda: next(elapsed_seq))
    monkeypatch.setattr("greenfloor.cli.manager._coinset_peak_height", lambda **kwargs: 100)

    events = _watch_reorg_risk_with_coinset(
        network="mainnet",
        confirmed_block_index=100,
        additional_blocks=6,
        warning_interval_seconds=300,
        timeout_seconds=60,
    )
    assert events[0]["event"] == "reorg_watch_started"
    assert events[-1]["event"] == "reorg_watch_timeout"
    assert events[-1]["remaining_blocks"] == "6"


# ---------------------------------------------------------------------------
# _build_and_post_offer cloud wallet dispatch gate
# ---------------------------------------------------------------------------


def test_build_and_post_offer_dispatches_to_cloud_wallet_when_configured(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    dispatched = [False]
    captured_dry_run: list[bool] = []

    def _fake_cloud_wallet(**kwargs):
        dispatched[0] = True
        captured_dry_run.append(bool(kwargs["dry_run"]))
        return 0

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_and_post_offer_cloud_wallet",
        _fake_cloud_wallet,
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
    assert dispatched[0] is True
    assert captured_dry_run == [False]


def test_build_and_post_offer_dry_run_uses_cloud_wallet_when_configured(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program)
    _write_markets(markets)

    dispatched = [False]
    captured_dry_run: list[bool] = []

    def _fake_cloud_wallet(**kwargs):
        dispatched[0] = True
        captured_dry_run.append(bool(kwargs["dry_run"]))
        print(json.dumps({"dry_run": True, "results": [], "built_offers_preview": []}))
        return 0

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_and_post_offer_cloud_wallet",
        _fake_cloud_wallet,
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
        dry_run=True,
    )
    assert code == 0
    assert dispatched[0] is True
    assert captured_dry_run == [True]
    payload = json.loads(capsys.readouterr().out.strip().splitlines()[-1])
    assert payload["dry_run"] is True


# ---------------------------------------------------------------------------
# _build_and_post_offer_cloud_wallet direct tests
# ---------------------------------------------------------------------------


def _load_program_and_market(program_path: Path, markets_path: Path):
    from greenfloor.config.io import load_markets_config, load_program_config

    prog = load_program_config(program_path)
    mkt = load_markets_config(markets_path).markets[0]
    return prog, mkt


def test_build_and_post_offer_cloud_wallet_happy_path_dexie(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    from greenfloor.cli.manager import _build_and_post_offer_cloud_wallet

    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program_path)
    _write_markets_with_ladder(markets_path)
    prog, mkt = _load_program_and_market(program_path, markets_path)
    prog.home_dir = str(tmp_path)
    reset_concurrent_log_handlers(module=manager_mod)

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def create_offer(
            *,
            offered,
            requested,
            fee,
            expires_at_iso,
            split_input_coins=True,
            split_input_coins_fee=0,
        ):
            _ = split_input_coins, split_input_coins_fee
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet():
            return {"offers": [{"bech32": "offer1testartifact"}]}

    posted = {}

    class _FakeDexie:
        def __init__(self, _base_url: str):
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            posted["offer"] = offer
            return {"success": True, "id": "dexie-99"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SUBMITTED", []),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._poll_offer_artifact_until_available",
        lambda **kwargs: "offer1testartifact",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._verify_offer_text_for_dexie",
        lambda _offer: None,
    )
    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)

    code = _build_and_post_offer_cloud_wallet(
        program=prog,
        market=mkt,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=0.003,
        dry_run=False,
    )
    assert code == 0
    assert posted["offer"] == "offer1testartifact"
    captured = capsys.readouterr()
    payload = json.loads(captured.out.strip())
    assert payload["publish_failures"] == 0
    assert payload["results"][0]["result"]["id"] == "dexie-99"
    assert payload["offer_fee_mojos"] == 0
    assert captured.err == ""
    log_text = (tmp_path / "logs" / "debug.log").read_text(encoding="utf-8")
    assert "signed_offer_file:offer1testartifact" in log_text
    assert "signed_offer_metadata:ticker=A1" in log_text
    assert "amount=10" in log_text
    assert "trading_pair=A1:xch" in log_text


def test_build_and_post_offer_cloud_wallet_returns_error_when_no_offer_artifact(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    from greenfloor.cli.manager import _build_and_post_offer_cloud_wallet

    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program_path)
    _write_markets_with_ladder(markets_path)
    prog, mkt = _load_program_and_market(program_path, markets_path)

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def create_offer(
            *,
            offered,
            requested,
            fee,
            expires_at_iso,
            split_input_coins=True,
            split_input_coins_fee=0,
        ):
            _ = split_input_coins, split_input_coins_fee
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet():
            return {"offers": []}  # no offer1... bech32

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SUBMITTED", []),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._poll_offer_artifact_until_available",
        lambda **kwargs: (_ for _ in ()).throw(RuntimeError("cloud_wallet_offer_artifact_timeout")),
    )

    code = _build_and_post_offer_cloud_wallet(
        program=prog,
        market=mkt,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=0.003,
        dry_run=False,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 1
    assert payload["results"][0]["result"]["error"] == "cloud_wallet_offer_artifact_timeout"


def test_build_and_post_offer_cloud_wallet_verify_error_blocks_post(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    from greenfloor.cli.manager import _build_and_post_offer_cloud_wallet

    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program_path)
    _write_markets_with_ladder(markets_path)
    prog, mkt = _load_program_and_market(program_path, markets_path)

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def create_offer(
            *,
            offered,
            requested,
            fee,
            expires_at_iso,
            split_input_coins=True,
            split_input_coins_fee=0,
        ):
            _ = split_input_coins, split_input_coins_fee
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet():
            return {"offers": [{"bech32": "offer1badoffer"}]}

    post_called = [False]

    class _FakeDexie:
        def __init__(self, _base_url: str):
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            post_called[0] = True
            return {"success": True}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SUBMITTED", []),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._poll_offer_artifact_until_available",
        lambda **kwargs: "offer1badoffer",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._verify_offer_text_for_dexie",
        lambda _offer: "wallet_sdk_offer_missing_expiration",
    )
    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)

    code = _build_and_post_offer_cloud_wallet(
        program=prog,
        market=mkt,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=0.003,
        dry_run=False,
    )
    assert code == 2
    assert post_called[0] is False
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["results"][0]["result"]["error"] == "wallet_sdk_offer_missing_expiration"


def test_build_and_post_offer_cloud_wallet_dry_run_skips_publish(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    from greenfloor.cli.manager import _build_and_post_offer_cloud_wallet

    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program_path)
    _write_markets_with_ladder(markets_path)
    prog, mkt = _load_program_and_market(program_path, markets_path)

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def create_offer(
            *,
            offered,
            requested,
            fee,
            expires_at_iso,
            split_input_coins=True,
            split_input_coins_fee=0,
        ):
            _ = offered, requested, fee, expires_at_iso, split_input_coins, split_input_coins_fee
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet():
            return {"offers": [{"bech32": "offer1dryruncloudwallet"}]}

    class _FailDexie:
        def __init__(self, _base_url: str):
            raise AssertionError("DexieAdapter must not be constructed in dry_run")

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SUBMITTED", []),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._poll_offer_artifact_until_available",
        lambda **kwargs: "offer1dryruncloudwallet",
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._verify_offer_text_for_dexie",
        lambda _offer: None,
    )
    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FailDexie)

    code = _build_and_post_offer_cloud_wallet(
        program=prog,
        market=mkt,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=0.003,
        dry_run=True,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["dry_run"] is True
    assert payload["publish_attempts"] == 0
    assert payload["publish_failures"] == 0
    assert payload["results"] == []
    assert len(payload["built_offers_preview"]) == 1


def test_poll_offer_artifact_until_available_returns_new_offer(monkeypatch) -> None:
    wallets = [
        {
            "offers": [
                {"offerId": "old-1", "bech32": "offer1old", "expiresAt": "2026-01-01T00:00:00Z"}
            ]
        },
        {
            "offers": [
                {"offerId": "new-1", "bech32": "offer1new", "expiresAt": "2026-01-02T00:00:00Z"}
            ]
        },
    ]
    monotonic_values = iter([0.0, 1.0, 1.0])

    class _FakeWallet:
        @staticmethod
        def get_wallet():
            if wallets:
                return wallets.pop(0)
            return {"offers": []}

    monkeypatch.setattr("greenfloor.cli.manager.time.sleep", lambda _seconds: None)
    monkeypatch.setattr("greenfloor.cli.manager.time.monotonic", lambda: next(monotonic_values))

    offer = manager_mod._poll_offer_artifact_until_available(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        known_markers={"id:old-1", "bech32:offer1old"},
        timeout_seconds=10,
    )
    assert offer == "offer1new"


def test_poll_offer_artifact_until_available_times_out(monkeypatch) -> None:
    monotonic_values = iter([0.0, 5.0, 11.0])

    class _FakeWallet:
        @staticmethod
        def get_wallet():
            return {"offers": []}

    monkeypatch.setattr("greenfloor.cli.manager.time.sleep", lambda _seconds: None)
    monkeypatch.setattr("greenfloor.cli.manager.time.monotonic", lambda: next(monotonic_values))

    try:
        manager_mod._poll_offer_artifact_until_available(
            wallet=cast(CloudWalletAdapter, _FakeWallet()),
            known_markers=set(),
            timeout_seconds=10,
        )
    except RuntimeError as exc:
        assert str(exc) == "cloud_wallet_offer_artifact_timeout"
    else:
        raise AssertionError("expected cloud_wallet_offer_artifact_timeout")


def test_poll_offer_artifact_until_available_requests_creator_open_pending(monkeypatch) -> None:
    calls: list[tuple[bool | None, list[str] | None, int]] = []
    monotonic_values = iter([0.0, 0.5, 0.5])

    class _FakeWallet:
        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=0):
            calls.append((is_creator, states, first))
            return {
                "offers": [
                    {"offerId": "new-1", "bech32": "offer1new", "expiresAt": "2026-01-02T00:00:00Z"}
                ]
            }

    monkeypatch.setattr("greenfloor.cli.manager.time.sleep", lambda _seconds: None)
    monkeypatch.setattr("greenfloor.cli.manager.time.monotonic", lambda: next(monotonic_values))

    offer = manager_mod._poll_offer_artifact_until_available(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        known_markers=set(),
        timeout_seconds=10,
    )
    assert offer == "offer1new"
    assert calls
    assert calls[0][0] is True
    assert calls[0][1] == ["OPEN", "PENDING"]


# ---------------------------------------------------------------------------
# until_ready success path (stop_reason="ready")
# ---------------------------------------------------------------------------


def test_coin_split_until_ready_succeeds_when_denominations_met(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program, provider="dexie")
    _write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            # 4 confirmed coins of size 10 + one larger reserve coin for asset a1.
            rows = [
                {
                    "id": f"Coin_{i}",
                    "name": f"coin-{i}",
                    "amount": 10,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
                for i in range(4)
            ]
            rows.append(
                {
                    "id": "Coin_reserve",
                    "name": "coin-reserve",
                    "amount": 20,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
            )
            return rows

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            return {"signature_request_id": "sr-ok", "status": "SIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SIGNED", []),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._wait_for_mempool_then_confirmation",
        lambda **kwargs: [],
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=False,
        size_base_units=10,
        until_ready=True,
        max_iterations=3,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["stop_reason"] == "ready"
    assert payload["denomination_readiness"]["ready"] is True
    assert payload["split_gate"]["reserve_ready"] is True


def test_coin_split_gate_ready_skips_split_in_non_interactive_mode(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program, provider="dexie")
    _write_markets_with_ladder(markets)

    split_called = [False]

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            _ = include_pending, asset_id
            return [
                {
                    "id": f"Coin_{i}",
                    "name": f"coin-{i}",
                    "amount": 10,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
                for i in range(4)
            ] + [
                {
                    "id": "Coin_reserve",
                    "name": "coin-reserve",
                    "amount": 50,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
            ]

        @staticmethod
        def split_coins(*, coin_ids, amount_per_coin, number_of_coins, fee):
            _ = coin_ids, amount_per_coin, number_of_coins, fee
            split_called[0] = True
            return {"signature_request_id": "sr-should-not-run", "status": "UNSIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )

    code = _coin_split(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        coin_ids=[],
        amount_per_coin=0,
        number_of_coins=0,
        no_wait=True,
        size_base_units=10,
        until_ready=False,
        max_iterations=1,
        prompt_for_override=False,
    )
    assert code == 0
    assert split_called[0] is False
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["stop_reason"] == "ready"
    assert payload["split_gate"]["ready"] is True


# ---------------------------------------------------------------------------
# coin-combine until_ready requires_new_coin_selection path
# ---------------------------------------------------------------------------


def test_coin_combine_until_ready_with_coin_ids_stops_with_requires_new_coin_selection(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_with_cloud_wallet(program, provider="dexie")
    _write_markets_with_ladder(markets)

    class _FakeWallet:
        vault_id = "wallet-1"

        def __init__(self, _config):
            pass

        @staticmethod
        def list_coins(*, include_pending=True, asset_id=None):
            # 8 coins > combine_threshold=6 → not ready after combine
            return [
                {
                    "id": f"Coin_{i}",
                    "name": f"coin-{i}",
                    "amount": 10,
                    "state": "CONFIRMED",
                    "asset": {"id": "a1"},
                }
                for i in range(8)
            ]

        @staticmethod
        def combine_coins(*, number_of_coins, fee, largest_first, asset_id, input_coin_ids=None):
            return {"signature_request_id": "sr-combine", "status": "SIGNED"}

    monkeypatch.setattr("greenfloor.cli.manager.CloudWalletAdapter", _FakeWallet)
    monkeypatch.setattr(
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network, minimum_fee_mojos=0: (0, "config_minimum_fee_fallback"),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SIGNED", []),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._wait_for_mempool_then_confirmation",
        lambda **kwargs: [],
    )

    # Provide explicit coin IDs so loop cannot auto-select new candidates
    code = _coin_combine(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        number_of_coins=6,  # matches combine_threshold and len(coin_ids)
        asset_id="a1",
        coin_ids=[f"coin-{i}" for i in range(6)],
        no_wait=False,
        size_base_units=10,
        until_ready=True,
        max_iterations=3,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["stop_reason"] == "requires_new_coin_selection"
    assert payload["denomination_readiness"]["ready"] is False
