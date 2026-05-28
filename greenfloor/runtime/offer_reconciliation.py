"""Offer lifecycle reconciliation against venue and Coinset signals."""

from __future__ import annotations

import urllib.error
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from greenfloor.adapters.coinset import extract_coinset_tx_ids_from_offer_payload
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.core.offer_reconcile import (
    CycleOfferTransition,
    resolve_missing_watched_offer_transition,
    resolve_watched_offer_transition_from_signals,
    unchanged_offer_transition,
    unsupported_venue_offer_transition,
)
from greenfloor.runtime.coin_ops.coins import safe_int
from greenfloor.storage.sqlite import SqliteStore

OFFER_LIFECYCLE_TRANSITION_EVENT = "offer_lifecycle_transition"

__all__ = [
    "CycleOfferTransition",
    "OFFER_LIFECYCLE_TRANSITION_EVENT",
    "ReconcileBatchResult",
    "ReconcileOfferResult",
    "ReconcileOfferRowOutcome",
    "dexie_offer_status",
    "persist_offer_lifecycle_transition",
    "persist_reconcile_outcome",
    "reconcile_offer_row",
    "reconcile_offers",
    "reconcile_result_from_transition",
    "resolve_missing_watched_offer_transition",
    "resolve_watched_offer_transition",
]


def dexie_offer_status(payload: dict[str, Any]) -> int | None:
    raw_status = payload.get("status")
    if raw_status is None and isinstance(payload.get("offer"), dict):
        raw_status = payload["offer"].get("status")
    return safe_int(raw_status)


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


def resolve_watched_offer_transition(
    *,
    current_state: str,
    status: int | None,
    coinset_tx_ids: list[str],
    get_tx_signal_state: Callable[[list[str]], dict[str, dict[str, Any]]],
) -> CycleOfferTransition:
    """Resolve lifecycle state for a watched offer using canonical reconcile rules."""
    coinset_confirmed_tx_ids, coinset_mempool_tx_ids = _coinset_signal_lists(
        coinset_tx_ids=coinset_tx_ids,
        get_tx_signal_state=get_tx_signal_state,
    )
    return resolve_watched_offer_transition_from_signals(
        current_state=current_state,
        status=status,
        coinset_tx_ids=coinset_tx_ids,
        coinset_confirmed_tx_ids=coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids=coinset_mempool_tx_ids,
    )


@dataclass(frozen=True, slots=True)
class ReconcileOfferRowOutcome:
    offer_id: str
    market_id: str
    last_seen_status: int | None
    transition: CycleOfferTransition

    def to_result(self) -> ReconcileOfferResult:
        return reconcile_result_from_transition(
            offer_id=self.offer_id,
            market_id=self.market_id,
            transition=self.transition,
            last_seen_status=self.last_seen_status,
        )


def reconcile_result_from_transition(
    *,
    offer_id: str,
    market_id: str,
    transition: CycleOfferTransition,
    last_seen_status: int | None,
) -> ReconcileOfferResult:
    taker_signal, taker_diagnostic = transition.taker_fields(last_seen_status=last_seen_status)
    return ReconcileOfferResult(
        offer_id=offer_id,
        market_id=market_id,
        old_state=transition.old_state,
        new_state=transition.new_state,
        changed=transition.changed,
        last_seen_status=last_seen_status,
        reason=transition.reason,
        taker_signal=taker_signal,
        taker_diagnostic=taker_diagnostic,
        signal_source=transition.signal_source,
        coinset_tx_ids=transition.coinset_tx_ids,
        coinset_confirmed_tx_ids=transition.coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids=transition.coinset_mempool_tx_ids,
    )


