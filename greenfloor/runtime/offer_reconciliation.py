"""Offer lifecycle reconciliation against venue and Coinset signals."""

from __future__ import annotations

import urllib.error
from collections.abc import Callable
from dataclasses import dataclass
from datetime import datetime
from typing import Any, Protocol

from greenfloor.adapters.coinset import extract_coinset_tx_ids_from_offer_payload
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.config.io import resolve_trade_asset_for_dexie
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
    "OFFER_LIFECYCLE_TRANSITION_EVENT",
    "MarketWatchedOffersReconcileResult",
    "ReconcileBatchResult",
    "ReconcileCycleEffects",
    "ReconcileOfferResult",
    "ReconcileOfferRowOutcome",
    "dexie_offer_status",
    "persist_offer_lifecycle_transition",
    "persist_reconcile_outcome",
    "reconcile_market_watched_offers",
    "reconcile_offer_row",
    "reconcile_offers",
    "reconcile_result_from_transition",
    "resolve_watched_offer_transition",
    "transition_from_dexie_offer_payload",
]


class ReconcileCycleEffects(Protocol):
    cycle_errors: int
    immediate_requeue_requested: bool
    immediate_requeue_signals: list[str]


@dataclass(slots=True)
class MarketWatchedOffersReconcileResult:
    augmented_offers: list[dict[str, Any]]
    dexie_size_by_offer_id: dict[str, int]
    dexie_fetch_error: str | None
    offers: list[dict[str, Any]]


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


