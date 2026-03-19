#!/usr/bin/env python3
"""Trace hidden locked quote coins in a Cloud Wallet vault.

Usage:
  .venv/bin/python scripts/trace_locked_quote_coins.py
  .venv/bin/python scripts/trace_locked_quote_coins.py --json
  .venv/bin/python scripts/trace_locked_quote_coins.py --coin-id <coin-id>

This script is read-only. It is meant for cases where a wallet asset shows a
non-zero locked balance, but the normal coin list only exposes a smaller
spendable subset. The script:

  - fetches the quote asset totals from `wallet.assets`
  - fetches the raw quote coin set with `includeSpent=true`
  - isolates current unspent locked coins
  - walks each locked coin's local lineage
  - fetches nearby creator offers that offer the quote asset
  - inspects linked wallet transactions and reservation-split relations

The goal is to make stale reservation / create-offer locks visible in one run.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
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
    *,
    wallet: Any,
    query: str,
    variables: dict[str, Any],
    retries: int = 5,
) -> dict[str, Any]:
    last_error: Exception | None = None
    for attempt in range(1, retries + 1):
        try:
            return wallet._graphql(query=query, variables=variables)
        except Exception as err:  # pragma: no cover - defensive around remote API instability
            last_error = err
            if attempt == retries:
                raise
            time.sleep(0.5 * attempt)
    if last_error:
        raise last_error
    return {}


def _parse_dt(value: Any) -> datetime | None:
    text = str(value or "").strip()
    if not text:
        return None
    if text.endswith("Z"):
        text = f"{text[:-1]}+00:00"
    try:
        parsed = datetime.fromisoformat(text)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def _coin_hex(value: Any) -> str:
    text = str(value or "").strip()
    if text.startswith("CoinRecord_"):
        return text.removeprefix("CoinRecord_").lower()
    return text.lower()


def _wallet_asset_row(*, wallet: Any, asset_id: str) -> dict[str, int] | None:
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
    for edge in edges:
        node = edge.get("node") if isinstance(edge, dict) else None
        if not isinstance(node, dict):
            continue
        if str(node.get("assetId", "")).strip() != asset_id:
            continue
        return {
            "total": int(node.get("totalAmount", 0) or 0),
            "spendable": int(node.get("spendableAmount", 0) or 0),
            "locked": int(node.get("lockedAmount", 0) or 0),
        }
    return None


def _fetch_quote_coins(*, wallet: Any, asset_id: str) -> list[dict[str, Any]]:
    query = """
