from __future__ import annotations

from pathlib import Path
from unittest.mock import MagicMock

import pytest

from greenfloor.runtime.engine_build_and_post import run_build_and_post_offer_in_process
from greenfloor.runtime.offer_post_request import parse_managed_offer_post_result
from tests.helpers.daemon_test_fixtures import market_config


def test_run_build_and_post_offer_in_process_delegates_to_engine(monkeypatch) -> None:
    captured: dict[str, object] = {}

    def _fake_run(request: dict[str, object]) -> dict[str, object]:
        captured["request"] = request
        return {
            "exit_code": 0,
            "output": "",
            "payload": {
                "publish_failures": 0,
                "results": [{"result": {"success": True, "id": "offer-rust-1"}}],
            },
        }

    fake_engine = MagicMock()
    fake_engine.build_and_post_offer = _fake_run
    monkeypatch.setattr(
        "greenfloor.runtime.engine_build_and_post.import_engine",
        lambda: fake_engine,
    )

    exit_code, payload = run_build_and_post_offer_in_process(
        program_path=Path("/tmp/program.yaml"),
        markets_path=Path("/tmp/markets.yaml"),
        testnet_markets_path=None,
        network="mainnet",
        market_id="eco_market",
        size_base_units=100,
        publish_venue="dexie",
        action_side="buy",
        persist_results=False,
    )
    assert exit_code == 0
    assert payload["results"][0]["result"]["id"] == "offer-rust-1"
    request = captured["request"]
    assert isinstance(request, dict)
    assert request["market_id"] == "eco_market"
    assert request["action_side"] == "buy"
    assert request["persist_results"] is False


def test_run_managed_offer_post_uses_engine_build_and_post(monkeypatch) -> None:
    def _fake_run(request: dict[str, object]) -> dict[str, object]:
        return {
            "exit_code": 0,
            "output": "",
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

    engine = MagicMock()
    engine.build_and_post_offer = MagicMock(side_effect=_fake_run)
    monkeypatch.setattr(
        "greenfloor.runtime.engine_build_and_post.import_engine",
        lambda: engine,
    )

    exit_code, payload = run_build_and_post_offer_in_process(
        program_path=Path("/tmp/program.yaml"),
        markets_path=Path("/tmp/markets.yaml"),
        testnet_markets_path=None,
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
    engine.build_and_post_offer.assert_called_once()
    request = engine.build_and_post_offer.call_args.args[0]
    assert request["size_base_units"] == 50
    assert request["action_side"] == "sell"


def test_run_build_and_post_offer_in_process_requires_market_selector() -> None:
    with pytest.raises(ValueError, match="market_id or pair"):
        run_build_and_post_offer_in_process(
            program_path=Path("/tmp/program.yaml"),
            markets_path=Path("/tmp/markets.yaml"),
            testnet_markets_path=None,
            network="mainnet",
            size_base_units=1,
        )