def persist_offer_lifecycle_transition(
    *,
    store: SqliteStore,
    offer_id: str,
    market_id: str,
    transition: CycleOfferTransition,
    last_seen_status: int | None,
    venue: str | None = None,
    action: str | None = None,
    dexie_error: str | None = None,
) -> None:
    store.upsert_offer_state(
        offer_id=offer_id,
        market_id=market_id,
        state=transition.new_state,
        last_seen_status=last_seen_status,
    )
    taker_signal, taker_diagnostic = transition.taker_fields(last_seen_status=last_seen_status)
    payload: dict[str, Any] = {
        "offer_id": offer_id,
        "market_id": market_id,
        "old_state": transition.old_state,
        "new_state": transition.new_state,
        "changed": transition.changed,
        "reason": transition.reason,
        "signal": transition.signal,
        "signal_source": transition.signal_source,
        "last_seen_status": last_seen_status,
        "dexie_status": last_seen_status,
        "coinset_tx_ids": transition.coinset_tx_ids,
        "coinset_confirmed_tx_ids": transition.coinset_confirmed_tx_ids,
        "coinset_mempool_tx_ids": transition.coinset_mempool_tx_ids,
        "taker_signal": taker_signal,
        "taker_diagnostic": taker_diagnostic,
    }
    if venue is not None:
        payload["venue"] = venue
    if action is not None:
        payload["action"] = action
    if dexie_error is not None:
        payload["dexie_error"] = dexie_error
    store.add_audit_event(
        OFFER_LIFECYCLE_TRANSITION_EVENT,
        payload,
        market_id=market_id,
    )
    if taker_signal != "none":
        store.add_audit_event(
            "taker_detection",
            {
                "offer_id": offer_id,
                "market_id": market_id,
                "venue": venue or "dexie",
                "signal": taker_signal,
                "advisory_diagnostic": taker_diagnostic,
                "old_state": transition.old_state,
                "new_state": transition.new_state,
                "last_seen_status": last_seen_status,
                "signal_source": transition.signal_source,
                "coinset_confirmed_tx_ids": transition.coinset_confirmed_tx_ids,
            },
            market_id=market_id,
        )


def reconcile_offer_row(
    *,
    row: dict[str, Any],
    target_venue: str,
    dexie_adapter: DexieAdapter | None,
    get_tx_signal_state: Callable[[list[str]], dict[str, dict[str, Any]]],
) -> ReconcileOfferRowOutcome:
    offer_id = str(row["offer_id"])
    market_value = str(row["market_id"])
    current_state = str(row["state"])
    status: int | None = None

    if target_venue != "dexie":
        transition = unsupported_venue_offer_transition(
            current_state=current_state,
            venue=target_venue,
        )
        return ReconcileOfferRowOutcome(
            offer_id=offer_id,
            market_id=market_value,
            last_seen_status=status,
            transition=transition,
        )

    assert dexie_adapter is not None
    try:
        payload = dexie_adapter.get_offer(offer_id)
        status = dexie_offer_status(payload)
        coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(payload)
        transition = resolve_watched_offer_transition(
            current_state=current_state,
            status=status,
            coinset_tx_ids=coinset_tx_ids,
            get_tx_signal_state=get_tx_signal_state,
        )
    except urllib.error.HTTPError as exc:
        status = None
        if int(getattr(exc, "code", 0)) == 404:
            transition = resolve_missing_watched_offer_transition(current_state=current_state)
        else:
            transition = unchanged_offer_transition(
                current_state=current_state,
                reason=f"dexie_http_error:{exc.code}",
            )
    except Exception as exc:
        status = None
        transition = unchanged_offer_transition(
            current_state=current_state,
            reason=f"dexie_lookup_error:{exc}",
        )

    return ReconcileOfferRowOutcome(
        offer_id=offer_id,
        market_id=market_value,
        last_seen_status=status,
        transition=transition,
    )


@dataclass(slots=True)
class ReconcileBatchResult:
    items: list[dict[str, Any]]
    reconciled_count: int
    changed_count: int


def persist_reconcile_outcome(
    *,
    store: SqliteStore,
    outcome: ReconcileOfferRowOutcome,
    target_venue: str,
) -> None:
    persist_offer_lifecycle_transition(
        store=store,
        offer_id=outcome.offer_id,
        market_id=outcome.market_id,
        transition=outcome.transition,
        last_seen_status=outcome.last_seen_status,
        venue=target_venue,
        action="offers_reconcile",
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
        outcome = reconcile_offer_row(
            row=row,
            target_venue=target_venue,
            dexie_adapter=dexie_adapter,
            get_tx_signal_state=store.get_tx_signal_state,
        )
        persist_reconcile_outcome(store=store, outcome=outcome, target_venue=target_venue)
        result = outcome.to_result()
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