query quoteCoins(
  $walletId: ID!
  $assetId: ID
  $includePending: Boolean
  $includeSpent: Boolean
  $first: Int
) {
  coins(
    walletId: $walletId
    assetId: $assetId
    includePending: $includePending
    includeSpent: $includeSpent
    sortKey: CREATED_AT
    first: $first
  ) {
    edges {
      node {
        id
        name
        createdAt
        createdBlockHeight
        spentBlockHeight
        amount
        state
        isLocked
        isLinkedToOpenOffer
        puzzleHash
        parentCoinName
      }
    }
  }
}
"""
    payload = _graphql_with_retry(
        wallet=wallet,
        query=query,
        variables={
            "walletId": wallet.vault_id,
            "assetId": asset_id,
            "includePending": True,
            "includeSpent": True,
            "first": 100,
        },
    )
    edges = (payload.get("coins") or {}).get("edges") or []
    rows: list[dict[str, Any]] = []
    for edge in edges:
        node = edge.get("node") if isinstance(edge, dict) else None
        if not isinstance(node, dict):
            continue
        rows.append(
            {
                "id": str(node.get("id", "")).strip(),
                "coin_id": _coin_hex(node.get("id")),
                "name": str(node.get("name", "")).strip().lower(),
                "created_at": str(node.get("createdAt", "")).strip(),
                "created_dt": _parse_dt(node.get("createdAt")),
                "created_block_height": node.get("createdBlockHeight"),
                "spent_block_height": node.get("spentBlockHeight"),
                "amount": int(node.get("amount", 0) or 0),
                "state": str(node.get("state", "")).strip().upper(),
                "is_locked": bool(node.get("isLocked", False)),
                "is_linked_to_open_offer": bool(node.get("isLinkedToOpenOffer", False)),
                "puzzle_hash": str(node.get("puzzleHash", "")).strip(),
                "parent_coin_id": str(node.get("parentCoinName", "")).strip().lower(),
            }
        )
    return rows


def _current_locked_quote_coins(coins: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows = [
        coin
        for coin in coins
        if coin["state"] == "SETTLED"
        and coin["spent_block_height"] is None
        and coin["is_locked"]
        and not coin["is_linked_to_open_offer"]
    ]
    rows.sort(
        key=lambda coin: (coin["created_dt"] or datetime.min.replace(tzinfo=UTC), coin["coin_id"])
    )
    return rows


def _lineage_for_coin(
    *, coin: dict[str, Any], coins_by_id: dict[str, dict[str, Any]]
) -> list[dict[str, Any]]:
    lineage: list[dict[str, Any]] = []
    current = coin
    seen: set[str] = set()
    for _ in range(32):
        coin_id = str(current.get("coin_id", "")).strip().lower()
        if not coin_id or coin_id in seen:
            break
        seen.add(coin_id)
        lineage.append(current)
        parent_id = str(current.get("parent_coin_id", "")).strip().lower()
        if not parent_id:
            break
        parent = coins_by_id.get(parent_id)
        if parent is None:
            break
        current = parent
    return lineage


def _fetch_creator_quote_offers_basic(*, wallet: Any, quote_asset_id: str) -> list[dict[str, Any]]:
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
          offerId
          state
          settlementType
          createdAt
          expiresAt
          assets(first: 4) {
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
    for _ in range(128):
        payload = _graphql_with_retry(
            wallet=wallet,
            query=query,
            variables={"walletId": wallet.vault_id, "first": 100, "after": after},
        )
        offers_payload = (payload.get("wallet") or {}).get("offers") or {}
        edges = offers_payload.get("edges") or []
        for edge in edges:
            node = edge.get("node") if isinstance(edge, dict) else None
            if not isinstance(node, dict):
                continue
            legs = []
            for asset_edge in (node.get("assets") or {}).get("edges") or []:
                asset_node = asset_edge.get("node") if isinstance(asset_edge, dict) else None
                if not isinstance(asset_node, dict):
                    continue
                asset = asset_node.get("asset") or {}
                if not isinstance(asset, dict):
                    continue
                legs.append(
                    {
                        "asset_id": str(asset.get("id", "")).strip(),
                        "type": str(asset_node.get("type", "")).strip().upper(),
                        "amount": int(asset_node.get("amount", 0) or 0),
                    }
                )
            if not any(
                leg["asset_id"] == quote_asset_id and leg["type"] == "OFFERED" for leg in legs
            ):
                continue
            offers.append(
                {
                    "wallet_offer_id": str(node.get("id", "")).strip(),
                    "offer_id": str(node.get("offerId", "")).strip(),
                    "state": str(node.get("state", "")).strip(),
                    "settlement_type": node.get("settlementType"),
                    "created_at": str(node.get("createdAt", "")).strip(),
                    "created_dt": _parse_dt(node.get("createdAt")),
                    "expires_at": str(node.get("expiresAt", "")).strip(),
                    "legs": legs,
                }
            )
        page_info = offers_payload.get("pageInfo") or {}
        if not bool(page_info.get("hasNextPage", False)):
            break
        after_value = page_info.get("endCursor")
        if not isinstance(after_value, str) or not after_value:
            break
        after = after_value
    return offers


def _fetch_wallet_offer_detail(*, wallet: Any, wallet_offer_id: str) -> dict[str, Any] | None:
    query = """
