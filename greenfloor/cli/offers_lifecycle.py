"""CLI offer lifecycle commands (reconcile, status, cancel)."""

from __future__ import annotations

from pathlib import Path

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.config.io import load_program_config, resolve_state_db_path
from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.runtime.json_output import format_json_output
from greenfloor.runtime.offers_cancel import cancel_offers_on_venue, select_offers_for_cancel
from greenfloor.storage.sqlite import SqliteStore


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
        import_engine(),
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
    store = SqliteStore(db_path)
    try:
        offers = store.list_offer_states(market_id=market_id, limit=limit)
        events = store.list_recent_audit_events(
            event_types=[
                "strategy_offer_execution",
                "offer_cancel_policy",
                "offer_lifecycle_transition",
                "offer_reconciliation",
                "taker_detection",
                "dexie_offers_error",
            ],
            market_id=market_id,
            limit=events_limit,
        )
    finally:
        store.close()
    by_state: dict[str, int] = {}
    for row in offers:
        by_state[row["state"]] = by_state.get(row["state"], 0) + 1
    print(
        format_json_output(
            {
                "state_db": str(db_path),
                "market_id": market_id,
                "offer_count": len(offers),
                "by_state": by_state,
                "offers": offers,
                "recent_events": events,
            }
        )
    )
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
        submit_onchain_after_offchain,
        onchain_market_id,
        onchain_pair,
    )
    if submit_onchain_after_offchain:
        raise ValueError(
            "submit_onchain_after_offchain is removed with Cloud Wallet; cancel via Dexie only"
        )
    program = load_program_config(program_path)
    db_path = resolve_state_db_path(program_config_path=program_path, explicit_db_path=None)
    store = SqliteStore(db_path)
    dexie = DexieAdapter(program.dexie_api_base)
    try:
        requested_ids = [str(value).strip() for value in offer_ids if str(value).strip()]
        selected_offers = select_offers_for_cancel(
            store=store,
            offer_ids=requested_ids,
            cancel_open=cancel_open,
        )
        items, failures = cancel_offers_on_venue(
            dexie=dexie,
            store=store,
            selected_offers=selected_offers,
        )
    finally:
        store.close()
    print(
        format_json_output(
            {
                "venue": program.offer_publish_venue,
                "cancel_open": bool(cancel_open),
                "requested_offer_ids": requested_ids,
                "selected_count": len(selected_offers),
                "cancelled_count": len(selected_offers) - failures,
                "failed_count": failures,
                "items": items,
            }
        )
    )
    return 0 if failures == 0 else 2
