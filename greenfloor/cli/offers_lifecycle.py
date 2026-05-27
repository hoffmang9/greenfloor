"""CLI offer lifecycle commands (reconcile, status, cancel)."""

from __future__ import annotations

import urllib.error
import urllib.parse
from pathlib import Path
from typing import Any

from greenfloor.adapters.coinset import extract_coinset_tx_ids_from_offer_payload
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.cli.manager_setup import resolve_db_path
from greenfloor.cli.offer_build_post import resolve_market_for_build
from greenfloor.config.io import (
    load_markets_config_with_optional_overlay,
    load_program_config,
)
from greenfloor.core.offer_lifecycle import OfferLifecycleState, OfferSignal, apply_offer_signal
from greenfloor.offer_decode import extract_coin_id_hints_from_offer_text
from greenfloor.runtime import coinset_runtime
from greenfloor.runtime.cloud_wallet import adapter as cloud_wallet_adapter
from greenfloor.runtime.cloud_wallet import assets as cloud_wallet_assets
from greenfloor.runtime.cloud_wallet.adapter import (
    _format_json_output as format_json_output,
)
from greenfloor.runtime.cloud_wallet.coins import (
    is_spendable_coin,
    resolve_coin_global_ids,
    safe_int,
)
from greenfloor.storage.sqlite import SqliteStore


def resolve_cloud_wallet_asset_id_for_wallet(
    *,
    wallet,
    canonical_asset_id: str,
    symbol_hint: str | None = None,
    program_home_dir: str | None = None,
) -> str:
    return cloud_wallet_assets.resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=canonical_asset_id,
        symbol_hint=symbol_hint,
        program_home_dir=program_home_dir,
    )


def dexie_offer_status(payload: dict[str, Any]) -> int | None:
    raw_status = payload.get("status")
    if raw_status is None and isinstance(payload.get("offer"), dict):
        raw_status = payload["offer"].get("status")
    return safe_int(raw_status)


def reconciled_state_from_dexie_status(
    *,
    status: int,
    current_state: str,
) -> str:
    if status == 4:
        transition = apply_offer_signal(
            OfferLifecycleState.OPEN,
            OfferSignal.TX_CONFIRMED,
        )
        return transition.new_state.value
    if status == 6:
        transition = apply_offer_signal(
            OfferLifecycleState.OPEN,
            OfferSignal.EXPIRED,
        )
        return transition.new_state.value
    if status == 3:
        return "cancelled"
    if status in {0, 1, 2, 5}:
        if current_state in {
            OfferLifecycleState.TX_BLOCK_CONFIRMED.value,
            OfferLifecycleState.EXPIRED.value,
            "cancelled",
        }:
            return current_state
        # Dexie status alone is not sufficient evidence of a mempool take.
        # Only Coinset mempool tx signals should move an offer to
        # `mempool_observed`; otherwise preserve the current state.
        return current_state
    # Preserve state for unrecognized Dexie statuses instead of creating an
    # orphan classification.
    return current_state


