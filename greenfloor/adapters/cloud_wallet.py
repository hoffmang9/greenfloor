from __future__ import annotations

import base64
import json
import logging
import os
import random
import re
import socket
import string
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

# GraphQL document shape: `query Name(` / `mutation Name(` / `query {` (anonymous).
_GRAPHQL_NAMED_OP_RE = re.compile(
    r"\b(query|mutation|subscription)\s+(\w+)\s*[\(\{]",
    re.IGNORECASE | re.DOTALL,
)
_GRAPHQL_ANON_OP_RE = re.compile(
    r"\b(query|mutation|subscription)\s*\{",
    re.IGNORECASE | re.DOTALL,
)


@dataclass(frozen=True, slots=True)
class CloudWalletConfig:
    base_url: str
    user_key_id: str
    private_key_pem_path: str
    vault_id: str
    network: str
    kms_key_id: str | None = None
    kms_region: str | None = None
    kms_public_key_hex: str | None = None


class CloudWalletAdapter:
    """Cloud Wallet GraphQL adapter authenticated via chia-user-key headers."""

    def __init__(self, config: CloudWalletConfig) -> None:
        self._base_url = config.base_url.rstrip("/")
        self._graphql_url = f"{self._base_url}/graphql"
        self._user_key_id = config.user_key_id
        pem_path = Path(config.private_key_pem_path).expanduser().resolve()
        if ".greenfloor" not in pem_path.parts:
            raise ValueError("cloud_wallet_private_key_pem_path_must_be_under_dot_greenfloor")
        if not pem_path.is_file():
            raise FileNotFoundError(
                f"cloud_wallet_private_key_pem_path_not_found:{os.fspath(pem_path)}"
            )
        self._private_key_pem_path = os.fspath(pem_path)
        self._vault_id = config.vault_id
        self._network = config.network
        self._kms_key_id = (config.kms_key_id or "").strip() or None
        self._kms_region = (config.kms_region or "").strip() or "us-west-2"
        self._kms_public_key_hex = (config.kms_public_key_hex or "").strip() or None

    @property
    def vault_id(self) -> str:
        return self._vault_id

    @property
    def network(self) -> str:
        return self._network

    @property
    def kms_configured(self) -> bool:
        return self._kms_key_id is not None

    def _resolve_kms_public_key(self) -> str:
        """Return the compressed-hex KMS public key, fetching from AWS if not cached."""
        if self._kms_public_key_hex:
            return self._kms_public_key_hex
        if not self._kms_key_id:
            raise RuntimeError("kms_key_id is not configured")
        from greenfloor.adapters.kms_signer import get_public_key_compressed_hex

        self._kms_public_key_hex = get_public_key_compressed_hex(self._kms_key_id, self._kms_region)
        return self._kms_public_key_hex

    def list_coins(
        self,
        *,
        asset_id: str | None = None,
        include_pending: bool = False,
        min_amount_mojos: int | None = 1000,
    ) -> list[dict[str, Any]]:
        """List wallet coins via Cloud Wallet GraphQL.

        Defaults match ent-wallet hotwallet-style filtering for faster queries:
        ``includePending=false`` (SETTLED-only) and ``minAmount=1000`` when set.
        One CAT unit is exactly 1000 mojos (``AGENTS.md`` CAT discipline); for
        XCH, 1000 mojos only drops sub-dust outputs. Pass
        ``include_pending=True`` and/or ``min_amount_mojos=None`` where pending
        or sub-unit coins must be visible (coin-ops, pre-offer balance checks).

        Upstream Cloud Wallet can mis-resolve ``node.asset`` on asset-scoped coin
        queries, including falling back to XCH for rows that were already
        selected by the requested CAT scope. Match the first-party UI here:
        when ``asset_id`` is provided, trust the query scope and omit row
        asset metadata instead of importing misleading fallback values.
        """
        asset_fields = ""
        if not asset_id:
            asset_fields = """
        asset {
          id
          type
        }"""
        query = f"""
query listCoins($walletId: ID!, $includePending: Boolean, $after: String, $assetId: ID, $minAmount: BigInt) {{
  coins(
    walletId: $walletId
    assetId: $assetId
    includePending: $includePending
    minAmount: $minAmount
    excludeAmounts: []
    excludeCoins: []
    sortKey: AMOUNT
    first: 100
    after: $after
  ) {{
    pageInfo {{
      hasNextPage
      endCursor
    }}
    edges {{
      cursor
      node {{
        id
        name
        amount
        state
        isLocked
        puzzleHash
        parentCoinName{asset_fields}
      }}
    }}
  }}
}}
"""
        after: str | None = None
        coins: list[dict[str, Any]] = []
        while True:
            variables: dict[str, Any] = {
                "walletId": self._vault_id,
                "assetId": asset_id,
                "includePending": bool(include_pending),
                "after": after,
                "minAmount": (str(int(min_amount_mojos)) if min_amount_mojos is not None else None),
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

    def get_chia_usd_quote(self) -> float:
        query = """
query quote($asset: String!) {
  quote(asset: $asset) {
    price
    baseAsset
    currency
    source
    createdAt
  }
}
"""
        payload = self._graphql(query=query, variables={"asset": "chia"})
        quote_payload = payload.get("quote")
        if not isinstance(quote_payload, dict):
            raise RuntimeError("cloud_wallet_missing_quote")
        raw_price = quote_payload.get("price")
        if not isinstance(raw_price, str | int | float):
            raise RuntimeError("cloud_wallet_invalid_quote_price")
        try:
            price = float(raw_price)
        except (TypeError, ValueError) as exc:
            raise RuntimeError("cloud_wallet_invalid_quote_price") from exc
        if price <= 0:
            raise RuntimeError("cloud_wallet_invalid_quote_price")
        return price

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
        result = {
            "signature_request_id": str(signature_request.get("id", "")).strip(),
            "status": str(signature_request.get("status", "")).strip(),
        }
        return self._auto_sign_if_kms(result)

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
        result = {
            "signature_request_id": str(signature_request.get("id", "")).strip(),
            "status": str(signature_request.get("status", "")).strip(),
        }
        return self._auto_sign_if_kms(result)

    def create_offer(
        self,
        *,
        offered: list[dict[str, Any]],
        requested: list[dict[str, Any]],
        fee: int,
        expires_at_iso: str,
        split_input_coins: bool = True,
        split_input_coins_fee: int = 0,
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
                    "splitInputCoins": bool(split_input_coins),
                    "splitInputCoinsFee": int(split_input_coins_fee),
                }
            },
        )
        create_payload = response.get("createOffer") or {}
        signature_request = create_payload.get("signatureRequest") or {}
        result = {
            "signature_request_id": str(signature_request.get("id", "")).strip(),
            "status": str(signature_request.get("status", "")).strip(),
        }
        return self._auto_sign_if_kms(result)

    def cancel_offer(self, *, offer_id: str, cancel_off_chain: bool = False) -> dict[str, Any]:
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
                    "cancelOffChain": bool(cancel_off_chain),
                }
            },
        )
        cancel_payload = response.get("cancelOffer") or {}
        signature_request = cancel_payload.get("signatureRequest") or {}
        result = {
            "signature_request_id": str(signature_request.get("id", "")).strip(),
            "status": str(signature_request.get("status", "")).strip(),
        }
        return self._auto_sign_if_kms(result)

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

    def get_coin_record(self, *, coin_id: str) -> dict[str, Any]:
        clean_coin_id = str(coin_id).strip()
        if not clean_coin_id:
            raise ValueError("coin_id is required")
        query = """
query getCoinRecord($id: ID!) {
  node(id: $id) {
    __typename
    ... on CoinRecord {
      id
      name
      amount
      state
      isLocked
      isLinkedToOpenOffer
      puzzleHash
      parentCoinName
      createdBlockHeight
      spentBlockHeight
      asset {
        id
        type
      }
    }
  }
}
"""
        payload = self._graphql(query=query, variables={"id": clean_coin_id})
        coin_record = payload.get("node")
        if not isinstance(coin_record, dict):
            return {"id": clean_coin_id, "state": "UNKNOWN"}
        return coin_record

    def get_signature_request_offer(self, *, signature_request_id: str) -> dict[str, Any]:
        query = """
query getSignatureRequestOffer($id: ID!) {
  signatureRequest(id: $id) {
    id
    status
    transaction {
      offer {
        id
        offerId
        bech32
        state
        createdAt
      }
    }
  }
}
"""
        payload = self._graphql(query=query, variables={"id": signature_request_id})
        signature_request = payload.get("signatureRequest") or {}
        if not isinstance(signature_request, dict):
            return {"id": signature_request_id, "status": "UNKNOWN"}
        transaction = signature_request.get("transaction") or {}
        offer = transaction.get("offer") if isinstance(transaction, dict) else None
        if not isinstance(offer, dict):
            return {
                "id": str(signature_request.get("id", signature_request_id)).strip(),
                "status": str(signature_request.get("status", "UNKNOWN")).strip(),
                "offer_id": "",
                "bech32": "",
                "state": "",
                "created_at": "",
            }
        return {
            "id": str(signature_request.get("id", signature_request_id)).strip(),
            "status": str(signature_request.get("status", "UNKNOWN")).strip(),
            "offer_id": str(offer.get("id", "")).strip() or str(offer.get("offerId", "")).strip(),
            "bech32": str(offer.get("bech32", "")).strip(),
            "state": str(offer.get("state", "")).strip(),
            "created_at": str(offer.get("createdAt", "")).strip(),
        }

    def get_wallet(
        self,
        *,
        is_creator: bool | None = None,
        states: list[str] | None = None,
        first: int = 100,
    ) -> dict[str, Any]:
        # Cloud Wallet currently rejects wallet.offers limits above 100.
        # Revisit this guard if Cloud Wallet pagination defaults/maxima change.
        first_limit = min(100, max(0, int(first)))
        query = """
query getWallet($walletId: ID, $isCreator: Boolean, $states: [OfferState!], $first: Int) {
  wallet(id: $walletId) {
    offers(isCreator: $isCreator, states: $states, first: $first) {
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
        payload = self._graphql(
            query=query,
            variables={
                "walletId": self._vault_id,
                "isCreator": is_creator,
                "states": states,
                "first": first_limit,
            },
        )
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

    def get_vault_custody_snapshot(self) -> dict[str, Any]:
        """Return vault custody config and signer key material for local vault spend assembly."""
        query = """
