"""Retry/polling PyO3 protocol surface."""

from __future__ import annotations

from typing import Protocol


class RetryPolicyKernelProtocol(Protocol):
    def parse_rate_limit_retry_seconds(self, error_text: str) -> float | None: ...

    def moderate_retry_sleep_seconds(
        self, current_sleep: float, rate_limit_wait: float | None
    ) -> float: ...

    def moderate_retry_next_sleep(self, current_sleep: float) -> float: ...

    def dexie_invalid_offer_should_retry(
        self, error: str, attempt: int, max_attempts: int
    ) -> bool: ...

    def dexie_invalid_offer_retry_sleep(self, attempt: int, initial_sleep: float) -> float: ...

    def coinset_fee_lookup_retry_sleep(self, attempt: int) -> float: ...

    def poll_exponential_sleep_now(
        self,
        elapsed_seconds: int,
        timeout_seconds: int,
        sleep_seconds: float,
        initial_sleep: float,
        max_sleep: float,
    ) -> float | None: ...

    def poll_exponential_advance_sleep(
        self,
        sleep_seconds: float,
        initial_sleep: float,
        max_sleep: float,
        multiplier: float,
    ) -> float: ...