query walletOfferDetail($id: ID!) {
  walletOffer(id: $id) {
    id
    offerId
    state
    settlementType
    createdAt
    expiresAt
    assets(first: 4) {
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
    transactions {
      id
      createdAt
      type
      fee
      state
      inputs {
        edges {
          node {
            id
            amount
            asset {
              id
            }
          }
        }
      }
      outputs {
        edges {
          node {
            id
            amount
            asset {
              id
            }
          }
        }
      }
    }
  }
}
"""
    payload = _graphql_with_retry(wallet=wallet, query=query, variables={"id": wallet_offer_id})
    node = payload.get("walletOffer")
    if not isinstance(node, dict):
        return None
    return node if isinstance(node, dict) else None


def _transaction_summary(node: dict[str, Any]) -> dict[str, Any]:
    def _legs(section: str) -> list[dict[str, Any]]:
        edges = (node.get(section) or {}).get("edges") or []
        rows: list[dict[str, Any]] = []
        for edge in edges:
            leg = edge.get("node") if isinstance(edge, dict) else None
            if not isinstance(leg, dict):
                continue
            asset = leg.get("asset") or {}
            rows.append(
                {
                    "coin_id": _coin_hex(leg.get("id")),
                    "amount": int(leg.get("amount", 0) or 0),
                    "asset_id": str(asset.get("id", "")).strip() if isinstance(asset, dict) else "",
                }
            )
        return rows

    return {
        "id": str(node.get("id", "")).strip(),
        "created_at": str(node.get("createdAt", "")).strip(),
        "amount": int(node.get("amount", 0) or 0),
        "type": str(node.get("type", "")).strip(),
        "state": str(node.get("state", "")).strip(),
        "inputs": _legs("inputs"),
        "outputs": _legs("outputs"),
    }


def _coin_match_summary(*, coin_id: str, tx: dict[str, Any]) -> dict[str, bool]:
    inputs = {row["coin_id"] for row in tx.get("inputs", [])}
    outputs = {row["coin_id"] for row in tx.get("outputs", [])}
    return {
        "tx_input_match": coin_id in inputs,
        "tx_output_match": coin_id in outputs,
    }


@dataclass
class CoinTrace:
    coin: dict[str, Any]
    lineage: list[dict[str, Any]]
    nearby_offers: list[dict[str, Any]]


def _trace_coin(
    *,
    wallet: Any,
    coin: dict[str, Any],
    coins_by_id: dict[str, dict[str, Any]],
    creator_quote_offers: list[dict[str, Any]],
    window_hours: int,
) -> CoinTrace:
    lineage = _lineage_for_coin(coin=coin, coins_by_id=coins_by_id)
    coin_created = coin.get("created_dt")
    nearby_offers: list[dict[str, Any]] = []
    if coin_created is None:
        return CoinTrace(coin=coin, lineage=lineage, nearby_offers=nearby_offers)

    window = timedelta(hours=max(1, int(window_hours)))
    candidates = [
        offer
        for offer in creator_quote_offers
        if offer["created_dt"] is not None and abs(offer["created_dt"] - coin_created) <= window
    ]
    candidates.sort(key=lambda offer: abs((offer["created_dt"] - coin_created).total_seconds()))

    for offer in candidates[:8]:
        detail = _fetch_wallet_offer_detail(wallet=wallet, wallet_offer_id=offer["wallet_offer_id"])
        if not isinstance(detail, dict):
            continue
        summarized_transactions: list[dict[str, Any]] = []
        for raw_tx in detail.get("transactions") or []:
            if not isinstance(raw_tx, dict):
                continue
            tx_detail = _transaction_summary(raw_tx)
            match_summary = _coin_match_summary(coin_id=coin["coin_id"], tx=tx_detail)
            summarized_transactions.append({**tx_detail, "match": match_summary})
        nearby_offers.append(
            {
                "wallet_offer_id": str(detail.get("id", "")).strip(),
                "offer_id": str(detail.get("offerId", "")).strip(),
                "state": str(detail.get("state", "")).strip(),
                "settlement_type": detail.get("settlementType"),
                "created_at": str(detail.get("createdAt", "")).strip(),
                "expires_at": str(detail.get("expiresAt", "")).strip(),
                "transactions": summarized_transactions,
            }
        )
    return CoinTrace(coin=coin, lineage=lineage, nearby_offers=nearby_offers)


def _render_text_report(
    *,
    vault_id: str,
    quote_asset_id: str,
    wallet_asset: dict[str, int] | None,
    current_locked: list[dict[str, Any]],
    traces: list[CoinTrace],
) -> str:
    lines: list[str] = []
    lines.append(f"vault_id: {vault_id}")
    lines.append(f"quote_asset_id: {quote_asset_id}")
    if wallet_asset is not None:
        lines.append(
            "wallet_asset: "
            f"total={wallet_asset['total']} "
            f"spendable={wallet_asset['spendable']} "
            f"locked={wallet_asset['locked']}"
        )
    lines.append(f"current_locked_coin_count: {len(current_locked)}")
    lines.append(f"current_locked_total: {sum(int(row['amount']) for row in current_locked)}")
    lines.append("")
    for trace in traces:
        coin = trace.coin
        lines.append(
            "locked_coin: "
            f"{coin['coin_id']} "
            f"amount={coin['amount']} "
            f"created_at={coin['created_at']} "
            f"created_height={coin['created_block_height']}"
        )
        lines.append("lineage:")
        for row in trace.lineage:
            lines.append(
                "  "
                f"{row['coin_id']} "
                f"amount={row['amount']} "
                f"created_at={row['created_at']} "
                f"spent_height={row['spent_block_height']}"
            )
        if not trace.nearby_offers:
            lines.append("nearby_offers: none")
            lines.append("")
            continue
        lines.append("nearby_offers:")
        for offer in trace.nearby_offers:
            lines.append(
                "  "
                f"{offer['wallet_offer_id']} "
                f"state={offer['state']} "
                f"settlement={offer['settlement_type']} "
                f"created_at={offer['created_at']}"
            )
            if not offer["transactions"]:
                lines.append("    transactions: none")
                continue
            for tx in offer["transactions"]:
                match = tx["match"]
                lines.append(
                    "    "
                    f"{tx['id']} "
                    f"type={tx['type']} "
                    f"state={tx['state']} "
                    f"match={json.dumps(match, sort_keys=True)}"
                )
                reservation = tx.get("reservation")
                if reservation is not None:
                    lines.append(
                        "      "
                        f"reservation={reservation['id']} "
                        f"type={reservation['type']} "
                        f"state={reservation['state']}"
                    )
        lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Trace hidden locked quote coins in a Cloud Wallet vault."
    )
    parser.add_argument(
        "--program-config",
        default=str(Path("~/.greenfloor/config/program.yaml").expanduser()),
        help="Path to program.yaml",
    )
    parser.add_argument("--vault-id", default="", help="Optional Cloud Wallet vault override")
    parser.add_argument(
        "--quote", default="wUSDC.b", help="Quote asset reference (default: wUSDC.b)"
    )
    parser.add_argument(
        "--coin-id",
        action="append",
        default=[],
        help="Restrict tracing to one or more coin ids (hex or CoinRecord_...)",
    )
    parser.add_argument(
        "--window-hours",
        type=int,
        default=24,
        help="Creator-offer correlation window around locked coin creation time",
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
        from greenfloor.adapters.cloud_wallet import CloudWalletAdapter  # noqa: PLC0415

        wallet = CloudWalletAdapter(cfg)

    quote_asset_id = _resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=args.quote,
        symbol_hint=args.quote,
        program_home_dir=str(program.home_dir),
    )
    wallet_asset = _wallet_asset_row(wallet=wallet, asset_id=quote_asset_id)
    quote_coins = _fetch_quote_coins(wallet=wallet, asset_id=quote_asset_id)
    coins_by_id = {row["coin_id"]: row for row in quote_coins if row["coin_id"]}
    current_locked = _current_locked_quote_coins(quote_coins)

    requested_coin_ids = {_coin_hex(value) for value in args.coin_id}
    if requested_coin_ids:
        current_locked = [row for row in current_locked if row["coin_id"] in requested_coin_ids]

    creator_quote_offers = _fetch_creator_quote_offers_basic(
        wallet=wallet, quote_asset_id=quote_asset_id
    )
    traces = [
        _trace_coin(
            wallet=wallet,
            coin=coin,
            coins_by_id=coins_by_id,
            creator_quote_offers=creator_quote_offers,
            window_hours=args.window_hours,
        )
        for coin in current_locked
    ]

    payload = {
        "vault_id": wallet.vault_id,
        "quote_asset_id": quote_asset_id,
        "wallet_asset": wallet_asset,
        "current_locked_coins": current_locked,
        "traces": [
            {
                "coin": trace.coin,
                "lineage": trace.lineage,
                "nearby_offers": trace.nearby_offers,
            }
            for trace in traces
        ],
    }
    if args.json:
        print(json.dumps(payload, indent=2, sort_keys=True))
        return 0

    print(
        _render_text_report(
            vault_id=wallet.vault_id,
            quote_asset_id=quote_asset_id,
            wallet_asset=wallet_asset,
            current_locked=current_locked,
            traces=traces,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