query getVaultCustodySnapshot($walletId: ID!, $first: Int!) {
  wallet(id: $walletId) {
    custodyConfig {
      vaultCustodyConfig {
        vaultLauncherId
        custodyThreshold
        recoveryThreshold
        recoveryClawbackTimelock
        custodyKeys(first: $first) {
          edges {
            node {
              publicKey
              curve
            }
          }
        }
        recoveryKeys(first: $first) {
          edges {
            node {
              publicKey
              curve
            }
          }
        }
      }
    }
  }
}
"""
        payload = self._graphql(
            query=query,
            variables={
                "walletId": self._vault_id,
                "first": 50,
            },
        )
        wallet = payload.get("wallet") if isinstance(payload, dict) else None
        if not isinstance(wallet, dict):
            return {}
        custody_config = wallet.get("custodyConfig")
        if not isinstance(custody_config, dict):
            return {}
        vault_cfg = custody_config.get("vaultCustodyConfig")
        if not isinstance(vault_cfg, dict):
            return {}
        return vault_cfg

    # ------------------------------------------------------------------
    # KMS vault signing
    # ------------------------------------------------------------------

    _SIGN_SIGNATURE_REQUEST_MUTATION = """
mutation SignSignatureRequest($input: SignSignatureRequestInput!) {
  signSignatureRequest(input: $input) {
    signatureRequest {
      id
      status
    }
  }
}
"""

    _GET_SIGNATURE_REQUEST_WITH_MESSAGES_QUERY = """
