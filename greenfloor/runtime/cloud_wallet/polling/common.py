from __future__ import annotations

import datetime as dt
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter


def parse_iso8601(value: str) -> dt.datetime | None:
    raw = value.strip()
    if not raw:
        return None
    normalized = raw.replace("Z", "+00:00")
    try:
        parsed = dt.datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=dt.UTC)
    return parsed.astimezone(dt.UTC)


def offer_markers(offers: list[dict]) -> set[str]:
    markers: set[str] = set()
    for offer in offers:
        offer_id = str(offer.get("offerId", "")).strip()
        if offer_id:
            markers.add(f"id:{offer_id}")
        bech32 = str(offer.get("bech32", "")).strip()
        if bech32:
            markers.add(f"bech32:{bech32}")
    return markers


def pick_new_offer_artifact(
    *,
    offers: list[dict],
    known_markers: set[str],
    min_created_at: dt.datetime | None = None,
    require_open_state: bool = False,
    prefer_newest: bool = True,
) -> str:
    candidates: list[tuple[dt.datetime, dt.datetime, str]] = []
    allowed_candidate_states = {"OPEN", "PENDING"}
    for offer in offers:
        state = str(offer.get("state", "")).strip().upper()
        if state not in allowed_candidate_states:
            continue
        if require_open_state and state != "OPEN":
            continue
        bech32 = str(offer.get("bech32", "")).strip()
        if not bech32.startswith("offer1"):
            continue
        offer_id = str(offer.get("offerId", "")).strip()
        markers = {f"bech32:{bech32}"}
        if offer_id:
            markers.add(f"id:{offer_id}")
        if markers.issubset(known_markers):
            continue
        created_at = parse_iso8601(str(offer.get("createdAt", "")).strip())
        if min_created_at is not None:
            if created_at is None or created_at < min_created_at:
                continue
        expires_at = parse_iso8601(str(offer.get("expiresAt", "")).strip())
        candidates.append(
            (
                created_at or dt.datetime.min.replace(tzinfo=dt.UTC),
                expires_at or dt.datetime.min.replace(tzinfo=dt.UTC),
                bech32,
            )
        )
    if not candidates:
        return ""
    candidates.sort(key=lambda row: (row[0], row[1]), reverse=bool(prefer_newest))
    return candidates[0][2]


def wallet_get_wallet_offers(
    wallet: CloudWalletAdapter,
    *,
    is_creator: bool | None,
    states: list[str] | None,
) -> dict[str, Any]:
    return wallet.get_wallet(is_creator=is_creator, states=states, first=100)


def is_transient_cloud_wallet_list_coins_error(error: str) -> bool:
    normalized = str(error).strip().lower()
    if not normalized:
        return False
    transient_markers = (
        "cloud_wallet_http_error:504",
        "cloud_wallet_http_error:503",
        "cloud_wallet_network_error",
        "http error 504",
        "http error 503",
        "gateway timeout",
        "service unavailable",
        "timed out",
        "timeout",
        "temporary failure",
        "connection reset",
        "connection refused",
        "remote end closed connection",
    )
    return any(marker in normalized for marker in transient_markers)


# Backward-compatible alias for legacy imports and test monkeypatch targets.
_is_transient_cloud_wallet_list_coins_error = is_transient_cloud_wallet_list_coins_error
