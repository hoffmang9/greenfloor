"""PyO3 bridge for offer reconciliation (internal)."""

from __future__ import annotations

from typing import Any

from greenfloor.core.cycle._bridge import _import_signer


def resolve_missing_watched_offer_transition(current_state: str) -> Any:
    return _import_signer().resolve_missing_watched_offer_transition(str(current_state))


def resolve_watched_offer_transition_from_signals(
    *,
    current_state: str,
    status: int | None,
    coinset_tx_ids: list[str],
    coinset_confirmed_tx_ids: list[str],
    coinset_mempool_tx_ids: list[str],
) -> Any:
    return _import_signer().resolve_watched_offer_transition_from_signals(
        str(current_state),
        status,
        list(coinset_tx_ids),
        list(coinset_confirmed_tx_ids),
        list(coinset_mempool_tx_ids),
    )


def unchanged_offer_transition(current_state: str, reason: str) -> Any:
    return _import_signer().unchanged_offer_transition(str(current_state), str(reason))


def unsupported_venue_offer_transition(current_state: str, venue: str) -> Any:
    return _import_signer().unsupported_venue_offer_transition(str(current_state), str(venue))
