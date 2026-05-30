"""Tests for daemon Rust engine build-and-post delegation."""

from __future__ import annotations

from pathlib import Path
from typing import Any
from unittest.mock import MagicMock

import pytest

from greenfloor.runtime.engine_build_and_post import run_build_and_post_offer_in_process
from greenfloor.runtime.offer_post_request import parse_managed_offer_post_result
from greenfloor.runtime.resolved_daemon_paths import (
    ResolvedDaemonPaths,
    resolve_resolved_daemon_paths,
    set_resolved_daemon_paths,
)
from tests.helpers.daemon_test_fixtures import market_config, signer_program_config


def test_resolve_resolved_daemon_paths_uses_context_override(tmp_path: Path) -> None:
    program = signer_program_config()
    program_path = tmp_path / "config" / "program.yaml"
    markets_path = tmp_path / "config" / "markets.yaml"
    program_path.parent.mkdir(parents=True)
    program_path.write_text("app: {}\n", encoding="utf-8")
    markets_path.write_text("markets: []\n", encoding="utf-8")
    set_resolved_daemon_paths(
        ResolvedDaemonPaths(
            program_path=program_path,
            markets_path=markets_path,
        )
    )
    resolved = resolve_resolved_daemon_paths(program)
    assert resolved.program_path == program_path.resolve()
    assert resolved.markets_path == markets_path.resolve()


def _fake_build_response() -> MagicMock:
    response = MagicMock()
    response.exit_code = 0
    response.payload = {
        "publish_failures": 0,
        "results": [{"result": {"success": True, "id": "offer-rust-1"}}],
    }
    return response


def test_run_build_and_post_offer_in_process_delegates_to_engine(monkeypatch) -> None:
    captured: dict[str, Any] = {}

    def _fake_run(request: Any) -> MagicMock:
        captured["request"] = request
        return _fake_build_response()

    fake_engine = MagicMock()

    def _capture_request(*_args: Any, **kwargs: Any) -> dict[str, Any]:
        return kwargs

    fake_engine.BuildAndPostOfferRequest = _capture_request
    fake_engine.build_and_post_offer = _fake_run
    monkeypatch.setattr(
        "greenfloor.runtime.engine_build_and_post.import_engine",
        lambda: fake_engine,
    )
    paths = ResolvedDaemonPaths(
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


def test_run_managed_offer_post_uses_engine_build_and_post(monkeypatch) -> None:
    response = MagicMock()
    response.exit_code = 0
    response.payload = {
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
    }

    def _build_request(
        program_path: Path,
        markets_path: Path,
        network: str,
        size_base_units: int,
        **kwargs: Any,
    ) -> dict[str, Any]:
        return {
            "program_path": program_path,
            "markets_path": markets_path,
            "network": network,
            "size_base_units": size_base_units,
            **kwargs,
        }

    engine = MagicMock(return_value=response)
    fake_module = MagicMock()
    fake_module.BuildAndPostOfferRequest = _build_request
    fake_module.build_and_post_offer = engine
    monkeypatch.setattr(
        "greenfloor.runtime.engine_build_and_post.import_engine",
        lambda: fake_module,
    )
    paths = ResolvedDaemonPaths(
        program_path=Path("/tmp/program.yaml"),
        markets_path=Path("/tmp/markets.yaml"),
    )
    exit_code, payload = run_build_and_post_offer_in_process(
        paths=paths,
        network="mainnet",
        market_id=market_config().market_id,
        size_base_units=50,
        publish_venue="dexie",
        action_side="sell",
        persist_results=False,
    )
    result = parse_managed_offer_post_result(exit_code, payload)
    assert result.success is True
    assert result.offer_id == "offer-managed-1"
    assert result.offer_create_ms == 42
    engine.assert_called_once()
    request = engine.call_args.args[0]
    assert request["size_base_units"] == 50
    assert request["action_side"] == "sell"


def test_run_build_and_post_offer_in_process_requires_market_selector() -> None:
    paths = ResolvedDaemonPaths(
        program_path=Path("/tmp/program.yaml"),
        markets_path=Path("/tmp/markets.yaml"),
    )
    with pytest.raises(ValueError, match="market_id or pair"):
        run_build_and_post_offer_in_process(
            paths=paths,
            network="mainnet",
            size_base_units=1,
        )
