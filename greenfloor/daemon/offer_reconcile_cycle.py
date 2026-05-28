"""Per-cycle Dexie offer fetch, watchlist refresh, and lifecycle transitions."""

from __future__ import annotations

from datetime import datetime
from typing import Any, Protocol

from greenfloor.adapters.coinset import extract_coinset_tx_ids_from_offer_payload
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.config.io import resolve_trade_asset_for_dexie
from greenfloor.core.offer_reconcile import (
    CycleOfferTransition,
    resolve_missing_watched_offer_transition,
)
from greenfloor.daemon.market_helpers import _resolve_quote_asset_for_offer
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.daemon.watchlist import (
    _build_dexie_size_by_offer_id,
    _is_dexie_offer_missing_error,
    _update_market_coin_watchlist_from_dexie,
    _watchlist_offer_ids_from_store,
)
from greenfloor.runtime.offer_reconciliation import (
    persist_offer_lifecycle_transition,
    resolve_watched_offer_transition,
)
from greenfloor.storage.sqlite import SqliteStore


class _CycleReconcileResult(Protocol):
    cycle_errors: int
    immediate_requeue_requested: bool
    immediate_requeue_signals: list[str]


def _apply_cycle_transition(
    *,
    store: SqliteStore,
    market: Any,
    offer_id: str,
    transition: CycleOfferTransition,
    result: _CycleReconcileResult,
    state_by_offer_id: dict[str, str],
    last_seen_status: int | None,
    dexie_status: int | None = None,
    dexie_error: str | None = None,
) -> None:
    _log_market_decision(
        market.market_id,
        "offer_transition",
        offer_id=offer_id,
        dexie_status=dexie_status,
        signal_source=transition.signal_source,
        old_state=transition.old_state,
        new_state=transition.new_state,
        signal=transition.signal,
    )
    persist_offer_lifecycle_transition(
        store=store,
        offer_id=offer_id,
        market_id=market.market_id,
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


def reconcile_market_cycle_offers(
    *,
    market: Any,
    network: str,
    dexie: DexieAdapter,
    store: SqliteStore,
    now: datetime,
    result: _CycleReconcileResult,
) -> tuple[list[dict[str, Any]], dict[str, int], str | None, list[dict[str, Any]]]:
    """Fetch Dexie offers, augment beyond-cap offers, and transition lifecycle states.

    Returns (augmented_offers, dexie_size_by_offer_id, dexie_fetch_error, offers).
    offers is the raw Dexie list (used by cancel policy); augmented_offers includes
    beyond-cap individually-fetched offers.
    """
    dexie_fetch_error: str | None = None
    dexie_offered_asset = resolve_trade_asset_for_dexie(
        asset=str(market.base_asset),
        network=network,
    )
    dexie_requested_asset = _resolve_quote_asset_for_offer(
        quote_asset=str(market.quote_asset),
        network=network,
    )
    try:
        offers = dexie.get_offers(dexie_offered_asset, dexie_requested_asset)
        _log_market_decision(
            market.market_id,
            "dexie_offers_fetched",
            offered=dexie_offered_asset,
            requested=dexie_requested_asset,
            count=len(offers),
        )
    except Exception as exc:  # pragma: no cover - network dependent
        dexie_fetch_error = str(exc)
        result.cycle_errors += 1
        _log_market_decision(
            market.market_id,
            "dexie_offers_error",
            error=str(exc),
        )
        store.add_audit_event(
            "dexie_offers_error",
            {"market_id": market.market_id, "error": str(exc)},
            market_id=market.market_id,
        )
        offers = []
    our_offer_ids = _watchlist_offer_ids_from_store(
        store=store,
        market_id=market.market_id,
        clock=now,
    )
    state_by_offer_id = {
        str(row["offer_id"]): str(row["state"])
        for row in store.list_offer_states(market_id=market.market_id, limit=5000)
    }
    dexie_offer_ids_in_list = {str(o.get("id", "")).strip() for o in offers if o.get("id")}
    beyond_cap_ids = our_offer_ids - dexie_offer_ids_in_list
    augmented_offers = list(offers)
    augmented_by_id: dict[str, dict[str, Any]] = {}
    for offer in augmented_offers:
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
            if _is_dexie_offer_missing_error(exc):
                current_state = state_by_offer_id.get(watched_offer_id, "open")
                transition = resolve_missing_watched_offer_transition(
                    current_state=current_state,
                )
                missing_watched_offer_ids.add(watched_offer_id)
                _apply_cycle_transition(
                    store=store,
                    market=market,
                    offer_id=watched_offer_id,
                    transition=transition,
                    result=result,
                    state_by_offer_id=state_by_offer_id,
                    last_seen_status=None,
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
    dexie_size_by_offer_id: dict[str, int] = _build_dexie_size_by_offer_id(
        augmented_offers, str(market.base_asset)
    )
    if dexie_fetch_error is None:
        _update_market_coin_watchlist_from_dexie(
            market=market,
            offers=augmented_offers,
            store=store,
            clock=now,
        )
    for offer in augmented_offers:
        offer_id = str(offer.get("id", ""))
        if not offer_id:
            continue
        if offer_id not in our_offer_ids:
            continue
        raw_status = offer.get("status", -1)
        status = int(raw_status) if raw_status is not None else None
        current_state = state_by_offer_id.get(offer_id, "open")
        coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(offer)
        transition = resolve_watched_offer_transition(
            current_state=current_state,
            status=status,
            coinset_tx_ids=coinset_tx_ids,
            get_tx_signal_state=store.get_tx_signal_state,
        )
        _apply_cycle_transition(
            store=store,
            market=market,
            offer_id=offer_id,
            transition=transition,
            result=result,
            state_by_offer_id=state_by_offer_id,
            last_seen_status=status,
            dexie_status=status,
        )
    return augmented_offers, dexie_size_by_offer_id, dexie_fetch_error, offers
