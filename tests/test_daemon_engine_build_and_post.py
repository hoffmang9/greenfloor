"""Tests for daemon Rust engine build-and-post delegation."""

from __future__ import annotations

from pathlib import Path
from typing import Any
from unittest.mock import MagicMock

import pytest

from greenfloor.daemon.offer_dispatch.managed import managed_offer_post
from greenfloor.runtime.daemon_config_paths import (
    DaemonConfigPaths,
    resolve_daemon_config_paths,
    set_daemon_config_paths,
)
from greenfloor.runtime.engine_build_and_post import run_build_and_post_offer_in_process
from tests.helpers.daemon_test_fixtures import market_config, signer_program_config


def test_resolve_daemon_config_paths_uses_context_override(tmp_path: Path) -> None:
    program = signer_program_config()
    program_path = tmp_path / "config" / "program.yaml"
    markets_path = tmp_path / "config" / "markets.yaml"
    program_path.parent.mkdir(parents=True)
    program_path.write_text("app: {}\n", encoding="utf-8")
    markets_path.write_text("markets: []\n", encoding="utf-8")
    set_daemon_config_paths(
        DaemonConfigPaths(
            program_path=program_path,
            markets_path=markets_path,
        )
    )
    resolved = resolve_daemon_config_paths(program)
    assert resolved.program_path == program_path.resolve()
    assert resolved.markets_path == markets_path.resolve()


def test_run_build_and_post_offer_in_process_delegates_to_engine(monkeypatch) -> None:
    captured: dict[str, Any] = {}

    def _fake_engine(request: dict[str, Any]) -> dict[str, Any]:
        captured["request"] = request
        return {
            "exit_code": 0,
            "payload": {
                "publish_failures": 0,
                "results": [{"result": {"success": True, "id": "offer-rust-1"}}],
            },
        }

    monkeypatch.setattr(
        "greenfloor.runtime.engine_build_and_post._engine_build_and_post_offer",
        lambda: _fake_engine,
    )
    paths = DaemonConfigPaths(
        program_path=Path("/tmp/program.yaml"),
        markets_path=Path("/tmp/markets.yaml"),
    )
    exit_code, payload = run_build_and_post_offer_in_process(
        paths=paths,
        network="mainnet",
        market_id="eco_market",
        size_base_units=100,
        publish_venue="dexie",
        action_side="buy",
        persist_results=False,
    )
    assert exit_code == 0
    assert payload["results"][0]["result"]["id"] == "offer-rust-1"
    assert captured["request"]["market_id"] == "eco_market"
    assert captured["request"]["action_side"] == "buy"
    assert captured["request"]["persist_results"] is False


def test_managed_offer_post_uses_engine_build_and_post(monkeypatch) -> None:
    engine = MagicMock(
        return_value={
            "exit_code": 0,
            "payload": {
                "publish_failures": 0,
                "results": [
                    {
                        "result": {
                            "success": True,
                            "id": "offer-managed-1",
                            "timing_ms": {"create_total_ms": 42},
                        }
                    }
                ],
            },
        }
    )
    monkeypatch.setattr(
        "greenfloor.runtime.engine_build_and_post._engine_build_and_post_offer",
        lambda: engine,
    )
    result = managed_offer_post(
        program=signer_program_config(),
        market=market_config(),
        size_base_units=50,
        publish_venue="dexie",
        runtime_dry_run=False,
        side="sell",
    )
    assert result.success is True
    assert result.offer_id == "offer-managed-1"
    assert result.offer_create_ms == 42
    engine.assert_called_once()
    request = engine.call_args.args[0]
    assert request["size_base_units"] == 50
    assert request["action_side"] == "sell"


def test_run_build_and_post_offer_in_process_requires_market_selector() -> None:
    paths = DaemonConfigPaths(
        program_path=Path("/tmp/program.yaml"),
        markets_path=Path("/tmp/markets.yaml"),
    )
    with pytest.raises(ValueError, match="market_id or pair"):
        run_build_and_post_offer_in_process(
            paths=paths,
            network="mainnet",
            size_base_units=1,
        )
