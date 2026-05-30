"""Typed contracts for the Rust daemon PyO3 surface."""

from __future__ import annotations

from typing import Any, Protocol, runtime_checkable


@runtime_checkable
class CoinWatchlistCache(Protocol):
    """Shared in-process coin watchlist cache (Rust-backed)."""


@runtime_checkable
class DaemonDispatchState(Protocol):
    cursor: int
    immediate_requeue_ids: list[str]


@runtime_checkable
class DaemonCycleTestControls(Protocol):
    skip_strategy_execution: bool
    force_market_error_for: str | None


@runtime_checkable
class DaemonRunOnceRequest(Protocol):
    program_path: Any
    markets_path: Any
    coinset_base_url: str
    state_dir: Any
    coin_watchlist: CoinWatchlistCache
    testnet_markets_path: Any | None
    state_db_override: str | None
    poll_coinset_mempool: bool
    use_websocket_capture: bool
    allowed_key_ids: list[str]
    dispatch_state: DaemonDispatchState
    test_controls: DaemonCycleTestControls


@runtime_checkable
class DaemonLoopRequest(Protocol):
    program_path: Any
    markets_path: Any
    coinset_base_url: str
    state_dir: Any
    testnet_markets_path: Any | None
    state_db_override: str | None
    allowed_key_ids: list[str]


@runtime_checkable
class DaemonCycleOnceResponse(Protocol):
    exit_code: int
    dispatch_state: DaemonDispatchState
    cycle_summary: dict[str, Any]


class DaemonEngineTypes(Protocol):
    CoinWatchlistCache: type[CoinWatchlistCache]
    DaemonDispatchState: type[DaemonDispatchState]
    DaemonCycleTestControls: type[DaemonCycleTestControls]
    DaemonRunOnceRequest: type[DaemonRunOnceRequest]
    DaemonLoopRequest: type[DaemonLoopRequest]
    DaemonCycleOnceResponse: type[DaemonCycleOnceResponse]

    def run_daemon_cycle_once(self, request: DaemonRunOnceRequest) -> DaemonCycleOnceResponse: ...
    def run_daemon_loop(self, request: DaemonLoopRequest) -> int: ...
    def reconcile_offers_cli(
        self,
        db_path: str,
        dexie_base_url: str,
        target_venue: str,
        market_id: str | None,
        limit: int,
    ) -> dict[str, Any]: ...
    def resolve_state_db_path(
        self, program_home_dir: str, explicit_db_path: str | None = ...
    ) -> str: ...
    def use_websocket_capture_for_trigger_mode(self, tx_block_trigger_mode: str) -> bool: ...


__all__ = [
    "CoinWatchlistCache",
    "DaemonCycleOnceResponse",
    "DaemonCycleTestControls",
    "DaemonDispatchState",
    "DaemonEngineTypes",
    "DaemonLoopRequest",
    "DaemonRunOnceRequest",
]
