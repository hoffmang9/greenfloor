"""Offer lifecycle reconciliation against venue and Coinset signals."""

from __future__ import annotations

import urllib.error
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from greenfloor.adapters.coinset import extract_coinset_tx_ids_from_offer_payload
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.core.offer_lifecycle import OfferLifecycleState, OfferSignal, apply_offer_signal
from greenfloor.runtime.coin_ops.coins import safe_int
from greenfloor.storage.sqlite import SqliteStore


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
        # Dexie status alone is not sufficient evidence of a mempool take.
        # Only Coinset mempool tx signals should move an offer to
        # `mempool_observed`; otherwise preserve the current state.
        return current_state
    return current_state


@dataclass(slots=True)
class ReconcileOfferResult:
    offer_id: str
    market_id: str
    old_state: str
    new_state: str
    changed: bool
    last_seen_status: int | None
    reason: str
    taker_signal: str
    taker_diagnostic: str
    signal_source: str
    coinset_tx_ids: list[str]
    coinset_confirmed_tx_ids: list[str]
    coinset_mempool_tx_ids: list[str]


def _coinset_signal_lists(
    *,
    coinset_tx_ids: list[str],
    get_tx_signal_state: Callable[[list[str]], dict[str, dict[str, Any]]],
) -> tuple[list[str], list[str]]:
    if not coinset_tx_ids:
        return [], []
    signal_by_tx_id = get_tx_signal_state(coinset_tx_ids)
    confirmed: list[str] = []
    mempool: list[str] = []
    for tx_id in coinset_tx_ids:
        signal = signal_by_tx_id.get(tx_id, {})
        if signal.get("tx_block_confirmed_at"):
            confirmed.append(tx_id)
            continue
        if signal.get("mempool_observed_at"):
            mempool.append(tx_id)
    return confirmed, mempool


def _apply_coinset_signals(
    *,
    current_state: str,
    status: int | None,
    coinset_confirmed_tx_ids: list[str],
    coinset_mempool_tx_ids: list[str],
) -> tuple[str, str, str]:
    """Return (next_state, reason, signal_source) from Coinset signals."""
    if coinset_confirmed_tx_ids and status != 3 and current_state != "cancelled":
        transition = apply_offer_signal(
            OfferLifecycleState.OPEN,
            OfferSignal.TX_CONFIRMED,
        )
        return (
            transition.new_state.value,
            "coinset_tx_block_webhook_confirmed",
            "coinset_webhook",
        )
    if coinset_mempool_tx_ids:
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
        return next_state, "coinset_mempool_observed", "coinset_mempool"
    return current_state, "ok", "none"


def _apply_dexie_status_fallback(
    *,
    status: int | None,
    current_state: str,
    coinset_tx_ids: list[str],
    signal_source: str,
    next_state: str,
    reason: str,
) -> tuple[str, str, str]:
    if status is None:
        if not coinset_tx_ids:
            return current_state, "missing_status", signal_source
        if signal_source == "none":
            return current_state, "coinset_signal_unavailable_for_offer", signal_source
        return next_state, reason, signal_source
    if signal_source == "none":
        return (
            reconciled_state_from_dexie_status(status=status, current_state=current_state),
            reason,
            "dexie_status_fallback",
        )
    return next_state, reason, signal_source


def _taker_fields(
    *,
    coinset_confirmed_tx_ids: list[str],
    coinset_mempool_tx_ids: list[str],
    status: int | None,
    current_state: str,
    next_state: str,
) -> tuple[str, str]:
    if (
        coinset_confirmed_tx_ids
        and status != 3
        and current_state != "cancelled"
        and next_state == OfferLifecycleState.TX_BLOCK_CONFIRMED.value
    ):
        return "coinset_tx_block_webhook", "coinset_tx_block_confirmed"
    if coinset_mempool_tx_ids:
        return "none", "coinset_mempool_observed"
    if status in {4, 5}:
        return "none", "dexie_status_pattern_fallback"
    return "none", "none"


