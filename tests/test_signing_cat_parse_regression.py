from __future__ import annotations

import hashlib

from greenfloor.adapters import bls_cat_coins


def _coin_id_bytes(parent_coin_info: bytes, puzzle_hash: bytes, amount: int) -> bytes:
    return hashlib.sha256(
        parent_coin_info + puzzle_hash + int(amount).to_bytes(8, "big", signed=False)
    ).digest()


def test_list_unspent_cat_coins_by_ids_maps_rust_summaries_to_cat_adapters(monkeypatch) -> None:
    asset_id = "11" * 32
    child_amount = 17634
    inner_puzzle_hash = bytes.fromhex("22" * 32)
    cat_puzzle_hash = bytes.fromhex("33" * 32)
    parent_parent_coin_id = bytes.fromhex("44" * 32)
    parent_puzzle_hash = bytes.fromhex("55" * 32)

    parent_coin_id = _coin_id_bytes(parent_parent_coin_id, parent_puzzle_hash, child_amount)
    child_coin_id = _coin_id_bytes(parent_coin_id, cat_puzzle_hash, child_amount)

    summary = {
        "coin_id": f"0x{child_coin_id.hex()}",
        "parent_coin_info": f"0x{parent_coin_id.hex()}",
        "puzzle_hash": f"0x{cat_puzzle_hash.hex()}",
        "amount": child_amount,
        "p2_puzzle_hash": f"0x{inner_puzzle_hash.hex()}",
        "asset_id": f"0x{asset_id}",
    }

    class _FakeSdk:
        @staticmethod
        def to_hex(value: bytes) -> str:
            return bytes(value).hex()

    def _fake_by_ids(*, network: str, coin_ids: list[str]) -> list[dict]:
        _ = network, coin_ids
        return [summary]

    monkeypatch.setattr(
        "greenfloor.adapters.bls_cat_coins._fetch_cat_summaries_by_ids",
        _fake_by_ids,
    )

    cats = bls_cat_coins._list_unspent_cat_coins_by_ids(
        sdk=_FakeSdk(),
        network="testnet11",
        coin_ids=[child_coin_id.hex()],
    )

    assert len(cats) == 1
    assert _FakeSdk.to_hex(cats[0].coin.coin_id()) == child_coin_id.hex()
    assert _FakeSdk.to_hex(cats[0].info.asset_id) == asset_id
