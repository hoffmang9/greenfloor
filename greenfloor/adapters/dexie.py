from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any


class DexieAdapter:
    def __init__(self, base_url: str) -> None:
        self.base_url = base_url.rstrip("/")

    def get_tokens(self) -> list[dict]:
        url = f"{self.base_url}/v1/swap/tokens"
        with urllib.request.urlopen(url, timeout=15) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
        tokens = payload.get("tokens", payload)
        if isinstance(tokens, list):
            return [t for t in tokens if isinstance(t, dict)]
        return []

    def get_offers(self, offered: str, requested: str) -> list[dict]:
        q = urllib.parse.urlencode({"offered": offered, "requested": requested})
        url = f"{self.base_url}/v1/offers?{q}"
        with urllib.request.urlopen(url, timeout=20) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
        offers = payload.get("offers", [])
        return [o for o in offers if isinstance(o, dict)]

    def get_offer(self, offer_id: str) -> dict[str, Any]:
        clean_offer_id = str(offer_id).strip()
        if not clean_offer_id:
            raise ValueError("offer_id is required")
        url = f"{self.base_url}/v1/offers/{urllib.parse.quote(clean_offer_id)}"
        with urllib.request.urlopen(url, timeout=20) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
        if isinstance(payload, dict):
            return payload
        return {"success": False, "error": "invalid_response_format"}

    def post_offer(
        self,
        offer: str,
        *,
        drop_only: bool = True,
        claim_rewards: bool | None = None,
    ) -> dict[str, Any]:
        payload: dict[str, Any] = {
            "offer": offer,
            "drop_only": bool(drop_only),
        }
        if claim_rewards is not None:
            payload["claim_rewards"] = bool(claim_rewards)
        url = f"{self.base_url}/v1/offers"
        body = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(
            url,
            data=body,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=20) as resp:
                result = json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            raw = exc.read().decode("utf-8", errors="replace").strip()
            snippet = raw[:500] if raw else ""
            error = f"dexie_http_error:{exc.code}"
            if snippet:
                error = f"{error}:{snippet}"
            return {"success": False, "error": error}
        except urllib.error.URLError as exc:
            return {"success": False, "error": f"dexie_network_error:{exc.reason}"}
        if isinstance(result, dict):
            return result
        return {"success": False, "error": "invalid_response_format"}

    def cancel_offer(self, offer_id: str) -> dict[str, Any]:
        clean_offer_id = offer_id.strip()
        url = f"{self.base_url}/v1/offers/{urllib.parse.quote(clean_offer_id)}/cancel"
        body = json.dumps({"id": clean_offer_id}).encode("utf-8")
        req = urllib.request.Request(
            url,
            data=body,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(req, timeout=20) as resp:
            result = json.loads(resp.read().decode("utf-8"))
        if isinstance(result, dict):
            return result
        return {"success": False, "error": "invalid_response_format"}

    def lookup_token_by_cat_id(self, cat_id_hex: str) -> dict | None:
        """Find a token by CAT asset ID across swap tokens and v3 tickers."""
        target = cat_id_hex.strip().lower()
        if not target:
            return None

        for row in self._fetch_token_rows():
            if _row_matches_cat_target(row, target):
                return row

        ticker_rows = self._fetch_ticker_rows()
        for row in ticker_rows:
            if _row_matches_cat_target(row, target, include_ticker_split=True):
                return row
        return None

    def lookup_token_by_symbol(
        self,
        symbol: str,
        *,
        label_matcher: Any | None = None,
    ) -> dict | None:
        """Find a token by symbol/name/code with optional fuzzy label matching."""
        target = symbol.strip()
        if not target:
            return None
        match_fn = label_matcher or _case_insensitive_match
        for row in self._fetch_token_rows():
            for key in ("code", "name", "id"):
                if match_fn(str(row.get(key, "")), target):
                    return row
        return None

    def _fetch_token_rows(self) -> list[dict]:
        try:
            return self.get_tokens()
        except Exception:
            return []

    def _fetch_ticker_rows(self) -> list[dict]:
        url = f"{self.base_url}/v3/prices/tickers"
        try:
            with urllib.request.urlopen(url, timeout=20) as resp:
                payload = json.loads(resp.read().decode("utf-8"))
        except Exception:
            return []
        if isinstance(payload, list):
            return [r for r in payload if isinstance(r, dict)]
        if isinstance(payload, dict):
            tickers = payload.get("tickers")
            if isinstance(tickers, list):
                return [r for r in tickers if isinstance(r, dict)]
        return []


def _row_matches_cat_target(row: dict, target: str, *, include_ticker_split: bool = False) -> bool:
    candidates = {
        str(row.get("assetId", "")).strip().lower(),
        str(row.get("asset_id", "")).strip().lower(),
        str(row.get("id", "")).strip().lower(),
        str(row.get("tokenId", "")).strip().lower(),
        str(row.get("token_id", "")).strip().lower(),
        str(row.get("base_currency", "")).strip().lower(),
        str(row.get("target_currency", "")).strip().lower(),
    }
    ticker_id = str(row.get("ticker_id", "")).strip().lower()
    if ticker_id:
        candidates.add(ticker_id)
        if include_ticker_split and "_" in ticker_id:
            base, quote = ticker_id.split("_", 1)
            candidates.add(base)
            candidates.add(quote)
    return target in candidates


def _case_insensitive_match(left: str, right: str) -> bool:
    a = left.strip().lower()
    b = right.strip().lower()
    return bool(a and b and a == b)
