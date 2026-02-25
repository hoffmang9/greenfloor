from __future__ import annotations

import json
import sys
from pathlib import Path

import greenfloor.cli.manager as manager_mod
from greenfloor.cli.manager import (
    _build_and_post_offer,
    _coin_combine,
    _coin_split,
    _coins_list,
    _resolve_dexie_base_url,
    _resolve_offer_publish_settings,
    _resolve_splash_base_url,
    _verify_offer_text_for_dexie,
)


def test_resolve_dexie_base_url_by_network() -> None:
    assert _resolve_dexie_base_url("mainnet", None) == "https://api.dexie.space"
    assert _resolve_dexie_base_url("testnet11", None) == "https://api-testnet.dexie.space"
    assert _resolve_dexie_base_url("testnet", None) == "https://api-testnet.dexie.space"


def test_resolve_splash_base_url_defaults_when_not_explicit() -> None:
    assert _resolve_splash_base_url(None) == "http://john-deere.hoffmang.com:4000"


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
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network: (42, "coinset_conservative"),
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
        "greenfloor.cli.manager._resolve_taker_or_coin_operation_fee",
        lambda *, network: (77, "coinset_conservative"),
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
    assert calls["combine"] == (3, 77, True, "xch", None)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["venue"] is None
    assert payload["waited"] is False
    assert payload["fee_mojos"] == 77
    assert payload["coin_selection_mode"] == "adapter_auto_select"


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
        lambda *, network: (_ for _ in ()).throw(RuntimeError("coinset_unavailable")),
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
    assert "GREENFLOOR_COINSET_ADVISED_FEE_MOJOS" in payload["operator_guidance"]


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
        lambda *, network: (_ for _ in ()).throw(RuntimeError("coinset_unavailable")),
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
    assert "GREENFLOOR_COINSET_ADVISED_FEE_MOJOS" in payload["operator_guidance"]


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
        lambda *, network: (0, "env_override"),
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
        lambda *, network: (7, "coinset_conservative"),
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
        lambda *, network: (0, "env_override"),
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
        lambda *, network: (42, "coinset_conservative"),
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
        lambda *, network: (77, "coinset_conservative"),
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
        lambda *, network: (77, "coinset_conservative"),
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
        lambda *, network: (42, "coinset_conservative"),
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
    assert payload["coin_selection_mode"] == "adapter_auto_select"
    assert payload["denomination_readiness"]["current_count"] == 0
    assert payload["denomination_readiness"]["ready"] is False


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
                }
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
        lambda *, network: (42, "coinset_conservative"),
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
