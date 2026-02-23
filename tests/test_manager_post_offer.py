from __future__ import annotations

import json
import sys
from pathlib import Path

from greenfloor.cli.manager import (
    _build_and_post_offer,
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


def _write_program(path: Path, *, provider: str = "dexie") -> None:
    path.write_text(
        "\n".join(
            [
                "app:",
                '  network: "mainnet"',
                '  home_dir: "~/.greenfloor"',
                "runtime:",
                "  loop_interval_seconds: 30",
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
    class _Sdk:
        @staticmethod
        def validate_offer(offer: str) -> None:
            assert offer == "offer1ok"

    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    assert _verify_offer_text_for_dexie("offer1ok") is None


def test_verify_offer_text_for_dexie_falls_back_to_verify_offer(monkeypatch) -> None:
    class _Sdk:
        @staticmethod
        def verify_offer(offer: str) -> bool:
            return offer == "offer1ok"

    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    assert _verify_offer_text_for_dexie("offer1ok") is None
    assert _verify_offer_text_for_dexie("offer1bad") == "wallet_sdk_offer_verify_false"
