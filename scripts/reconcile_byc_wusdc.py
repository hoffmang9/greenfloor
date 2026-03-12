#!/usr/bin/env python3
"""Reconcile BYC/wUSDC holdings against Cloud Wallet offers.

Usage:
  .venv/bin/python scripts/reconcile_byc_wusdc.py
  .venv/bin/python scripts/reconcile_byc_wusdc.py --json

This script is read-only. It fetches:
  - wallet asset totals (total/spendable/locked)
  - coin-level settled/pending/spendable sums for BYC and wUSDC.b
  - paginated creator offers, classified as buy/sell for BYC:wUSDC

The goal is to make inventory discrepancies easy to spot in one output.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from greenfloor.cli.manager import (  # noqa: E402
    _new_cloud_wallet_adapter,
    _require_cloud_wallet_config,
    _resolve_cloud_wallet_asset_id,
)
from greenfloor.config.io import load_program_config  # noqa: E402


def _graphql_with_retry(
    *, wallet: Any, query: str, variables: dict[str, Any], retries: int = 4
) -> dict[str, Any]:
    last_error: Exception | None = None
    for attempt in range(1, retries + 1):
        try:
            return wallet._graphql(query=query, variables=variables)
        except Exception as err:  # pragma: no cover - defensive around remote API instability
            last_error = err
            if attempt == retries:
                raise
            time.sleep(0.3 * attempt)
    if last_error:
        raise last_error
    return {}


def _wallet_asset_rows(*, wallet: Any) -> list[dict[str, Any]]:
    query = """
query walletAssetAmounts($walletId: ID!, $first: Int) {
  wallet(id: $walletId) {
    assets(first: $first) {
      edges {
        node {
          assetId
          totalAmount
          spendableAmount
          lockedAmount
        }
      }
    }
  }
}
"""
    payload = _graphql_with_retry(
        wallet=wallet,
        query=query,
        variables={"walletId": wallet.vault_id, "first": 100},
    )
    edges = ((payload.get("wallet") or {}).get("assets") or {}).get("edges") or []
    rows: list[dict[str, Any]] = []
    for edge in edges:
        node = edge.get("node") if isinstance(edge, dict) else None
        if not isinstance(node, dict):
            continue
        rows.append(
            {
                "asset_id": str(node.get("assetId", "")).strip(),
                "total": int(node.get("totalAmount", 0) or 0),
                "spendable": int(node.get("spendableAmount", 0) or 0),
                "locked": int(node.get("lockedAmount", 0) or 0),
            }
        )
    return rows


def _coins_summary(*, wallet: Any, asset_id: str) -> dict[str, int]:
    coins = wallet.list_coins(asset_id=asset_id, include_pending=True)
    settled = 0
    pending = 0
    spendable = 0
    total_items = 0
    for coin in coins:
        amount = int(coin.get("amount", 0))
        state = str(coin.get("state", "")).strip().upper()
        total_items += amount
        if state == "SETTLED":
            settled += amount
        if state == "PENDING":
            pending += amount
        if not bool(coin.get("isLocked", False)) and state in {"SETTLED", "CONFIRMED", "UNSPENT"}:
            spendable += amount
    return {
        "coin_count": len(coins),
        "items_total": total_items,
        "settled": settled,
        "pending": pending,
        "spendable_estimate": spendable,
    }


def _fetch_creator_offers(*, wallet: Any) -> list[dict[str, Any]]:
    query = """
