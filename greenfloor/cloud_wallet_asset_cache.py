"""Disk cache for Cloud Wallet ``resolveWalletAssets`` list (wallet asset catalog).

Cached under ``<program_home_dir>/cache/`` so daemon/manager restarts reuse the
last successful fetch within a TTL (default 12 hours).

Environment:
    GREENFLOOR_CLOUD_WALLET_ASSETS_CACHE_TTL_SECONDS — min 60; default 43200 (12h).
"""

from __future__ import annotations

import json
import logging
import os
import time
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

logger = logging.getLogger(__name__)

SCHEMA_VERSION = 1


def wallet_assets_cache_ttl_seconds() -> int:
    raw = os.getenv("GREENFLOOR_CLOUD_WALLET_ASSETS_CACHE_TTL_SECONDS", "").strip()
    if raw:
        return max(60, int(raw))
    return 12 * 3600


def wallet_assets_cache_dir(home_dir: str) -> Path:
    path = (Path(home_dir).expanduser() / "cache").resolve()
    path.mkdir(parents=True, exist_ok=True)
    return path


def _cache_stem(*, base_url: str, vault_id: str) -> str:
    host = (urlparse(base_url).netloc or "unknown").replace(":", "_")
    safe_vault = "".join(c if c.isalnum() or c in "_-" else "_" for c in vault_id.strip())
    return f"wallet_assets_v{SCHEMA_VERSION}_{safe_vault}_{host}"


def wallet_assets_cache_path(home_dir: str, *, base_url: str, vault_id: str) -> Path:
    return (
        wallet_assets_cache_dir(home_dir)
        / f"{_cache_stem(base_url=base_url, vault_id=vault_id)}.json"
    )


def load_wallet_assets_edges(
    home_dir: str,
    *,
    base_url: str,
    vault_id: str,
    ttl_seconds: int | None = None,
) -> list[dict[str, Any]] | None:
    ttl = wallet_assets_cache_ttl_seconds() if ttl_seconds is None else max(60, int(ttl_seconds))
    path = wallet_assets_cache_path(home_dir, base_url=base_url, vault_id=vault_id)
    if not path.is_file():
        return None
    try:
        doc = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    if not isinstance(doc, dict):
        return None
    if int(doc.get("schema_version", 0)) != SCHEMA_VERSION:
        return None
    if str(doc.get("vault_id", "")).strip() != str(vault_id).strip():
        return None
    doc_base = str(doc.get("base_url", "")).strip().rstrip("/")
    if doc_base != str(base_url).strip().rstrip("/"):
        return None
    fetched = float(doc.get("fetched_at_unix", 0))
    if fetched <= 0 or (time.time() - fetched) > ttl:
        return None
    edges = doc.get("edges")
    if not isinstance(edges, list):
        return None
    return edges


def save_wallet_assets_edges(
    home_dir: str,
    *,
    base_url: str,
    vault_id: str,
    edges: list[dict[str, Any]],
) -> None:
    path = wallet_assets_cache_path(home_dir, base_url=base_url, vault_id=vault_id)
    path.parent.mkdir(parents=True, exist_ok=True)
    doc = {
        "schema_version": SCHEMA_VERSION,
        "fetched_at_unix": time.time(),
        "base_url": str(base_url).strip().rstrip("/"),
        "vault_id": str(vault_id).strip(),
        "edges": edges,
    }
    payload = json.dumps(doc, separators=(",", ":"), sort_keys=True).encode("utf-8")
    tmp = path.with_name(f"{path.name}.{os.getpid()}.tmp")
    try:
        tmp.write_bytes(payload)
        os.replace(tmp, path)
    except OSError as exc:
        logger.warning(
            "cloud_wallet_wallet_assets_cache_write_failed path=%s error=%s",
            path,
            exc,
        )
        try:
            tmp.unlink(missing_ok=True)
        except OSError:
            pass