query getSignatureRequest($id: ID!) {
  signatureRequest(id: $id) {
    id
    status
    messages {
      publicKey
      message
    }
  }
}
"""

    def get_signature_request_with_messages(self, *, signature_request_id: str) -> dict[str, Any]:
        """Fetch a signature request including its signable messages."""
        payload = self._graphql(
            query=self._GET_SIGNATURE_REQUEST_WITH_MESSAGES_QUERY,
            variables={"id": signature_request_id},
        )
        sr = payload.get("signatureRequest") or {}
        if not isinstance(sr, dict):
            return {"id": signature_request_id, "status": "UNKNOWN", "messages": []}
        return sr

    def _sign_signature_request(
        self,
        *,
        signature_request_id: str,
        public_key_hex: str,
        message_hex: str,
        signature_hex: str,
    ) -> dict[str, Any]:
        """Submit a single signature to the ent-wallet API."""
        resp = self._graphql(
            query=self._SIGN_SIGNATURE_REQUEST_MUTATION,
            variables={
                "input": {
                    "signatureRequestId": signature_request_id,
                    "publicKey": public_key_hex,
                    "message": message_hex,
                    "signature": signature_hex,
                }
            },
        )
        return (resp.get("signSignatureRequest") or {}).get("signatureRequest") or {}

    def sign_with_kms(self, *, signature_request_id: str) -> dict[str, Any]:
        """Sign all messages matching our KMS key on a signature request.

        Returns the final signature request state after signing.
        """
        if not self._kms_key_id:
            raise RuntimeError("kms_key_id is not configured; cannot sign with KMS")
        from greenfloor.adapters.kms_signer import sign_digest

        pubkey_hex = self._resolve_kms_public_key()
        sr = self.get_signature_request_with_messages(signature_request_id=signature_request_id)
        messages = sr.get("messages") or []
        signed_count = 0
        last_result: dict[str, Any] = sr

        for msg in messages:
            msg_pubkey = str(msg.get("publicKey", "")).lower().replace("0x", "")
            if msg_pubkey != pubkey_hex.lower().replace("0x", ""):
                continue
            message_hex = str(msg.get("message", "")).replace("0x", "")
            logger.info("kms_signing message for sig_request=%s", signature_request_id)
            compact_sig_hex = sign_digest(self._kms_key_id, self._kms_region, message_hex)
            last_result = self._sign_signature_request(
                signature_request_id=signature_request_id,
                public_key_hex=pubkey_hex,
                message_hex=message_hex,
                signature_hex=compact_sig_hex,
            )
            signed_count += 1

        logger.info(
            "kms_sign_complete sig_request=%s signed=%d", signature_request_id, signed_count
        )
        return last_result

    def _auto_sign_if_kms(self, result: dict[str, Any]) -> dict[str, Any]:
        """If KMS is configured and the operation returned a signature request, sign it.

        Mutates ``result`` in place with the updated status and returns it.
        """
        if not self.kms_configured:
            return result
        sig_id = str(result.get("signature_request_id", "")).strip()
        if not sig_id:
            return result
        status = str(result.get("status", "")).upper()
        if status not in {"UNSIGNED", "PARTIALLY_SIGNED", "AWAITING_REVIEW", ""}:
            return result
        sr = self.sign_with_kms(signature_request_id=sig_id)
        result["status"] = sr.get("status", result.get("status"))
        return result

    @staticmethod
    def _parse_retry_after_seconds(value: str) -> int | None:
        text = str(value or "").strip()
        if not text:
            return None
        if text.isdigit():
            seconds = int(text)
            return seconds if seconds > 0 else None
        match = re.search(r"try again in\s+(\d+)\s+seconds?", text, flags=re.IGNORECASE)
        if match is None:
            return None
        seconds = int(match.group(1))
        return seconds if seconds > 0 else None

    @staticmethod
    def _is_rate_limit_error_message(message: str) -> bool:
        normalized = str(message or "").strip().lower()
        return "rate limit" in normalized or "too many requests" in normalized

    @staticmethod
    def _is_transient_http_status(code: int) -> bool:
        return int(code) in {502, 503, 504}

    @staticmethod
    def _is_transient_error_message(message: str) -> bool:
        normalized = str(message or "").strip().lower()
        transient_markers = (
            "timed out",
            "timeout",
            "temporary unavailable",
            "temporarily unavailable",
            "bad gateway",
            "gateway timeout",
            "service unavailable",
            "connection reset",
            "connection refused",
        )
        return any(marker in normalized for marker in transient_markers)

    @staticmethod
    def _is_transient_url_error(reason: object) -> bool:
        if isinstance(reason, TimeoutError | socket.timeout):
            return True
        return CloudWalletAdapter._is_transient_error_message(str(reason or ""))

    @staticmethod
    def _backoff_seconds_for_attempt(
        *, attempt_index: int, retry_after_seconds: int | None
    ) -> float:
        # attempt_index is zero-based.
        exponential = min(16.0, float(2**attempt_index))
        if retry_after_seconds is None:
            return exponential
        return float(max(exponential, int(retry_after_seconds)))

    @staticmethod
    def _graphql_operation_label(query: str) -> str:
        text = str(query or "")
        match = _GRAPHQL_NAMED_OP_RE.search(text)
        if match:
            return f"{match.group(1).lower()}_{match.group(2)}"
        match = _GRAPHQL_ANON_OP_RE.search(text)
        if match:
            return f"{match.group(1).lower()}_anonymous"
        return "unknown"

    def _graphql(self, *, query: str, variables: dict[str, Any]) -> dict[str, Any]:
        body = json.dumps({"query": query, "variables": variables}, separators=(",", ":"))
        max_attempts = max(
            1, int(os.getenv("GREENFLOOR_CLOUD_WALLET_MAX_ATTEMPTS", "3").strip() or "3")
        )
        request_timeout_seconds = max(
            5,
            int(os.getenv("GREENFLOOR_CLOUD_WALLET_HTTP_TIMEOUT_SECONDS", "10").strip() or "10"),
        )
        operation = self._graphql_operation_label(query)
        slow_ms_env = os.getenv("GREENFLOOR_CLOUD_WALLET_SLOW_LOG_MS", "").strip()
        if slow_ms_env:
            slow_threshold_ms = max(0, int(slow_ms_env or "0"))
        else:
            slow_threshold_ms = max(1, int(request_timeout_seconds * 1000 * 0.8))
        for attempt in range(max_attempts):
            attempt_started = time.monotonic()

            def elapsed_ms(_t0: float = attempt_started) -> int:
                return int((time.monotonic() - _t0) * 1000)

            # Build fresh auth headers per attempt. Cloud Wallet rejects replayed
            # nonces, so retries must not reuse the same signed request headers.
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
                with urllib.request.urlopen(req, timeout=request_timeout_seconds) as resp:
                    payload = json.loads(resp.read().decode("utf-8"))
            except urllib.error.HTTPError as exc:
                duration_ms = elapsed_ms()
                raw = exc.read().decode("utf-8", errors="replace").strip()
                snippet = raw[:200] if raw else ""
                retry_after_header = exc.headers.get("Retry-After") if exc.headers else None
                retry_after_seconds = self._parse_retry_after_seconds(str(retry_after_header or ""))
                if retry_after_seconds is None:
                    retry_after_seconds = self._parse_retry_after_seconds(raw)
                if int(exc.code) == 429 and attempt < (max_attempts - 1):
                    sleep_seconds = self._backoff_seconds_for_attempt(
                        attempt_index=attempt,
                        retry_after_seconds=retry_after_seconds,
                    )
                    logger.warning(
                        "cloud_wallet_rate_limited http_status=429 operation=%s duration_ms=%s "
                        "attempt=%s/%s sleep_seconds=%.1f retry_after_seconds=%s",
                        operation,
                        duration_ms,
                        attempt + 1,
                        max_attempts,
                        sleep_seconds,
                        retry_after_seconds,
                    )
                    time.sleep(sleep_seconds)
                    continue
                if self._is_transient_http_status(int(exc.code)) and attempt < (max_attempts - 1):
                    sleep_seconds = self._backoff_seconds_for_attempt(
                        attempt_index=attempt,
                        retry_after_seconds=retry_after_seconds,
                    )
                    logger.warning(
                        "cloud_wallet_transient_http_error http_status=%s operation=%s duration_ms=%s "
                        "attempt=%s/%s sleep_seconds=%.1f",
                        exc.code,
                        operation,
                        duration_ms,
                        attempt + 1,
                        max_attempts,
                        sleep_seconds,
                    )
                    time.sleep(sleep_seconds)
                    continue
                message = f"cloud_wallet_http_error:{exc.code}"
                if snippet:
                    message = f"{message}:{snippet}"
                logger.warning(
                    "cloud_wallet_http_error_final operation=%s duration_ms=%s http_status=%s",
                    operation,
                    duration_ms,
                    exc.code,
                )
                raise RuntimeError(message) from exc
            except (urllib.error.URLError, TimeoutError) as exc:
                duration_ms = elapsed_ms()
                reason = exc.reason if isinstance(exc, urllib.error.URLError) else exc
                is_transient = (
                    self._is_transient_url_error(exc.reason)
                    if isinstance(exc, urllib.error.URLError)
                    else True
                )
                if is_transient and attempt < (max_attempts - 1):
                    sleep_seconds = self._backoff_seconds_for_attempt(
                        attempt_index=attempt,
                        retry_after_seconds=None,
                    )
                    logger.warning(
                        "cloud_wallet_transient_network_error operation=%s duration_ms=%s "
                        "attempt=%s/%s sleep_seconds=%.1f reason=%s",
                        operation,
                        duration_ms,
                        attempt + 1,
                        max_attempts,
                        sleep_seconds,
                        reason,
                    )
                    time.sleep(sleep_seconds)
                    continue
                logger.warning(
                    "cloud_wallet_network_error_final operation=%s duration_ms=%s reason=%s",
                    operation,
                    duration_ms,
                    reason,
                )
                raise RuntimeError(f"cloud_wallet_network_error:{reason}") from exc
            if not isinstance(payload, dict):
                raise RuntimeError("cloud_wallet_invalid_response")
            errors = payload.get("errors")
            if isinstance(errors, list) and errors:
                duration_ms = elapsed_ms()
                first = errors[0]
                if isinstance(first, dict):
                    error_message = str(first.get("message", "unknown"))
                else:
                    error_message = str(first)
                retry_after_seconds = self._parse_retry_after_seconds(error_message)
                if self._is_rate_limit_error_message(error_message) and attempt < (
                    max_attempts - 1
                ):
                    sleep_seconds = self._backoff_seconds_for_attempt(
                        attempt_index=attempt,
                        retry_after_seconds=retry_after_seconds,
                    )
                    logger.warning(
                        "cloud_wallet_rate_limited graphql_error operation=%s duration_ms=%s "
                        "attempt=%s/%s sleep_seconds=%.1f retry_after_seconds=%s message=%s",
                        operation,
                        duration_ms,
                        attempt + 1,
                        max_attempts,
                        sleep_seconds,
                        retry_after_seconds,
                        error_message,
                    )
                    time.sleep(sleep_seconds)
                    continue
                logger.warning(
                    "cloud_wallet_graphql_error_final operation=%s duration_ms=%s message=%s",
                    operation,
                    duration_ms,
                    error_message,
                )
                raise RuntimeError(f"cloud_wallet_graphql_error:{error_message}")
            data = payload.get("data")
            if not isinstance(data, dict):
                raise RuntimeError("cloud_wallet_missing_data")
            duration_ms = elapsed_ms()
            logger.info(
                "cloud_wallet_graphql_ok operation=%s duration_ms=%s attempt=%s/%s http_timeout_s=%s",
                operation,
                duration_ms,
                attempt + 1,
                max_attempts,
                request_timeout_seconds,
            )
            if slow_threshold_ms > 0 and duration_ms >= slow_threshold_ms:
                logger.warning(
                    "cloud_wallet_graphql_slow operation=%s duration_ms=%s threshold_ms=%s "
                    "http_timeout_s=%s",
                    operation,
                    duration_ms,
                    slow_threshold_ms,
                    request_timeout_seconds,
                )
            return data
        raise RuntimeError("cloud_wallet_rate_limit_retry_exhausted")

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

        completed = subprocess.run(
            [
                "openssl",
                "dgst",
                "-sha256",
                "-sign",
                self._private_key_pem_path,
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

    @staticmethod
    def _random_nonce(length: int) -> str:
        charset = string.ascii_letters + string.digits
        rng = random.SystemRandom()
        return "".join(rng.choice(charset) for _ in range(length))
