"""Coinset scan client with retries for vault and probe scripts."""

from __future__ import annotations

import random
import time
from collections.abc import Callable
from typing import Any, TypeVar

from greenfloor_scripts.chia_sdk_helpers import coin_id_from_record, hex_to_bytes, to_coinset_hex
from greenfloor_scripts.coinset_subprocess import (
    coin_records_cli,
    record_from_cli,
    resolve_client_cli,
)
from greenfloor_scripts.engine_subprocess import is_retryable_engine_cli_error

T = TypeVar("T")


def chunk_values(values: list[str], chunk_size: int) -> list[list[str]]:
    if chunk_size <= 0:
        return [values] if values else []
    return [values[idx : idx + chunk_size] for idx in range(0, len(values), chunk_size)]


def is_retryable_coinset_error(exc: Exception) -> bool:
    return is_retryable_engine_cli_error(exc)


def coinset_with_retries(
    func: Callable[[], T],
    *,
    attempts: int = 4,
    initial_delay_seconds: float = 0.8,
    jitter_ratio: float = 0.25,
    sleep: Callable[[float], None] = time.sleep,
) -> T:
    delay = max(0.1, float(initial_delay_seconds))
    jitter = min(max(0.0, float(jitter_ratio)), 0.9)
    last_exc: Exception | None = None
    for attempt in range(1, max(1, int(attempts)) + 1):
        try:
            return func()
        except Exception as exc:  # noqa: BLE001
            last_exc = exc
            if attempt >= attempts or not is_retryable_coinset_error(exc):
                raise
            sleep_multiplier = 1.0 + random.uniform(-jitter, jitter)
            sleep(max(0.05, delay * sleep_multiplier))
            delay = min(delay * 2.0, 8.0)
    if last_exc is not None:
        raise last_exc
    raise RuntimeError("coinset_retry_logic_unreachable")


class CoinsetScanner:
    def __init__(self, *, network: str, base_url: str | None = None) -> None:
        self.network, self.base_url = resolve_client_cli(network, base_url)

    def _records(
        self,
        endpoint: str,
        body: dict[str, Any],
        *,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return coinset_with_retries(
            lambda: coin_records_cli(
                self.network,
                self.base_url,
                endpoint,
                body,
                start_height=start_height,
                end_height=end_height,
            )
        )

    def _record(
        self,
        endpoint: str,
        body: dict[str, Any],
        key: str,
    ) -> dict[str, Any] | None:
        return coinset_with_retries(
            lambda: record_from_cli(
                self.network,
                self.base_url,
                endpoint,
                body,
                key,
            )
        )

    def get_blockchain_state(self) -> dict[str, Any] | None:
        return self._record("get_blockchain_state", {}, "blockchain_state")

    def by_puzzle_hash(
        self,
        *,
        puzzle_hash: str,
        include_spent: bool,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return self._records(
            "get_coin_records_by_puzzle_hash",
            {
                "puzzle_hash": puzzle_hash,
                "include_spent_coins": include_spent,
            },
            start_height=start_height,
            end_height=end_height,
        )

    def by_puzzle_hashes(
        self,
        *,
        puzzle_hashes: list[str],
        include_spent: bool,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        if not puzzle_hashes:
            return []
        return self._records(
            "get_coin_records_by_puzzle_hashes",
            {
                "puzzle_hashes": puzzle_hashes,
                "include_spent_coins": include_spent,
            },
            start_height=start_height,
            end_height=end_height,
        )

    def by_hint(
        self,
        *,
        hint: str,
        include_spent: bool,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        return self._records(
            "get_coin_records_by_hint",
            {
                "hint": hint,
                "include_spent_coins": include_spent,
            },
            start_height=start_height,
            end_height=end_height,
        )

    def by_hints(
        self,
        *,
        hints: list[str],
        include_spent: bool,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        if not hints:
            return []
        return self._records(
            "get_coin_records_by_hints",
            {
                "hints": hints,
                "include_spent_coins": include_spent,
            },
            start_height=start_height,
            end_height=end_height,
        )

    def by_names(
        self,
        *,
        coin_names: list[str],
        include_spent: bool = True,
        start_height: int | None = None,
        end_height: int | None = None,
    ) -> list[dict[str, Any]]:
        if not coin_names:
            return []
        return self._records(
            "get_coin_records_by_names",
            {
                "names": coin_names,
                "include_spent_coins": include_spent,
            },
            start_height=start_height,
            end_height=end_height,
        )

    def puzzle_and_solution(
        self, *, coin_id_hex: str, height: int | None = None
    ) -> dict[str, Any] | None:
        body: dict[str, Any] = {"coin_id": coin_id_hex}
        if height is not None and height > 0:
            body["height"] = int(height)
        return self._record("get_puzzle_and_solution", body, "coin_solution")

    def existing_coin_names(self, *, coin_ids_hex: list[str]) -> set[str]:
        """Return the subset of coin ids that Coinset resolves by exact name."""
        existing: set[str] = set()
        if not coin_ids_hex:
            return existing
        for batch in chunk_values(coin_ids_hex, 200):
            rows = self.by_names(
                coin_names=[to_coinset_hex(hex_to_bytes(coin_id)) for coin_id in batch],
                include_spent=True,
            )
            for record in rows:
                resolved = coin_id_from_record(record)
                if resolved:
                    existing.add(resolved)
        return existing
