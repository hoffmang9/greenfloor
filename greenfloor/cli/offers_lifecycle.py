"""CLI offer lifecycle commands (reconcile, status, cancel)."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from greenfloor.config.io import (
    load_markets_config_with_optional_overlay,
    load_program_config,
    resolve_market_for_build,
    resolve_state_db_path,
)
from greenfloor.runtime.cloud_wallet import adapter as cloud_wallet_adapter
from greenfloor.runtime.cloud_wallet.adapter import format_json_output
from greenfloor.runtime.cloud_wallet.coin_ops_refresh import execute_offer_onchain_refresh_split
from greenfloor.runtime.cloud_wallet.offers import (
    cloud_wallet_offer_ui_url,
    select_offers_for_cancel,
)
from greenfloor.runtime.offer_reconciliation import reconcile_offers
from greenfloor.storage.sqlite import SqliteStore


def offers_reconcile(
    *,
    program_path: Path,
    state_db: str | None,
    market_id: str | None,
    limit: int,
    venue: str | None,
) -> int:
    db_path = resolve_state_db_path(program_config_path=program_path, explicit_db_path=state_db)
    store = SqliteStore(db_path)
    try:
        program = load_program_config(program_path)
        target_venue = str(venue or program.offer_publish_venue).strip().lower()
        batch = reconcile_offers(
            store=store,
            dexie_api_base=program.dexie_api_base,
            target_venue=target_venue,
            market_id=market_id,
            limit=limit,
        )
        print(
            format_json_output(
                {
                    "state_db": str(db_path),
                    "venue": target_venue,
                    "market_id": market_id,
                    "reconciled_count": batch.reconciled_count,
                    "changed_count": batch.changed_count,
                    "items": batch.items,
                }
            )
        )
    finally:
        store.close()
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
    program = load_program_config(program_path)
    wallet = cloud_wallet_adapter.new_cloud_wallet_adapter(program)
    onchain_market = None
    if submit_onchain_after_offchain:
        if markets_path is None:
            raise ValueError("markets_path is required for submit_onchain_after_offchain")
        markets = load_markets_config_with_optional_overlay(
            path=markets_path,
            overlay_path=testnet_markets_path,
        )
        onchain_market = resolve_market_for_build(
            markets,
            market_id=onchain_market_id,
            pair=onchain_pair,
            network=program.app_network,
        )
    requested_ids = [str(value).strip() for value in offer_ids if str(value).strip()]
    selected_offers = select_offers_for_cancel(
        wallet=wallet,
        offer_ids=requested_ids,
        cancel_open=cancel_open,
    )

    items: list[dict[str, Any]] = []
    failures = 0
    for row in selected_offers:
        offer_id = row["offer_id"]
        offer_state = str(row.get("state", "")).strip().upper()
        cancel_off_chain = offer_state == "PENDING" or (
            submit_onchain_after_offchain and offer_state == "OPEN"
        )
        wallet_offer_id = row.get("wallet_offer_id", "")
        ui_url = cloud_wallet_offer_ui_url(
            cloud_wallet_base_url=str(program.cloud_wallet_base_url),
            vault_id=wallet.vault_id,
            wallet_offer_id=wallet_offer_id,
        )
        try:
            cancel_result = wallet.cancel_offer(
                offer_id=offer_id, cancel_off_chain=cancel_off_chain
            )
            item = {
                "offer_id": offer_id,
                "wallet_offer_id": wallet_offer_id,
                "state": row.get("state", ""),
                "expires_at": row.get("expires_at", ""),
                "cancel_off_chain": cancel_off_chain,
                "url": ui_url,
                "result": {
                    "success": True,
                    "signature_request_id": str(
                        cancel_result.get("signature_request_id", "")
                    ).strip(),
                    "signature_state": str(cancel_result.get("status", "")).strip(),
                },
            }
            missing_signature_request = not item["result"]["signature_request_id"]
            if missing_signature_request and not cancel_off_chain:
                failures += 1
                item["result"]["success"] = False
                item["result"]["error"] = "cancel_offer_missing_signature_request_id"
            elif missing_signature_request and cancel_off_chain:
                item["result"]["reason"] = "cancel_off_chain_requested"
            if submit_onchain_after_offchain and item["result"]["success"]:
                if not cancel_off_chain:
                    item["result"]["onchain_refresh"] = {
                        "status": "skipped",
                        "reason": "requires_off_chain_cancel_state_pending",
                        "signature_request_id": None,
                        "signature_state": "",
                    }
                else:
                    assert onchain_market is not None
                    refresh = execute_offer_onchain_refresh_split(
                        wallet=wallet,
                        market=onchain_market,
                        program=program,
                        offer_bech32=str(row.get("bech32", "")).strip(),
                    )
                    item["result"]["onchain_refresh"] = refresh
                    if not refresh.get("signature_request_id"):
                        failures += 1
                        item["result"]["success"] = False
                        item["result"]["error"] = (
                            "onchain_refresh_failed:missing_signature_request_id"
                        )
            items.append(item)
        except Exception as exc:
            failures += 1
            items.append(
                {
                    "offer_id": offer_id,
                    "wallet_offer_id": wallet_offer_id,
                    "state": row.get("state", ""),
                    "expires_at": row.get("expires_at", ""),
                    "url": ui_url,
                    "result": {
                        "success": False,
                        "error": str(exc),
                    },
                }
            )
    print(
        format_json_output(
            {
                "vault_id": wallet.vault_id,
                "cancel_open": bool(cancel_open),
                "requested_offer_ids": requested_ids,
                "submit_onchain_after_offchain": bool(submit_onchain_after_offchain),
                "onchain_market_id": (
                    onchain_market.market_id if onchain_market is not None else ""
                ),
                "selected_count": len(selected_offers),
                "cancelled_count": len(selected_offers) - failures,
                "failed_count": failures,
                "items": items,
            }
        )
    )
    return 0 if failures == 0 else 2