def transition_from_dexie_offer_payload(
    *,
    current_state: str,
    offer_payload: dict[str, Any],
    get_tx_signal_state: Callable[[list[str]], dict[str, dict[str, Any]]],
) -> CycleOfferTransition:
    """Resolve lifecycle transition from a Dexie offer dict (bulk list or single fetch)."""
    status = dexie_offer_status(offer_payload)
    coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(offer_payload)
    return resolve_watched_offer_transition(
        current_state=current_state,
        status=status,
        coinset_tx_ids=coinset_tx_ids,
        get_tx_signal_state=get_tx_signal_state,
    )


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
    return ReconcileOfferResult(
        offer_id=offer_id,
        market_id=market_id,
        old_state=transition.old_state,
        new_state=transition.new_state,
        changed=transition.changed,
        last_seen_status=last_seen_status,
        reason=transition.reason,
        taker_signal=transition.taker_signal,
        taker_diagnostic=transition.taker_diagnostic,
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
        "taker_signal": transition.taker_signal,
        "taker_diagnostic": transition.taker_diagnostic,
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
    if transition.taker_signal != "none":
        store.add_audit_event(
            "taker_detection",
            {
                "offer_id": offer_id,
                "market_id": market_id,
                "venue": venue or "dexie",
                "signal": transition.taker_signal,
                "advisory_diagnostic": transition.taker_diagnostic,
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
        transition = transition_from_dexie_offer_payload(
            current_state=current_state,
            offer_payload=payload,
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


def _apply_reconcile_transition(
    *,
    store: SqliteStore,
    market_id: str,
    offer_id: str,
    transition: CycleOfferTransition,
    result: ReconcileCycleEffects,
    state_by_offer_id: dict[str, str],
    last_seen_status: int | None,
    on_transition: Callable[..., None] | None = None,
    dexie_status: int | None = None,
    dexie_error: str | None = None,
) -> None:
    if on_transition is not None:
        on_transition(
            offer_id=offer_id,
            transition=transition,
            dexie_status=dexie_status,
            dexie_error=dexie_error,
        )
    persist_offer_lifecycle_transition(
        store=store,
        offer_id=offer_id,
        market_id=market_id,
        transition=transition,
        last_seen_status=last_seen_status,
        action="reconcile_coins_and_offers",
        dexie_error=dexie_error,
    )
    if transition.changed:
        state_by_offer_id[offer_id] = transition.new_state
    if transition.immediate_requeue:
        result.immediate_requeue_requested = True
        if transition.signal is not None:
            result.immediate_requeue_signals.append(transition.signal)


def reconcile_market_watched_offers(
    *,
    market: Any,
    network: str,
    dexie: DexieAdapter,
    store: SqliteStore,
    now: datetime,
    result: ReconcileCycleEffects,
    resolve_quote_asset: Callable[..., str],
    watchlist_offer_ids: Callable[..., set[str]],
    is_dexie_offer_missing: Callable[[Exception], bool],
    build_dexie_size_map: Callable[..., dict[str, int]],
    update_watchlist_from_dexie: Callable[..., None],
    on_decision: Callable[..., None] | None = None,
    on_transition: Callable[..., None] | None = None,
) -> MarketWatchedOffersReconcileResult:
    """Fetch Dexie offers for a market, augment beyond-cap watched offers, and transition states."""
    dexie_fetch_error: str | None = None
    market_id = str(market.market_id)
    dexie_offered_asset = resolve_trade_asset_for_dexie(
        asset=str(market.base_asset),
        network=network,
    )
    dexie_requested_asset = resolve_quote_asset(
        quote_asset=str(market.quote_asset),
        network=network,
    )
    try:
        offers = dexie.get_offers(dexie_offered_asset, dexie_requested_asset)
        if on_decision is not None:
            on_decision(
                "dexie_offers_fetched",
                offered=dexie_offered_asset,
                requested=dexie_requested_asset,
                count=len(offers),
            )
    except Exception as exc:  # pragma: no cover - network dependent
        dexie_fetch_error = str(exc)
        result.cycle_errors += 1
        if on_decision is not None:
            on_decision("dexie_offers_error", error=str(exc))
        store.add_audit_event(
            "dexie_offers_error",
            {"market_id": market_id, "error": str(exc)},
            market_id=market_id,
        )
        offers = []

    our_offer_ids = watchlist_offer_ids(store=store, market_id=market_id, clock=now)
    state_by_offer_id = {
        str(row["offer_id"]): str(row["state"])
        for row in store.list_offer_states(market_id=market_id, limit=5000)
    }
    dexie_offer_ids_in_list = {str(o.get("id", "")).strip() for o in offers if o.get("id")}
    beyond_cap_ids = our_offer_ids - dexie_offer_ids_in_list
    augmented_by_id: dict[str, dict[str, Any]] = {}
    for offer in offers:
        if not isinstance(offer, dict):
            continue
        offer_id = str(offer.get("id", "")).strip()
        if not offer_id:
            continue
        augmented_by_id[offer_id] = offer

    missing_watched_offer_ids: set[str] = set()
    for watched_offer_id in sorted(our_offer_ids):
        try:
            single_payload = dexie.get_offer(watched_offer_id, timeout=5)
            single_offer = single_payload.get("offer") if isinstance(single_payload, dict) else None
            if isinstance(single_offer, dict):
                augmented_by_id[watched_offer_id] = single_offer
        except Exception as exc:  # pragma: no cover - network dependent
            if is_dexie_offer_missing(exc):
                current_state = state_by_offer_id.get(watched_offer_id, "open")
                transition = resolve_missing_watched_offer_transition(
                    current_state=current_state,
                )
                missing_watched_offer_ids.add(watched_offer_id)
                _apply_reconcile_transition(
                    store=store,
                    market_id=market_id,
                    offer_id=watched_offer_id,
                    transition=transition,
                    result=result,
                    state_by_offer_id=state_by_offer_id,
                    last_seen_status=None,
                    on_transition=on_transition,
                    dexie_error=str(exc),
                )
            continue

    for beyond_offer_id in beyond_cap_ids - missing_watched_offer_ids:
        try:
            single_payload = dexie.get_offer(beyond_offer_id, timeout=5)
            single_offer = single_payload.get("offer") if isinstance(single_payload, dict) else None
            if isinstance(single_offer, dict):
                augmented_by_id[beyond_offer_id] = single_offer
        except Exception:  # pragma: no cover - network dependent
            pass

    augmented_offers = list(augmented_by_id.values())
    dexie_size_by_offer_id = build_dexie_size_map(augmented_offers, str(market.base_asset))
    if dexie_fetch_error is None:
        update_watchlist_from_dexie(
            market=market,
            offers=augmented_offers,
            store=store,
            clock=now,
        )

    for offer in augmented_offers:
        offer_id = str(offer.get("id", ""))
        if not offer_id or offer_id not in our_offer_ids:
            continue
        raw_status = offer.get("status", -1)
        status = int(raw_status) if raw_status is not None else None
        current_state = state_by_offer_id.get(offer_id, "open")
        transition = transition_from_dexie_offer_payload(
            current_state=current_state,
            offer_payload=offer,
            get_tx_signal_state=store.get_tx_signal_state,
        )
        _apply_reconcile_transition(
            store=store,
            market_id=market_id,
            offer_id=offer_id,
            transition=transition,
            result=result,
            state_by_offer_id=state_by_offer_id,
            last_seen_status=status,
            on_transition=on_transition,
            dexie_status=status,
        )

    return MarketWatchedOffersReconcileResult(
        augmented_offers=augmented_offers,
        dexie_size_by_offer_id=dexie_size_by_offer_id,
        dexie_fetch_error=dexie_fetch_error,
        offers=offers,
    )
