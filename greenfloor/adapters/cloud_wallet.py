from __future__ import annotations

import base64
import json
import random
import string
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass(frozen=True, slots=True)
class CloudWalletConfig:
    base_url: str
    user_key_id: str
    private_key_pem_path: str
    vault_id: str
    network: str


class CloudWalletAdapter:
    """Cloud Wallet GraphQL adapter authenticated via chia-user-key headers."""

    def __init__(self, config: CloudWalletConfig) -> None:
        self._base_url = config.base_url.rstrip("/")
        self._graphql_url = f"{self._base_url}/graphql"
        self._user_key_id = config.user_key_id
        self._private_key_pem = (
            Path(config.private_key_pem_path).expanduser().read_text(encoding="utf-8")
        )
        self._vault_id = config.vault_id
        self._network = config.network

    @property
    def vault_id(self) -> str:
        return self._vault_id

    @property
    def network(self) -> str:
        return self._network

    def list_coins(
        self,
        *,
        asset_id: str | None = None,
        include_pending: bool = True,
    ) -> list[dict[str, Any]]:
        query = """
query listCoins($walletId: ID!, $includePending: Boolean, $after: String, $assetId: ID) {
  coins(
    walletId: $walletId
    assetId: $assetId
    includePending: $includePending
    excludeAmounts: []
    excludeCoins: []
    sortKey: AMOUNT
    first: 100
    after: $after
  ) {
    pageInfo {
      hasNextPage
      endCursor
    }
    edges {
      cursor
      node {
        id
        name
        amount
        state
        puzzleHash
        parentCoinName
        asset {
          id
          type
        }
      }
    }
  }
}
"""
        after: str | None = None
        coins: list[dict[str, Any]] = []
        while True:
            variables: dict[str, Any] = {
                "walletId": self._vault_id,
                "assetId": asset_id,
                "includePending": bool(include_pending),
                "after": after,
            }
            payload = self._graphql(query=query, variables=variables)
            coins_payload = payload.get("coins") or {}
            edges = coins_payload.get("edges") or []
            for edge in edges:
                node = edge.get("node") if isinstance(edge, dict) else None
                if isinstance(node, dict):
                    coins.append(node)
            page_info = coins_payload.get("pageInfo") or {}
            if not bool(page_info.get("hasNextPage", False)):
                break
            after = page_info.get("endCursor")
            if not isinstance(after, str) or not after:
                break
        return coins

    def split_coins(
        self,
        *,
        coin_ids: list[str],
        amount_per_coin: int,
        number_of_coins: int,
        fee: int,
    ) -> dict[str, Any]:
        mutation = """
mutation splitCoins($walletId: ID!, $fee: BigInt!, $coinIds: [ID!]!, $amountPerCoin: BigInt!, $numberOfCoins: Int!) {
  splitCoins(
    input: {
      walletId: $walletId
      amountPerCoin: $amountPerCoin
      numberOfCoins: $numberOfCoins
      coinIds: $coinIds
      fee: $fee
    }
  ) {
    signatureRequest {
      id
      status
    }
  }
}
"""
        response = self._graphql(
            query=mutation,
            variables={
                "walletId": self._vault_id,
                "fee": int(fee),
                "coinIds": coin_ids,
                "amountPerCoin": int(amount_per_coin),
                "numberOfCoins": int(number_of_coins),
            },
        )
        split_payload = response.get("splitCoins") or {}
        signature_request = split_payload.get("signatureRequest") or {}
        return {
            "signature_request_id": str(signature_request.get("id", "")).strip(),
            "status": str(signature_request.get("status", "")).strip(),
        }

    def combine_coins(
        self,
        *,
        number_of_coins: int,
        fee: int,
        largest_first: bool = True,
        asset_id: str | None = None,
        input_coin_ids: list[str] | None = None,
        target_amount: int | None = None,
    ) -> dict[str, Any]:
        mutation = """
mutation combineCoins(
  $walletId: ID!
  $fee: BigInt!
  $numberOfCoins: Int!
  $largestFirst: Boolean
  $targetAmount: BigInt
  $inputCoinIds: [ID!]
  $assetId: ID
) {
  combineCoins(
    input: {
      walletId: $walletId
      numberOfCoins: $numberOfCoins
      fee: $fee
      largestFirst: $largestFirst
      targetAmount: $targetAmount
      inputCoinIds: $inputCoinIds
      assetId: $assetId
    }
  ) {
    signatureRequest {
      id
      status
    }
  }
}
"""
        response = self._graphql(
            query=mutation,
            variables={
                "walletId": self._vault_id,
                "fee": int(fee),
                "numberOfCoins": int(number_of_coins),
                "largestFirst": bool(largest_first),
                "targetAmount": int(target_amount) if target_amount is not None else None,
                "inputCoinIds": input_coin_ids,
                "assetId": asset_id,
            },
        )
        combine_payload = response.get("combineCoins") or {}
        signature_request = combine_payload.get("signatureRequest") or {}
        return {
            "signature_request_id": str(signature_request.get("id", "")).strip(),
            "status": str(signature_request.get("status", "")).strip(),
        }

    def create_offer(
        self,
        *,
        offered: list[dict[str, Any]],
        requested: list[dict[str, Any]],
        fee: int,
        expires_at_iso: str,
    ) -> dict[str, Any]:
        mutation = """
mutation createOffer($input: CreateOfferInput!) {
  createOffer(input: $input) {
    signatureRequest {
      id
      status
    }
  }
}
"""
        response = self._graphql(
            query=mutation,
            variables={
                "input": {
                    "walletId": self._vault_id,
                    "offered": offered,
                    "requested": requested,
                    "fee": int(fee),
                    "autoSubmit": True,
                    "expiresAt": expires_at_iso,
                }
            },
        )
        create_payload = response.get("createOffer") or {}
        signature_request = create_payload.get("signatureRequest") or {}
        return {
            "signature_request_id": str(signature_request.get("id", "")).strip(),
            "status": str(signature_request.get("status", "")).strip(),
        }

    def cancel_offer(self, *, offer_id: str) -> dict[str, Any]:
        clean_offer_id = str(offer_id).strip()
        if not clean_offer_id:
            raise ValueError("offer_id is required")
        mutation = """
mutation cancelOffer($input: CancelOfferInput!) {
  cancelOffer(input: $input) {
    signatureRequest {
      id
      status
    }
  }
}
"""
        response = self._graphql(
            query=mutation,
            variables={
                "input": {
                    "walletId": self._vault_id,
                    "offerId": clean_offer_id,
                }
            },
        )
        cancel_payload = response.get("cancelOffer") or {}
        signature_request = cancel_payload.get("signatureRequest") or {}
        return {
            "signature_request_id": str(signature_request.get("id", "")).strip(),
            "status": str(signature_request.get("status", "")).strip(),
        }

    def get_signature_request(self, *, signature_request_id: str) -> dict[str, Any]:
        query = """
query getSignatureRequest($id: ID!) {
  signatureRequest(id: $id) {
    id
    status
  }
}
"""
        payload = self._graphql(query=query, variables={"id": signature_request_id})
        signature_request = payload.get("signatureRequest") or {}
        if not isinstance(signature_request, dict):
            return {"id": signature_request_id, "status": "UNKNOWN"}
        return signature_request

    def get_wallet(self) -> dict[str, Any]:
        query = """
query getWallet($walletId: ID) {
  wallet(id: $walletId) {
    offers {
      edges {
        node {
              id
          offerId
          state
          settlementType
          expiresAt
          bech32
              createdAt
        }
      }
    }
  }
}
"""
        payload = self._graphql(query=query, variables={"walletId": self._vault_id})
        wallet = payload.get("wallet") or {}
        if not isinstance(wallet, dict):
            return {"offers": []}
        offers = (
            wallet.get("offers", {}).get("edges", [])
            if isinstance(wallet.get("offers"), dict)
            else []
        )
        normalized_offers: list[dict[str, Any]] = []
        for edge in offers:
            node = edge.get("node") if isinstance(edge, dict) else None
            if isinstance(node, dict):
                normalized_offers.append(node)
        return {"offers": normalized_offers}

    def _graphql(self, *, query: str, variables: dict[str, Any]) -> dict[str, Any]:
        body = json.dumps({"query": query, "variables": variables}, separators=(",", ":"))
        headers = self._build_auth_headers(body)
        req = urllib.request.Request(
            self._graphql_url,
            data=body.encode("utf-8"),
            method="POST",
            headers={
                "Content-Type": "application/json",
                "Accept": "application/json",
                "User-Agent": "greenfloor/0.1",
                **headers,
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                payload = json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            raw = exc.read().decode("utf-8", errors="replace").strip()
            snippet = raw[:200] if raw else ""
            message = f"cloud_wallet_http_error:{exc.code}"
            if snippet:
                message = f"{message}:{snippet}"
            raise RuntimeError(message) from exc
        except urllib.error.URLError as exc:
            raise RuntimeError(f"cloud_wallet_network_error:{exc.reason}") from exc
        if not isinstance(payload, dict):
            raise RuntimeError("cloud_wallet_invalid_response")
        errors = payload.get("errors")
        if isinstance(errors, list) and errors:
            first = errors[0]
            if isinstance(first, dict):
                raise RuntimeError(f"cloud_wallet_graphql_error:{first.get('message', 'unknown')}")
            raise RuntimeError(f"cloud_wallet_graphql_error:{first}")
        data = payload.get("data")
        if not isinstance(data, dict):
            raise RuntimeError("cloud_wallet_missing_data")
        return data

    def _build_auth_headers(self, raw_body: str) -> dict[str, str]:
        nonce = self._random_nonce(10)
        timestamp = str(int(time.time() * 1000))
        canonical = f"{raw_body}{nonce}{timestamp}"
        signature = self._sign_canonical(canonical)
        return {
            "chia-user-key-id": self._user_key_id,
            "chia-signature": signature,
            "chia-nonce": nonce,
            "chia-timestamp": timestamp,
        }

    def _sign_canonical(self, canonical: str) -> str:
        # Keep runtime dependencies minimal and deterministic for CI.
        return self._sign_canonical_with_openssl(canonical)

    def _sign_canonical_with_openssl(self, canonical: str) -> str:
        import subprocess
        import tempfile

        with tempfile.NamedTemporaryFile("w", delete=False, encoding="utf-8") as key_fp:
            key_fp.write(self._private_key_pem)
            key_path = key_fp.name
        try:
            completed = subprocess.run(
                [
                    "openssl",
                    "dgst",
                    "-sha256",
                    "-sign",
                    key_path,
                ],
                input=canonical.encode("utf-8"),
                capture_output=True,
                check=False,
            )
            if completed.returncode != 0:
                raise RuntimeError(
                    f"cloud_wallet_signature_failed:{completed.stderr.decode('utf-8', errors='replace').strip()}"
                )
            return base64.b64encode(completed.stdout).decode("ascii")
        finally:
            try:
                Path(key_path).unlink(missing_ok=True)
            except Exception:
                pass

    @staticmethod
    def _random_nonce(length: int) -> str:
        charset = string.ascii_letters + string.digits
        rng = random.SystemRandom()
        return "".join(rng.choice(charset) for _ in range(length))
