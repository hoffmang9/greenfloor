"""Daemon cycle orchestration: single-cycle execution and the long-running loop."""

from __future__ import annotations

import logging
import os
import time
from collections import deque
from collections.abc import Mapping
from pathlib import Path
from typing import Any

from greenfloor.config.io import load_program_config
from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.daemon.bootstrap import (
    initialize_daemon_file_logging,
    log_daemon_event,
    warn_if_daemon_log_level_auto_healed,
)
from greenfloor.daemon.coinset_ws import CoinsetWebsocketClient
from greenfloor.daemon.cycle_market_batch import (
    MarketDispatchState,
)
from greenfloor.daemon.cycle_ws_handlers import build_coinset_websocket_handlers
from greenfloor.daemon.engine_cycle import run_daemon_cycle_once_via_engine
from greenfloor.daemon.inventory_scan import (
    _build_coinset_adapter,
    _resolve_coinset_ws_url,
)
from greenfloor.daemon.market_logging import _daemon_logger


def resolve_cycle_websocket_capture(*, program, loop_websocket_active: bool) -> bool:
    if loop_websocket_active:
        return False
    mode = str(getattr(program, "tx_block_trigger_mode", "websocket"))
    use_websocket = require_engine_method(
        import_engine(),
        "use_websocket_capture_for_trigger_mode",
        missing="daemon websocket capture policy",
    )
    return bool(use_websocket(mode))


def consume_reload_marker(state_dir: Path) -> bool:
    marker = state_dir / "reload_request.json"
    if not marker.exists():
        return False
    marker.unlink(missing_ok=True)
    return True


def run_once(
    program_path: Path,
    markets_path: Path,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
    poll_coinset_mempool: bool = True,
    use_websocket_capture: bool = False,
    program=None,
    testnet_markets_path: Path | None = None,
    market_dispatch_state: MarketDispatchState | None = None,
    test_controls: Mapping[str, Any] | None = None,
) -> int:
    del program
    exit_code, updated_state = run_daemon_cycle_once_via_engine(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        allowed_keys=allowed_keys,
        db_path_override=db_path_override,
        coinset_base_url=coinset_base_url,
        state_dir=state_dir,
        poll_coinset_mempool=poll_coinset_mempool,
        use_websocket_capture=use_websocket_capture,
        market_dispatch_state=market_dispatch_state,
        test_controls=test_controls,
    )
    if market_dispatch_state is not None:
        market_dispatch_state.cursor = updated_state.cursor
        market_dispatch_state.immediate_requeue_ids = deque(updated_state.immediate_requeue_ids)
    return exit_code


def run_loop(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
) -> int:
    current_program = load_program_config(program_path)
    market_dispatch_state = MarketDispatchState()
    initialize_daemon_file_logging(
        current_program.home_dir, log_level=getattr(current_program, "app_log_level", "INFO")
    )
    warn_if_daemon_log_level_auto_healed(program=current_program, program_path=program_path)
    _daemon_logger.info(
        "daemon_starting mode=loop program_config=%s markets_config=%s",
        os.fspath(program_path),
        os.fspath(markets_path),
    )
    from greenfloor.config.io import resolve_state_db_path

    db_path = resolve_state_db_path(
        program_home_dir=current_program.home_dir,
        explicit_db_path=db_path_override,
    )
    coinset = _build_coinset_adapter(program=current_program, coinset_base_url=coinset_base_url)
    ws_url = _resolve_coinset_ws_url(program=current_program, coinset_base_url=coinset_base_url)
    ws_handlers = build_coinset_websocket_handlers(db_path=db_path)

    ws_client = CoinsetWebsocketClient(
        ws_url=ws_url,
        reconnect_interval_seconds=current_program.tx_block_websocket_reconnect_interval_seconds,
        on_mempool_tx_ids=ws_handlers.on_mempool_tx_ids,
        on_confirmed_tx_ids=ws_handlers.on_confirmed_tx_ids,
        on_audit_event=ws_handlers.on_audit_event,
        on_observed_coin_ids=ws_handlers.on_observed_coin_ids,
        recovery_poll=coinset.get_all_mempool_tx_ids,
    )
    ws_client.start()

    try:
        while True:
            initialize_daemon_file_logging(
                current_program.home_dir,
                log_level=getattr(current_program, "app_log_level", "INFO"),
            )
            warn_if_daemon_log_level_auto_healed(program=current_program, program_path=program_path)
            exit_code = run_once(
                program_path=program_path,
                markets_path=markets_path,
                allowed_keys=allowed_keys,
                db_path_override=db_path_override,
                coinset_base_url=coinset_base_url,
                state_dir=state_dir,
                poll_coinset_mempool=False,
                use_websocket_capture=False,
                program=current_program,
                testnet_markets_path=testnet_markets_path,
                market_dispatch_state=market_dispatch_state,
            )
            if exit_code != 0:
                _daemon_logger.warning("daemon_cycle_exit_code=%s", exit_code)
            if consume_reload_marker(state_dir):
                log_daemon_event(level=logging.INFO, payload={"event": "config_reloaded"})
            time.sleep(max(1, current_program.runtime_loop_interval_seconds))
            current_program = load_program_config(program_path)
    except KeyboardInterrupt:
        return 0
    finally:
        ws_client.stop()
        _daemon_logger.info("daemon_stopped mode=loop")
    return 0
