from __future__ import annotations

from pathlib import Path

import pytest

from tests.helpers.engine_binary import (
    GreenfloorEngineBinaryError,
    resolve_greenfloor_engine_binary,
)
from tests.helpers.manager_cli import parse_json_output, run_manager
from tests.helpers.manager_program_fixtures import (
    write_manager_program,
    write_manager_program_with_signer,
)


def test_resolve_greenfloor_engine_binary_from_env(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    binary = tmp_path / "greenfloor-engine"
    binary.write_text("#!/bin/sh\n", encoding="utf-8")
    binary.chmod(0o755)
    monkeypatch.setenv("GREENFLOOR_ENGINE_BIN", str(binary))
    assert resolve_greenfloor_engine_binary() == binary


def test_resolve_greenfloor_engine_binary_missing_env(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv("GREENFLOOR_ENGINE_BIN", raising=False)
    monkeypatch.setattr(
        "tests.helpers.engine_binary.shutil.which",
        lambda _name: None,
    )
    monkeypatch.setattr(
        "tests.helpers.engine_binary.repo_root",
        lambda: Path("/nonexistent"),
    )
    with pytest.raises(GreenfloorEngineBinaryError, match="binary not found"):
        resolve_greenfloor_engine_binary(build_if_missing=False)


# build-and-post-offer delegation, dry-run payload, and publish-failure exit codes are covered
# by greenfloor-engine/src/manager/build_and_post.rs unit tests.


def test_build_and_post_offer_dry_run_returns_preview(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    markets.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                '    base_asset: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"',
                '    base_symbol: "TCAT"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-main-1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    pricing:",
                "      min_price_quote_per_base: 0.0031",
                "      max_price_quote_per_base: 0.0038",
            ]
        ),
        encoding="utf-8",
    )

    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "build-and-post-offer",
            "--market-id",
            "m1",
            "--size-base-units",
            "1",
            "--dry-run",
            "--network",
            "mainnet",
        ],
        env={"GREENFLOOR_TEST_OFFER_TEXT": "offer1dryrunpreviewstub"},
    )
    assert code == 0
    payload = parse_json_output(stdout)
    assert payload["dry_run"] is True
    assert payload["publish_attempts"] == 0
    assert payload["built_offers_preview"]
    assert payload["results"] == []


def test_build_and_post_offer_rejects_invalid_repeat(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    markets.write_text("markets: []\n", encoding="utf-8")

    code, _stdout, stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "build-and-post-offer",
            "--market-id",
            "m1",
            "--size-base-units",
            "1",
            "--repeat",
            "0",
            "--network",
            "mainnet",
        ]
    )
    assert code != 0
    assert "repeat must be positive" in stderr