query walletOffers($walletId: ID!, $first: Int, $after: String) {
  wallet(id: $walletId) {
    offers(first: $first, after: $after, isCreator: true) {
      pageInfo {
        hasNextPage
        endCursor
      }
      edges {
        node {
          id
          state
          createdAt
          assets(first: 10) {
            edges {
              node {
                amount
                type
                asset {
                  id
                }
              }
            }
          }
        }
      }
    }
  }
}
"""
    after: str | None = None
    offers: list[dict[str, Any]] = []
    for _ in range(64):
        payload = _graphql_with_retry(
            wallet=wallet,
            query=query,
            variables={"walletId": wallet.vault_id, "first": 100, "after": after},
        )
        offers_payload = (payload.get("wallet") or {}).get("offers") or {}
        edges = offers_payload.get("edges") or []
        for edge in edges:
            node = edge.get("node") if isinstance(edge, dict) else None
            if isinstance(node, dict):
                offers.append(node)
        page_info = offers_payload.get("pageInfo") or {}
        if not bool(page_info.get("hasNextPage", False)):
            break
        after_val = page_info.get("endCursor")
        if not isinstance(after_val, str) or not after_val:
            break
        after = after_val
    return offers


def _classify_offer(
    *,
    offer: dict[str, Any],
    byc_asset_id: str,
    quote_asset_id: str,
) -> tuple[str, int, int] | None:
    legs = []
    for edge in (offer.get("assets") or {}).get("edges") or []:
        node = edge.get("node") if isinstance(edge, dict) else None
        if not isinstance(node, dict):
            continue
        asset = node.get("asset") or {}
        if not isinstance(asset, dict):
            continue
        legs.append(
            (
                str(asset.get("id", "")).strip(),
                str(node.get("type", "")).strip().upper(),
                int(node.get("amount", 0) or 0),
            )
        )
    if not legs:
        return None
    legmap = {(asset_id, leg_type): amount for asset_id, leg_type, amount in legs}
    if (byc_asset_id, "OFFERED") in legmap and (quote_asset_id, "REQUESTED") in legmap:
        return ("sell", legmap[(byc_asset_id, "OFFERED")], legmap[(quote_asset_id, "REQUESTED")])
    if (quote_asset_id, "OFFERED") in legmap and (byc_asset_id, "REQUESTED") in legmap:
        return ("buy", legmap[(byc_asset_id, "REQUESTED")], legmap[(quote_asset_id, "OFFERED")])
    return None


def _to_units(value: int) -> str:
    return f"{value / 1000:.3f}"


def main() -> int:
    parser = argparse.ArgumentParser(description="Reconcile BYC/wUSDC balances and offer ledger.")
    parser.add_argument(
        "--program-config",
        default=str(Path("~/.greenfloor/config/program.yaml").expanduser()),
        help="Path to program.yaml",
    )
    parser.add_argument("--vault-id", default="", help="Optional Cloud Wallet vault override")
    parser.add_argument("--byc", default="BYC", help="BYC asset reference (default: BYC)")
    parser.add_argument(
        "--quote", default="wUSDC.b", help="Quote asset reference (default: wUSDC.b)"
    )
    parser.add_argument("--json", action="store_true", help="Output JSON report")
    args = parser.parse_args()

    program = load_program_config(Path(args.program_config))
    wallet = _new_cloud_wallet_adapter(program)
    if args.vault_id.strip() and args.vault_id.strip() != wallet.vault_id:
        cfg = _require_cloud_wallet_config(program)
        cfg = cfg.__class__(
            base_url=cfg.base_url,
            user_key_id=cfg.user_key_id,
            private_key_pem_path=cfg.private_key_pem_path,
            vault_id=args.vault_id.strip(),
            network=cfg.network,
            kms_key_id=cfg.kms_key_id,
            kms_region=cfg.kms_region,
            kms_public_key_hex=cfg.kms_public_key_hex,
        )
        from greenfloor.adapters.cloud_wallet import (
            CloudWalletAdapter,  # local import to avoid script startup side effects
        )

        wallet = CloudWalletAdapter(cfg)

    byc_asset_id = _resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=args.byc,
        symbol_hint=args.byc,
    )
    quote_asset_id = _resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=args.quote,
        symbol_hint=args.quote,
    )

    asset_rows = _wallet_asset_rows(wallet=wallet)
    asset_map = {row["asset_id"]: row for row in asset_rows}
    byc_asset = asset_map.get(byc_asset_id, {"total": 0, "spendable": 0, "locked": 0})
    quote_asset = asset_map.get(quote_asset_id, {"total": 0, "spendable": 0, "locked": 0})
    byc_coins = _coins_summary(wallet=wallet, asset_id=byc_asset_id)
    quote_coins = _coins_summary(wallet=wallet, asset_id=quote_asset_id)

    offers = _fetch_creator_offers(wallet=wallet)
    settled = {"buy_count": 0, "sell_count": 0, "byc_mojos": 0, "quote_mojos": 0}
    open_offers = {"buy_count": 0, "sell_count": 0, "byc_mojos": 0, "quote_mojos": 0}
    matched_offers = 0
    for offer in offers:
        classified = _classify_offer(
            offer=offer,
            byc_asset_id=byc_asset_id,
            quote_asset_id=quote_asset_id,
        )
        if classified is None:
            continue
        matched_offers += 1
        side, byc_mojos, quote_mojos = classified
        state = str(offer.get("state", "")).strip().upper()
        bucket = settled if state == "SETTLED" else open_offers if state == "OPEN" else None
        if bucket is None:
            continue
        bucket[f"{side}_count"] += 1
        bucket["byc_mojos"] += byc_mojos if side == "buy" else -byc_mojos
        bucket["quote_mojos"] += -quote_mojos if side == "buy" else quote_mojos

    ui_total = int(byc_asset["total"]) + int(quote_asset["total"])
    report = {
        "vault_id": wallet.vault_id,
        "resolved_assets": {"byc": byc_asset_id, "quote": quote_asset_id},
        "wallet_assets": {
            "byc": byc_asset,
            "quote": quote_asset,
        },
        "coin_state": {
            "byc": byc_coins,
            "quote": quote_coins,
        },
        "offers": {
            "fetched_creator_offers": len(offers),
            "matched_byc_quote_offers": matched_offers,
            "settled": settled,
            "open": open_offers,
        },
        "derived": {
            "ui_total_mojos": ui_total,
            "ui_total_units": _to_units(ui_total),
            "ui_plus_byc_pending_mojos": ui_total + int(byc_coins["pending"]),
            "ui_plus_byc_pending_units": _to_units(ui_total + int(byc_coins["pending"])),
            "settled_net_quote_mojos": int(settled["quote_mojos"]),
            "settled_net_quote_units": _to_units(int(settled["quote_mojos"])),
            "settled_net_byc_mojos": int(settled["byc_mojos"]),
            "settled_net_byc_units": _to_units(int(settled["byc_mojos"])),
        },
    }

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
        return 0

    print(f"vault_id: {report['vault_id']}")
    print(f"assets: BYC={byc_asset_id}  QUOTE={quote_asset_id}")
    print()
    print("wallet assets (mojos / units):")
    print(
        f"  BYC   total={byc_asset['total']} ({_to_units(int(byc_asset['total']))})"
        f" spendable={byc_asset['spendable']} ({_to_units(int(byc_asset['spendable']))})"
        f" locked={byc_asset['locked']} ({_to_units(int(byc_asset['locked']))})"
    )
    print(
        f"  QUOTE total={quote_asset['total']} ({_to_units(int(quote_asset['total']))})"
        f" spendable={quote_asset['spendable']} ({_to_units(int(quote_asset['spendable']))})"
        f" locked={quote_asset['locked']} ({_to_units(int(quote_asset['locked']))})"
    )
    print()
    print("coin state sums (from list_coins):")
    print(
        f"  BYC   settled={byc_coins['settled']} ({_to_units(byc_coins['settled'])})"
        f" pending={byc_coins['pending']} ({_to_units(byc_coins['pending'])})"
        f" item_sum={byc_coins['items_total']} ({_to_units(byc_coins['items_total'])})"
    )
    print(
        f"  QUOTE settled={quote_coins['settled']} ({_to_units(quote_coins['settled'])})"
        f" pending={quote_coins['pending']} ({_to_units(quote_coins['pending'])})"
        f" item_sum={quote_coins['items_total']} ({_to_units(quote_coins['items_total'])})"
    )
    print()
    print("offer ledger (creator offers, BYC<->QUOTE only):")
    print(
        f"  settled: buy_count={settled['buy_count']} sell_count={settled['sell_count']}"
        f" net_byc={settled['byc_mojos']} ({_to_units(settled['byc_mojos'])})"
        f" net_quote={settled['quote_mojos']} ({_to_units(settled['quote_mojos'])})"
    )
    print(
        f"  open:    buy_count={open_offers['buy_count']} sell_count={open_offers['sell_count']}"
        f" net_byc={open_offers['byc_mojos']} ({_to_units(open_offers['byc_mojos'])})"
        f" net_quote={open_offers['quote_mojos']} ({_to_units(open_offers['quote_mojos'])})"
    )
    print()
    print(
        f"derived: ui_total={ui_total} ({_to_units(ui_total)})"
        f"  ui_plus_byc_pending={ui_total + int(byc_coins['pending'])}"
        f" ({_to_units(ui_total + int(byc_coins['pending']))})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
