"""Cloud Wallet offer listing and cancel selection helpers."""

from __future__ import annotations

import urllib.parse
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter


def normalize_wallet_offer_row(row: dict[str, Any]) -> dict[str, str]:
    return {
        "wallet_offer_id": str(row.get("id", "")).strip(),
        "offer_id": str(row.get("offerId", "")).strip(),
        "state": str(row.get("state", "")).strip(),
        "expires_at": str(row.get("expiresAt", "")).strip(),
        "bech32": str(row.get("bech32", "")).strip(),
    }


def list_wallet_offers(*, wallet: CloudWalletAdapter) -> list[dict[str, str]]:
    wallet_payload = wallet.get_wallet()
    offers = wallet_payload.get("offers", [])
    rows: list[dict[str, str]] = []
    for row in offers if isinstance(offers, list) else []:
        if not isinstance(row, dict):
            continue
        normalized = normalize_wallet_offer_row(row)
        if normalized["offer_id"]:
            rows.append(normalized)
    return rows


def select_offers_for_cancel(
    *,
    wallet: CloudWalletAdapter,
    offer_ids: list[str],
    cancel_open: bool,
) -> list[dict[str, str]]:
    selected = list_wallet_offers(wallet=wallet)
    requested_ids = [str(value).strip() for value in offer_ids if str(value).strip()]
    if cancel_open:
        return [row for row in selected if str(row.get("state", "")).upper() == "OPEN"]
    if requested_ids:
        requested_set = set(requested_ids)
        return [row for row in selected if row["offer_id"] in requested_set]
    raise ValueError("provide at least one --offer-id or pass --cancel-open")


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