def reconcile_offer_row(
    *,
    row: dict[str, Any],
    target_venue: str,
    dexie_adapter: DexieAdapter | None,
    get_tx_signal_state: Callable[[list[str]], dict[str, dict[str, Any]]],
) -> ReconcileOfferResult:
    offer_id = str(row["offer_id"])
    market_value = str(row["market_id"])
    current_state = str(row["state"])
    coinset_tx_ids: list[str] = []
    coinset_confirmed_tx_ids: list[str] = []
    coinset_mempool_tx_ids: list[str] = []
    status: int | None = None
    reason = "ok"
    signal_source = "none"
    next_state = current_state

    if target_venue != "dexie":
        next_state = "reconcile_unsupported_venue"
        reason = f"unsupported_venue:{target_venue}"
    else:
        assert dexie_adapter is not None
        try:
            payload = dexie_adapter.get_offer(offer_id)
            status = dexie_offer_status(payload)
            coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(payload)
            coinset_confirmed_tx_ids, coinset_mempool_tx_ids = _coinset_signal_lists(
                coinset_tx_ids=coinset_tx_ids,
                get_tx_signal_state=get_tx_signal_state,
            )
            next_state, reason, signal_source = _apply_coinset_signals(
                current_state=current_state,
                status=status,
                coinset_confirmed_tx_ids=coinset_confirmed_tx_ids,
                coinset_mempool_tx_ids=coinset_mempool_tx_ids,
            )
            next_state, reason, signal_source = _apply_dexie_status_fallback(
                status=status,
                current_state=current_state,
                coinset_tx_ids=coinset_tx_ids,
                signal_source=signal_source,
                next_state=next_state,
                reason=reason,
            )
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

    changed = next_state != current_state
    taker_signal, taker_diagnostic = _taker_fields(
        coinset_confirmed_tx_ids=coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids=coinset_mempool_tx_ids,
        status=status,
        current_state=current_state,
        next_state=next_state,
    )
    return ReconcileOfferResult(
        offer_id=offer_id,
        market_id=market_value,
        old_state=current_state,
        new_state=next_state,
        changed=changed,
        last_seen_status=status,
        reason=reason,
        taker_signal=taker_signal,
        taker_diagnostic=taker_diagnostic,
        signal_source=signal_source,
        coinset_tx_ids=coinset_tx_ids,
        coinset_confirmed_tx_ids=coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids=coinset_mempool_tx_ids,
    )


@dataclass(slots=True)
class ReconcileBatchResult:
    items: list[dict[str, Any]]
    reconciled_count: int
    changed_count: int


def persist_reconcile_result(
    *,
    store: SqliteStore,
    result: ReconcileOfferResult,
    target_venue: str,
) -> None:
    store.upsert_offer_state(
        offer_id=result.offer_id,
        market_id=result.market_id,
        state=result.new_state,
        last_seen_status=result.last_seen_status,
    )
    store.add_audit_event(
        "offer_reconciliation",
        {
            "offer_id": result.offer_id,
            "market_id": result.market_id,
            "venue": target_venue,
            "old_state": result.old_state,
            "new_state": result.new_state,
            "changed": result.changed,
            "last_seen_status": result.last_seen_status,
            "reason": result.reason,
            "taker_signal": result.taker_signal,
            "taker_diagnostic": result.taker_diagnostic,
            "signal_source": result.signal_source,
            "coinset_tx_ids": result.coinset_tx_ids,
            "coinset_confirmed_tx_ids": result.coinset_confirmed_tx_ids,
            "coinset_mempool_tx_ids": result.coinset_mempool_tx_ids,
        },
        market_id=result.market_id,
    )
    if result.taker_signal != "none":
        store.add_audit_event(
            "taker_detection",
            {
                "offer_id": result.offer_id,
                "market_id": result.market_id,
                "venue": target_venue,
                "signal": result.taker_signal,
                "advisory_diagnostic": result.taker_diagnostic,
                "old_state": result.old_state,
                "new_state": result.new_state,
                "last_seen_status": result.last_seen_status,
                "signal_source": result.signal_source,
                "coinset_confirmed_tx_ids": result.coinset_confirmed_tx_ids,
            },
            market_id=result.market_id,
        )


def reconcile_offers(
    *,
    store: SqliteStore,
    dexie_api_base: str,
    target_venue: str,
    market_id: str | None,
    limit: int,
) -> ReconcileBatchResult:
    dexie_adapter = DexieAdapter(dexie_api_base) if target_venue == "dexie" else None
    rows = store.list_offer_states(market_id=market_id, limit=limit)
    items: list[dict[str, Any]] = []
    reconciled = 0
    changed = 0
    for row in rows:
        result = reconcile_offer_row(
            row=row,
            target_venue=target_venue,
            dexie_adapter=dexie_adapter,
            get_tx_signal_state=store.get_tx_signal_state,
        )
        persist_reconcile_result(store=store, result=result, target_venue=target_venue)
        reconciled += 1
        changed += int(result.changed)
        items.append(
            {
                "offer_id": result.offer_id,
                "market_id": result.market_id,
                "old_state": result.old_state,
                "new_state": result.new_state,
                "changed": result.changed,
                "last_seen_status": result.last_seen_status,
                "reason": result.reason,
                "taker_signal": result.taker_signal,
                "taker_diagnostic": result.taker_diagnostic,
                "signal_source": result.signal_source,
                "coinset_tx_ids": result.coinset_tx_ids,
                "coinset_confirmed_tx_ids": result.coinset_confirmed_tx_ids,
                "coinset_mempool_tx_ids": result.coinset_mempool_tx_ids,
            }
        )
    return ReconcileBatchResult(
        items=items,
        reconciled_count=reconciled,
        changed_count=changed,
    )
