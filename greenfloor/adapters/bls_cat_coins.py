"""BLS CAT coin discovery via ``greenfloor_signer`` (canonical Rust coinset parse path).

Returns lightweight stand-in objects with ``.coin`` and ``.info.asset_id`` shaped like
``chia_wallet_sdk`` ``Cat`` instances. They are **not** full SDK ``Cat`` values — callers
that need puzzle parsing or driver spends must use the Rust signer paths instead.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from typing import Any, Protocol, cast, runtime_checkable


@runtime_checkable
class CatCoinLike(Protocol):
    def coin_id(self) -> bytes: ...


@runtime_checkable
class CatInfoLike(Protocol):
    asset_id: bytes


@runtime_checkable
class CatLike(Protocol):
    coin: CatCoinLike
    info: CatInfoLike


def _hex_to_bytes(value: str) -> bytes:
    raw = value.strip().lower()
    if raw.startswith("0x"):
        raw = raw[2:]
    if len(raw) % 2:
        raw = f"0{raw}"
    return bytes.fromhex(raw)


from greenfloor.core.kernel_bridge import import_kernel

@dataclass(slots=True)
class _CatCoinAdapter:
    parent_coin_info: bytes
    puzzle_hash: bytes
    amount: int

    def coin_id(self) -> bytes:
        return hashlib.sha256(
            self.parent_coin_info
            + self.puzzle_hash
            + int(self.amount).to_bytes(8, "big", signed=False)
        ).digest()


@dataclass(slots=True)
class _CatInfoAdapter:
    asset_id: bytes


@dataclass(slots=True)
class _CatAdapter:
    coin: _CatCoinAdapter
    info: _CatInfoAdapter


def _summary_to_cat(summary: dict[str, Any]) -> _CatAdapter:
    parent = _hex_to_bytes(str(summary["parent_coin_info"]))
    puzzle_hash = _hex_to_bytes(str(summary["puzzle_hash"]))
    amount = int(summary["amount"])
    asset_raw = str(summary.get("asset_id", "")).strip()
    asset_id = _hex_to_bytes(asset_raw) if asset_raw else b""
    return _CatAdapter(
        coin=_CatCoinAdapter(
            parent_coin_info=parent,
            puzzle_hash=puzzle_hash,
            amount=amount,
        ),
        info=_CatInfoAdapter(asset_id=asset_id),
    )


def _fetch_cat_summaries(
    *,
    network: str,
    receive_address: str,
    asset_id: str,
) -> list[dict[str, Any]]:
    signer = import_kernel()
    raw = signer.list_bls_cat_coins(network, receive_address, asset_id)
    if not isinstance(raw, list):
        return []
    return [item for item in raw if isinstance(item, dict)]


def _fetch_cat_summaries_by_ids(*, network: str, coin_ids: list[str]) -> list[dict[str, Any]]:
    signer = import_kernel()
    raw = signer.list_bls_cat_coins_by_ids(network, coin_ids)
    if not isinstance(raw, list):
        return []
    return [item for item in raw if isinstance(item, dict)]


def _list_unspent_cat_coins(
    *,
    sdk: Any,
    network: str,
    receive_address: str,
    asset_id: str,
) -> list[CatLike]:
    """List unspent CAT coins as summary-derived stand-ins (``sdk`` is ignored)."""
    _ = sdk
    summaries = _fetch_cat_summaries(
        network=network,
        receive_address=receive_address,
        asset_id=asset_id,
    )
    return [cast(CatLike, _summary_to_cat(summary)) for summary in summaries]


def _list_unspent_cat_coins_by_ids(
    *,
    sdk: Any,
    network: str,
    coin_ids: list[str],
) -> list[CatLike]:
    """Resolve CAT coins by id as summary-derived stand-ins (``sdk`` is ignored)."""
    _ = sdk
    summaries = _fetch_cat_summaries_by_ids(network=network, coin_ids=coin_ids)
    return [cast(CatLike, _summary_to_cat(summary)) for summary in summaries]
