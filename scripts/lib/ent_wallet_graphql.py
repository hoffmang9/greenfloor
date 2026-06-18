"""Minimal ent-wallet GraphQL client with chia-user-key PEM authentication."""

from __future__ import annotations

import base64
import json
import random
import string
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


class EntWalletGraphqlClient:
    def __init__(self, *, base_url: str, user_key_id: str, private_key_pem_path: str) -> None:
        self._graphql_url = base_url.rstrip("/") + "/graphql"
        self._user_key_id = user_key_id
        pem_path = Path(private_key_pem_path).expanduser().resolve()
        if ".greenfloor" not in pem_path.parts:
            raise ValueError("ent_wallet_private_key_pem_path_must_be_under_dot_greenfloor")
        if not pem_path.is_file():
            raise FileNotFoundError(f"ent_wallet_private_key_pem_path_not_found:{pem_path}")
        self._private_key_pem_path = str(pem_path)

    @staticmethod
    def _random_nonce(length: int = 10) -> str:
        alphabet = string.ascii_letters + string.digits
        return "".join(random.choice(alphabet) for _ in range(length))

    def _sign_canonical(self, canonical: str) -> str:
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
                "ent_wallet_signature_failed:"
                f"{completed.stderr.decode('utf-8', errors='replace').strip()}"
            )
        return base64.b64encode(completed.stdout).decode("ascii")

    def _build_auth_headers(self, raw_body: str) -> dict[str, str]:
        nonce = self._random_nonce()
        timestamp = str(int(time.time() * 1000))
        canonical = f"{raw_body}{nonce}{timestamp}"
        return {
            "chia-user-key-id": self._user_key_id,
            "chia-signature": self._sign_canonical(canonical),
            "chia-nonce": nonce,
            "chia-timestamp": timestamp,
        }

    def graphql(self, *, query: str, variables: dict[str, Any]) -> dict[str, Any]:
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
            with urllib.request.urlopen(req, timeout=15) as resp:
                payload = json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            raw = exc.read().decode("utf-8", errors="replace").strip()
            raise RuntimeError(f"ent_wallet_http_error:{exc.code}:{raw[:200]}") from exc
        except urllib.error.URLError as exc:
            raise RuntimeError(f"ent_wallet_network_error:{exc.reason}") from exc
        if not isinstance(payload, dict):
            raise RuntimeError("ent_wallet_invalid_response")
        errors = payload.get("errors")
        if isinstance(errors, list) and errors:
            first = errors[0]
            message = first.get("message", "unknown") if isinstance(first, dict) else str(first)
            raise RuntimeError(f"ent_wallet_graphql_error:{message}")
        data = payload.get("data")
        if not isinstance(data, dict):
            raise RuntimeError("ent_wallet_missing_data")
        return data
