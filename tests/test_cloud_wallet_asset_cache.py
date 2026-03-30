from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Any

import pytest

from greenfloor.cloud_wallet_asset_cache import (
    SCHEMA_VERSION,
    load_wallet_assets_edges,
    save_wallet_assets_edges,
    wallet_assets_cache_path,
    wallet_assets_cache_ttl_seconds,
)
from greenfloor.runtime.offer_execution import seed_cloud_wallet_assets_cache


def test_wallet_assets_cache_round_trip(tmp_path: Path) -> None:
    home = str(tmp_path / "h")
    base = "https://api.vault.chia.net"
    vault = "Wallet_abc"
    edges = [{"node": {"assetId": "Asset_x", "type": "CAT2", "displayName": "T", "symbol": ""}}]
    save_wallet_assets_edges(home, base_url=base, vault_id=vault, edges=edges)
    path = wallet_assets_cache_path(home, base_url=base, vault_id=vault)
    assert path.is_file()
    loaded = load_wallet_assets_edges(home, base_url=base, vault_id=vault, ttl_seconds=3600)
    assert loaded == edges


def test_wallet_assets_cache_respects_ttl(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    home = str(tmp_path / "h")
    base = "https://api.example.com"
    vault = "Wallet_z"
    edges: list = []
    t0 = 1_000_000.0
    monkeypatch.setattr(time, "time", lambda: t0)
    save_wallet_assets_edges(home, base_url=base, vault_id=vault, edges=edges)
    assert load_wallet_assets_edges(home, base_url=base, vault_id=vault, ttl_seconds=60) == edges
    monkeypatch.setattr(time, "time", lambda: t0 + 120)
    assert load_wallet_assets_edges(home, base_url=base, vault_id=vault, ttl_seconds=60) is None


def test_wallet_assets_cache_rejects_schema_mismatch(tmp_path: Path) -> None:
    home = str(tmp_path / "h")
    path = wallet_assets_cache_path(home, base_url="https://x.example", vault_id="Wallet_q")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(
            {
                "schema_version": SCHEMA_VERSION + 99,
                "fetched_at_unix": time.time(),
                "base_url": "https://x.example",
                "vault_id": "Wallet_q",
                "edges": [],
            }
        ),
        encoding="utf-8",
    )
    assert load_wallet_assets_edges(home, base_url="https://x.example", vault_id="Wallet_q") is None


def test_wallet_assets_cache_ttl_default_is_twelve_hours(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("GREENFLOOR_CLOUD_WALLET_ASSETS_CACHE_TTL_SECONDS", raising=False)
    assert wallet_assets_cache_ttl_seconds() == 12 * 3600


def test_seed_cloud_wallet_assets_cache_writes_file(tmp_path: Path) -> None:
    home = str(tmp_path / "h")
    edges = [{"node": {"assetId": "Asset_1", "type": "CAT2", "displayName": "T", "symbol": ""}}]

    class _FakeWallet:
        vault_id = "Wallet_v"
        _base_url = "https://cw.example"

        def _graphql(self, *, query: str, variables: dict[str, Any]) -> dict[str, Any]:
            assert variables.get("walletId") == "Wallet_v"
            return {"wallet": {"assets": {"edges": edges}}}

    out = seed_cloud_wallet_assets_cache(wallet=_FakeWallet(), program_home_dir=home)
    assert out["edge_count"] == 1
    path = wallet_assets_cache_path(home, base_url="https://cw.example", vault_id="Wallet_v")
    assert Path(out["cache_path"]) == path
    loaded = load_wallet_assets_edges(home, base_url="https://cw.example", vault_id="Wallet_v")
    assert loaded == edges


def test_seed_cloud_wallet_assets_cache_empty_edges_raises(tmp_path: Path) -> None:
    class _FakeWallet:
        vault_id = "Wallet_v"
        _base_url = "https://cw.example"

        def _graphql(self, *, query: str, variables: dict[str, Any]) -> dict[str, Any]:
            return {"wallet": {"assets": {"edges": []}}}

    with pytest.raises(RuntimeError, match="empty_edges"):
        seed_cloud_wallet_assets_cache(wallet=_FakeWallet(), program_home_dir=str(tmp_path / "h"))
