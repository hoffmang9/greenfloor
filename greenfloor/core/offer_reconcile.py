"""Offer reconciliation transition kernel (Rust-backed)."""

from __future__ import annotations

import importlib
from dataclasses import dataclass
from typing import Any

_INSTALL_HINT = (
    "Install the greenfloor_signer extension (for example: "
    "`maturin develop -m greenfloor-signer-pyo3` from the repo root)."
)


def _import_signer() -> Any:
    try:
        return importlib.import_module("greenfloor_signer")
    except ImportError as exc:
        raise ImportError(
            f"greenfloor_signer is not available. {_INSTALL_HINT} Original error: {exc}"
        ) from exc


@dataclass(frozen=True, slots=True)
class CycleOfferTransition:
    old_state: str
    new_state: str
    reason: str
    signal_source: str
    signal: str | None
    changed: bool
    immediate_requeue: bool
    coinset_tx_ids: list[str]
    coinset_confirmed_tx_ids: list[str]
    coinset_mempool_tx_ids: list[str]

    def taker_fields(self, *, last_seen_status: int | None) -> tuple[str, str]:
        signer = _import_signer()
        return signer.offer_reconcile_taker_fields(
            self.coinset_confirmed_tx_ids,
            self.coinset_mempool_tx_ids,
            last_seen_status,
            self.old_state,
            self.new_state,
        )


def reconciled_state_from_dexie_status(*, status: int, current_state: str) -> str:
    signer = _import_signer()
    return str(
        signer.reconciled_state_from_dexie_status(int(status), str(current_state))
    )


def resolve_missing_watched_offer_transition(*, current_state: str) -> CycleOfferTransition:
    signer = _import_signer()
    result = signer.resolve_missing_watched_offer_transition(str(current_state))
    return _require_cycle_offer_transition(result)


def resolve_watched_offer_transition(
    *,
    current_state: str,
    status: int | None,
    coinset_tx_ids: list[str],
    coinset_confirmed_tx_ids: list[str],
    coinset_mempool_tx_ids: list[str],
) -> CycleOfferTransition:
    signer = _import_signer()
    result = signer.resolve_watched_offer_transition(
        str(current_state),
        status,
        list(coinset_tx_ids),
        list(coinset_confirmed_tx_ids),
        list(coinset_mempool_tx_ids),
    )
    return _require_cycle_offer_transition(result)


def _require_cycle_offer_transition(value: object) -> CycleOfferTransition:
    if not isinstance(value, CycleOfferTransition):
        raise TypeError("signer returned non-CycleOfferTransition result")
    return value
