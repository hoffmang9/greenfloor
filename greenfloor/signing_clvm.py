"""CLVM puzzle run + AGG_SIG condition parsing helpers for wallet signing."""

from __future__ import annotations

import hashlib
from typing import Any


def _int_to_clvm_bytes(value: int) -> bytes:
    if value <= 0:
        return b""
    size = (int(value).bit_length() + 7) // 8
    return int(value).to_bytes(size, "big", signed=False)


def _domain_bytes_for_agg_sig_kind(kind: str, agg_sig_me_additional_data: bytes) -> bytes | None:
    kind_l = kind.strip().lower()
    if kind_l == "unsafe":
        return None
    if kind_l == "me":
        return agg_sig_me_additional_data
    suffix_map = {
        "parent": 43,
        "puzzle": 44,
        "amount": 45,
        "puzzle_amount": 46,
        "parent_amount": 47,
        "parent_puzzle": 48,
    }
    suffix = suffix_map.get(kind_l)
    if suffix is None:
        return None
    hasher = hashlib.sha256()
    hasher.update(agg_sig_me_additional_data)
    hasher.update(bytes([suffix]))
    return hasher.digest()


def _extract_required_bls_targets_for_coin_spend(
    *,
    sdk: Any,
    coin_spend: Any,
    agg_sig_me_additional_data: bytes,
) -> list[tuple[bytes, bytes]]:
    clvm = sdk.Clvm()
    puzzle = clvm.deserialize(coin_spend.puzzle_reveal)
    solution = clvm.deserialize(coin_spend.solution)
    output = puzzle.run(solution, 11_000_000_000, False)
    conditions = output.value.to_list() or []

    coin = coin_spend.coin
    return _extract_required_bls_targets_for_conditions(
        conditions=conditions,
        coin=coin,
        agg_sig_me_additional_data=agg_sig_me_additional_data,
    )


def _extract_required_bls_targets_for_conditions(
    *,
    conditions: list[Any],
    coin: Any,
    agg_sig_me_additional_data: bytes,
) -> list[tuple[bytes, bytes]]:
    parent = bytes(coin.parent_coin_info)
    puzzle_hash = bytes(coin.puzzle_hash)
    amount = _int_to_clvm_bytes(int(coin.amount))
    coin_id = bytes(coin.coin_id())
    parser_specs = [
        ("parent", "parse_agg_sig_parent", parent),
        ("puzzle", "parse_agg_sig_puzzle", puzzle_hash),
        ("amount", "parse_agg_sig_amount", amount),
        ("puzzle_amount", "parse_agg_sig_puzzle_amount", puzzle_hash + amount),
        ("parent_amount", "parse_agg_sig_parent_amount", parent + amount),
        ("parent_puzzle", "parse_agg_sig_parent_puzzle", parent + puzzle_hash),
        ("unsafe", "parse_agg_sig_unsafe", b""),
        ("me", "parse_agg_sig_me", coin_id),
    ]

    targets: list[tuple[bytes, bytes]] = []
    for condition in conditions:
        for kind, parser_name, appended_info in parser_specs:
            parser = getattr(condition, parser_name, None)
            if parser is None:
                continue
            try:
                parsed = parser()
            except Exception:
                parsed = None
            if parsed is None:
                continue
            public_key = bytes(parsed.public_key.to_bytes())
            raw_message = bytes(parsed.message)
            domain = _domain_bytes_for_agg_sig_kind(kind, agg_sig_me_additional_data)
            full_message = raw_message + appended_info + (domain or b"")
            targets.append((public_key, full_message))
    return targets