def offers_reconcile(
    *,
    program_path: Path,
    state_db: str | None,
    market_id: str | None,
    limit: int,
    venue: str | None,
) -> int:
    db_path = resolve_db_path(program_path, state_db)
    store = SqliteStore(db_path)
    try:
        program = load_program_config(program_path)
        target_venue = str(venue or program.offer_publish_venue).strip().lower()
        rows = store.list_offer_states(market_id=market_id, limit=limit)
        items: list[dict] = []
        reconciled = 0
        changed = 0
        for row in rows:
            offer_id = str(row["offer_id"])
            market_value = str(row["market_id"])
            current_state = str(row["state"])
            taker_signal = "none"
            taker_diagnostic = "none"
            signal_source = "none"
            coinset_tx_ids: list[str] = []
            coinset_confirmed_tx_ids: list[str] = []
            coinset_mempool_tx_ids: list[str] = []
            if target_venue != "dexie":
                next_state = "reconcile_unsupported_venue"
                reason = f"unsupported_venue:{target_venue}"
                status = None
                changed_flag = next_state != current_state
            else:
                adapter = DexieAdapter(program.dexie_api_base)
                status: int | None
                reason = "ok"
                try:
                    payload = adapter.get_offer(offer_id)
                    status = dexie_offer_status(payload)
                    coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(payload)
                    if coinset_tx_ids:
                        signal_by_tx_id = store.get_tx_signal_state(coinset_tx_ids)
                        for tx_id in coinset_tx_ids:
                            signal = signal_by_tx_id.get(tx_id, {})
                            if signal.get("tx_block_confirmed_at"):
                                coinset_confirmed_tx_ids.append(tx_id)
                                continue
                            if signal.get("mempool_observed_at"):
                                coinset_mempool_tx_ids.append(tx_id)
                    if coinset_confirmed_tx_ids and status != 3 and current_state != "cancelled":
                        transition = apply_offer_signal(
                            OfferLifecycleState.OPEN,
                            OfferSignal.TX_CONFIRMED,
                        )
                        next_state = transition.new_state.value
                        reason = "coinset_tx_block_webhook_confirmed"
                        signal_source = "coinset_webhook"
                    elif coinset_mempool_tx_ids:
                        if current_state in {
                            OfferLifecycleState.TX_BLOCK_CONFIRMED.value,
                            OfferLifecycleState.EXPIRED.value,
                            "cancelled",
                        }:
                            next_state = current_state
                        else:
                            transition = apply_offer_signal(
                                OfferLifecycleState.OPEN,
                                OfferSignal.MEMPOOL_SEEN,
                            )
                            next_state = transition.new_state.value
                        reason = "coinset_mempool_observed"
                        signal_source = "coinset_mempool"
                    if status is None:
                        if not coinset_tx_ids:
                            next_state = current_state
                            reason = "missing_status"
                        elif signal_source == "none":
                            next_state = current_state
                            reason = "coinset_signal_unavailable_for_offer"
                    else:
                        if signal_source == "none":
                            next_state = reconciled_state_from_dexie_status(
                                status=status,
                                current_state=current_state,
                            )
                            signal_source = "dexie_status_fallback"
                except urllib.error.HTTPError as exc:
                    status = None
                    if int(getattr(exc, "code", 0)) == 404:
                        transition = apply_offer_signal(
                            OfferLifecycleState.OPEN,
                            OfferSignal.EXPIRED,
                        )
                        if current_state in {
                            OfferLifecycleState.TX_BLOCK_CONFIRMED.value,
                            OfferLifecycleState.EXPIRED.value,
                            "cancelled",
                        }:
                            next_state = current_state
                        else:
                            next_state = transition.new_state.value
                        reason = "dexie_offer_not_found"
                    else:
                        next_state = current_state
                        reason = f"dexie_http_error:{exc.code}"
                except Exception as exc:
                    status = None
                    next_state = current_state
                    reason = f"dexie_lookup_error:{exc}"
                changed_flag = next_state != current_state
            if (
                coinset_confirmed_tx_ids
                and status != 3
                and current_state != "cancelled"
                and next_state == OfferLifecycleState.TX_BLOCK_CONFIRMED.value
            ):
                taker_signal = "coinset_tx_block_webhook"
                taker_diagnostic = "coinset_tx_block_confirmed"
            elif coinset_mempool_tx_ids:
                taker_diagnostic = "coinset_mempool_observed"
            elif status in {4, 5}:
                taker_diagnostic = "dexie_status_pattern_fallback"
            store.upsert_offer_state(
                offer_id=offer_id,
                market_id=market_value,
                state=next_state,
                last_seen_status=status,
            )
            store.add_audit_event(
                "offer_reconciliation",
                {
                    "offer_id": offer_id,
                    "market_id": market_value,
                    "venue": target_venue,
                    "old_state": current_state,
                    "new_state": next_state,
                    "changed": changed_flag,
                    "last_seen_status": status,
                    "reason": reason,
                    "taker_signal": taker_signal,
                    "taker_diagnostic": taker_diagnostic,
                    "signal_source": signal_source,
                    "coinset_tx_ids": coinset_tx_ids,
                    "coinset_confirmed_tx_ids": coinset_confirmed_tx_ids,
                    "coinset_mempool_tx_ids": coinset_mempool_tx_ids,
                },
                market_id=market_value,
            )
            if taker_signal != "none":
                store.add_audit_event(
                    "taker_detection",
                    {
                        "offer_id": offer_id,
                        "market_id": market_value,
                        "venue": target_venue,
                        "signal": taker_signal,
                        "advisory_diagnostic": taker_diagnostic,
                        "old_state": current_state,
                        "new_state": next_state,
                        "last_seen_status": status,
                        "signal_source": signal_source,
                        "coinset_confirmed_tx_ids": coinset_confirmed_tx_ids,
                    },
                    market_id=market_value,
                )
            reconciled += 1
            changed += int(changed_flag)
            items.append(
                {
                    "offer_id": offer_id,
                    "market_id": market_value,
                    "old_state": current_state,
                    "new_state": next_state,
                    "changed": changed_flag,
                    "last_seen_status": status,
                    "reason": reason,
                    "taker_signal": taker_signal,
                    "taker_diagnostic": taker_diagnostic,
                    "signal_source": signal_source,
                    "coinset_tx_ids": coinset_tx_ids,
                    "coinset_confirmed_tx_ids": coinset_confirmed_tx_ids,
                    "coinset_mempool_tx_ids": coinset_mempool_tx_ids,
                }
            )
        print(
            format_json_output(
                {
                    "state_db": str(db_path),
                    "venue": target_venue,
                    "market_id": market_id,
                    "reconciled_count": reconciled,
                    "changed_count": changed,
                    "items": items,
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
    db_path = resolve_db_path(program_path, state_db)
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


def cloud_wallet_offer_ui_url(
    *, cloud_wallet_base_url: str, vault_id: str, wallet_offer_id: str
) -> str:
    raw = str(cloud_wallet_base_url).strip()
    if not raw:
        return ""
    parsed = urllib.parse.urlparse(raw)
    if not parsed.scheme or not parsed.netloc:
        return ""
    host = parsed.netloc
    if host.startswith("api."):
        host = host[4:]
    base = f"{parsed.scheme}://{host}"
    clean_vault = str(vault_id).strip()
    clean_offer = str(wallet_offer_id).strip()
    if not clean_vault or not clean_offer:
        return ""
    return f"{base}/wallet/{clean_vault}/offers/{clean_offer}"


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
    selected_offers: list[dict[str, str]] = []
    wallet_payload = wallet.get_wallet()
    offers = wallet_payload.get("offers", [])
    for row in offers if isinstance(offers, list) else []:
        if not isinstance(row, dict):
            continue
        selected_offers.append(
            {
                "wallet_offer_id": str(row.get("id", "")).strip(),
                "offer_id": str(row.get("offerId", "")).strip(),
                "state": str(row.get("state", "")).strip(),
                "expires_at": str(row.get("expiresAt", "")).strip(),
                "bech32": str(row.get("bech32", "")).strip(),
            }
        )
    selected_offers = [row for row in selected_offers if row["offer_id"]]
    if cancel_open:
        selected_offers = [
            row for row in selected_offers if str(row.get("state", "")).upper() == "OPEN"
        ]
    elif requested_ids:
        requested_set = set(requested_ids)
        selected_offers = [row for row in selected_offers if row["offer_id"] in requested_set]
    else:
        raise ValueError("provide at least one --offer-id or pass --cancel-open")

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
                    resolved_asset_id = resolve_cloud_wallet_asset_id_for_wallet(
                        wallet=wallet,
                        canonical_asset_id=onchain_market.base_asset,  # type: ignore[union-attr]
                        symbol_hint=onchain_market.base_symbol,  # type: ignore[union-attr]
                        program_home_dir=str(program.home_dir),
                    )
                    market_coins = wallet.list_coins(
                        asset_id=resolved_asset_id,
                        include_pending=True,
                    )
                    spendable_market_coins = [
                        coin for coin in market_coins if is_spendable_coin(coin)
                    ]
                    if not spendable_market_coins:
                        raise RuntimeError("no_spendable_market_coins_for_onchain_refresh")
                    coin_id_hints = extract_coin_id_hints_from_offer_text(
                        str(row.get("bech32", "")).strip()
                    )
                    resolved_coin_ids, _ = resolve_coin_global_ids(
                        spendable_market_coins, coin_id_hints
                    )
                    target_coin: dict[str, Any] | None = None
                    if resolved_coin_ids:
                        for coin in spendable_market_coins:
                            if str(coin.get("id", "")).strip() == resolved_coin_ids[0]:
                                target_coin = coin
                                break
                    if target_coin is None:
                        target_coin = sorted(
                            spendable_market_coins,
                            key=lambda c: int(c.get("amount", 0)),
                        )[0]
                    refresh_fee_mojos, refresh_fee_source = (
                        coinset_runtime._resolve_taker_or_coin_operation_fee(
                            network=program.app_network,
                            minimum_fee_mojos=0,
                        )
                    )
                    refresh_result = wallet.split_coins(
                        coin_ids=[str(target_coin.get("id", "")).strip()],
                        amount_per_coin=int(target_coin.get("amount", 0)),
                        number_of_coins=1,
                        fee=int(refresh_fee_mojos),
                    )
                    refresh_signature_request_id = str(
                        refresh_result.get("signature_request_id", "")
                    ).strip()
                    item["result"]["onchain_refresh"] = {
                        "status": ("executed" if refresh_signature_request_id else "skipped"),
                        "reason": (
                            "cloud_wallet_split_submitted"
                            if refresh_signature_request_id
                            else "missing_signature_request_id"
                        ),
                        "signature_request_id": refresh_signature_request_id or None,
                        "signature_state": str(refresh_result.get("status", "")).strip(),
                        "coin_id": str(target_coin.get("id", "")).strip(),
                        "coin_name": str(target_coin.get("name", "")).strip(),
                        "amount": int(target_coin.get("amount", 0)),
                        "asset_id": resolved_asset_id,
                        "fee_mojos": int(refresh_fee_mojos),
                        "fee_source": refresh_fee_source,
                    }
                    if not refresh_signature_request_id:
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
