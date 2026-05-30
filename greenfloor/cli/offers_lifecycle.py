"""CLI offer lifecycle commands (reconcile, status, cancel)."""

from __future__ import annotations

from pathlib import Path

from greenfloor.config.io import load_program_config, resolve_state_db_path
from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.runtime.json_output import format_json_output


def _engine():
    return import_engine()


def offers_reconcile(
    *,
    program_path: Path,
    state_db: str | None,
    market_id: str | None,
    limit: int,
    venue: str | None,
) -> int:
    program = load_program_config(program_path)
    db_path = resolve_state_db_path(program_config_path=program_path, explicit_db_path=state_db)
    target_venue = str(venue or program.offer_publish_venue).strip().lower()
    reconcile_fn = require_engine_method(
        _engine(),
        "reconcile_offers_cli",
        missing="offer reconcile cli",
    )
    payload = reconcile_fn(
        str(db_path),
        program.dexie_api_base,
        target_venue,
        market_id.strip() if market_id and market_id.strip() else None,
        int(limit),
    )
    print(format_json_output(dict(payload)))
    return 0


def offers_status(
    *,
    program_path: Path,
    state_db: str | None,
    market_id: str | None,
    limit: int,
    events_limit: int,
) -> int:
    db_path = resolve_state_db_path(program_config_path=program_path, explicit_db_path=state_db)
    status_fn = require_engine_method(
        _engine(),
        "offers_status_cli",
        missing="offer status cli",
    )
    payload = status_fn(
        str(db_path),
        market_id.strip() if market_id and market_id.strip() else None,
        int(limit),
        int(events_limit),
    )
    print(format_json_output(dict(payload)))
    return 0


def offers_cancel(
    *,
    program_path: Path,
    offer_ids: list[str],
    cancel_open: bool,
    markets_path: Path | None = None,
    testnet_markets_path: Path | None = None,
    submit_onchain_after_offchain: bool = False,
    onchain_market_id: str | None = None,
    onchain_pair: str | None = None,
) -> int:
    _ = (
        markets_path,
        testnet_markets_path,
        onchain_market_id,
        onchain_pair,
    )
    if submit_onchain_after_offchain:
        raise ValueError(
            "submit_onchain_after_offchain is removed with Cloud Wallet; cancel via Dexie only"
        )
    program = load_program_config(program_path)
    db_path = resolve_state_db_path(program_config_path=program_path, explicit_db_path=None)
    target_venue = str(program.offer_publish_venue).strip().lower()
    cancel_fn = require_engine_method(
        _engine(),
        "offers_cancel_cli",
        missing="offer cancel cli",
    )
    requested_ids = [str(value).strip() for value in offer_ids if str(value).strip()]
    payload = cancel_fn(
        str(db_path),
        program.dexie_api_base,
        target_venue,
        requested_ids,
        bool(cancel_open),
    )
    payload_dict = dict(payload)
    print(format_json_output(payload_dict))
    failures = int(payload_dict.get("failed_count", 0))
    return 0 if failures == 0 else 2
