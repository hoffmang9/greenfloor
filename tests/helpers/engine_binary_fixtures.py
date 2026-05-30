"""Shared helpers for mocking greenfloor-engine CLI delegation in tests."""

from __future__ import annotations

import json
from typing import Any

from greenfloor.runtime.json_output import format_json_output


def default_build_post_success_payload(**overrides: Any) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "market_id": "m1",
        "pair": "a1:xch",
        "resolved_base_asset_id": "a1",
        "resolved_quote_asset_id": "xch",
        "network": "mainnet",
        "size_base_units": 10,
        "repeat": 1,
        "publish_venue": "dexie",
        "dexie_base_url": "https://api.dexie.space",
        "splash_base_url": None,
        "drop_only": True,
        "claim_rewards": False,
        "dry_run": False,
        "publish_attempts": 1,
        "publish_failures": 0,
        "built_offers_preview": [],
        "bootstrap_actions": [],
        "results": [
            {
                "venue": "dexie",
                "result": {
                    "success": True,
                    "id": "offer-123",
                    "offer_view_url": "https://dexie.space/offers/offer-123",
                    "execution_mode": "direct",
                },
            }
        ],
        "offer_fee_mojos": 0,
        "offer_fee_source": "coinset_fee_unavailable",
        "execution_backend": "signer",
        "signer_path": True,
    }
    payload.update(overrides)
    return payload


def patch_engine_build_and_post(
    monkeypatch,
    *,
    exit_code: int = 0,
    payload: dict[str, Any] | None = None,
    capture: dict[str, Any] | None = None,
) -> None:
    captured = capture if capture is not None else {}

    def _fake_run(**kwargs: Any) -> int:
        captured.update(kwargs)
        if payload is not None:
            print(format_json_output(payload))
        return exit_code

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.run_build_and_post_offer_via_engine",
        _fake_run,
    )
