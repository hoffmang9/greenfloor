from __future__ import annotations

import asyncio
import json
import threading
from collections.abc import Callable
from typing import Any

import aiohttp

from greenfloor.adapters.coinset import extract_coinset_tx_ids_from_offer_payload


def _classify_payload_tx_ids(payload: dict[str, Any]) -> tuple[list[str], list[str]]:
    event_hint = str(payload.get("event") or payload.get("type") or "").strip().lower()
    tx_ids = extract_coinset_tx_ids_from_offer_payload(payload)
    if not tx_ids:
        return [], []
    is_confirmed = (
        bool(payload.get("confirmed", False))
        or bool(payload.get("in_block", False))
        or "confirm" in event_hint
        or "block" in event_hint
    )
    if is_confirmed:
        return [], tx_ids
    return tx_ids, []


class CoinsetWebsocketClient:
    def __init__(
        self,
        *,
        ws_url: str,
        reconnect_interval_seconds: int,
        on_mempool_tx_ids: Callable[[list[str]], None],
        on_confirmed_tx_ids: Callable[[list[str]], None],
        on_audit_event: Callable[[str, dict[str, Any]], None],
        recovery_poll: Callable[[], list[str]] | None = None,
    ) -> None:
        self._ws_url = ws_url
        self._reconnect_interval_seconds = max(1, int(reconnect_interval_seconds))
        self._on_mempool_tx_ids = on_mempool_tx_ids
        self._on_confirmed_tx_ids = on_confirmed_tx_ids
        self._on_audit_event = on_audit_event
        self._recovery_poll = recovery_poll
        self._stop_event = threading.Event()
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        if self._thread is not None and self._thread.is_alive():
            return
        self._stop_event.clear()
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def stop(self, *, timeout_seconds: float = 10.0) -> None:
        self._stop_event.set()
        if self._thread is not None:
            self._thread.join(timeout=timeout_seconds)

    def _run(self) -> None:
        asyncio.run(self._run_forever())

    async def _run_forever(self) -> None:
        while not self._stop_event.is_set():
            try:
                self._on_audit_event("coinset_ws_connecting", {"ws_url": self._ws_url})
                timeout = aiohttp.ClientTimeout(total=None, connect=15)
                async with aiohttp.ClientSession(timeout=timeout) as session:
                    async with session.ws_connect(self._ws_url, heartbeat=30) as ws:
                        self._on_audit_event("coinset_ws_connected", {"ws_url": self._ws_url})
                        await self._run_recovery_poll(reason="connected")
                        await self._consume_messages(ws)
            except Exception as exc:
                self._on_audit_event("coinset_ws_disconnected", {"error": str(exc)})
            if self._stop_event.is_set():
                break
            await self._sleep_with_stop(float(self._reconnect_interval_seconds))

    async def _run_recovery_poll(self, *, reason: str) -> None:
        if self._recovery_poll is None:
            return
        try:
            tx_ids = self._recovery_poll()
            self._on_mempool_tx_ids(tx_ids)
            self._on_audit_event(
                "coinset_ws_recovery_poll",
                {"reason": reason, "tx_id_count": len(tx_ids)},
            )
        except Exception as exc:
            self._on_audit_event(
                "coinset_ws_recovery_poll_error",
                {"reason": reason, "error": str(exc)},
            )

    async def _consume_messages(self, ws: aiohttp.ClientWebSocketResponse) -> None:
        while not self._stop_event.is_set():
            try:
                msg = await ws.receive(timeout=1.0)
            except asyncio.TimeoutError:
                continue
            if msg.type == aiohttp.WSMsgType.TEXT:
                self._handle_text_message(msg.data)
                continue
            if msg.type in {aiohttp.WSMsgType.CLOSED, aiohttp.WSMsgType.CLOSE}:
                raise RuntimeError("coinset_ws_closed")
            if msg.type == aiohttp.WSMsgType.ERROR:
                raise RuntimeError(f"coinset_ws_error:{ws.exception()}")

    def _handle_text_message(self, raw_data: str) -> None:
        try:
            payload = json.loads(raw_data)
        except Exception:
            self._on_audit_event("coinset_ws_payload_parse_error", {"raw": raw_data[:200]})
            return
        if not isinstance(payload, dict):
            self._on_audit_event("coinset_ws_payload_ignored", {"kind": type(payload).__name__})
            return
        mempool_tx_ids, confirmed_tx_ids = _classify_payload_tx_ids(payload)
        if mempool_tx_ids:
            self._on_mempool_tx_ids(mempool_tx_ids)
            self._on_audit_event(
                "coinset_ws_mempool_event",
                {"tx_id_count": len(mempool_tx_ids)},
            )
        if confirmed_tx_ids:
            self._on_confirmed_tx_ids(confirmed_tx_ids)
            self._on_audit_event(
                "coinset_ws_tx_block_event",
                {"tx_id_count": len(confirmed_tx_ids)},
            )

    async def _sleep_with_stop(self, seconds: float) -> None:
        remaining = max(0.0, seconds)
        while remaining > 0 and not self._stop_event.is_set():
            chunk = min(1.0, remaining)
            await asyncio.sleep(chunk)
            remaining -= chunk


def capture_coinset_websocket_once(
    *,
    ws_url: str,
    reconnect_interval_seconds: int,
    capture_window_seconds: int,
    on_mempool_tx_ids: Callable[[list[str]], None],
    on_confirmed_tx_ids: Callable[[list[str]], None],
    on_audit_event: Callable[[str, dict[str, Any]], None],
    recovery_poll: Callable[[], list[str]] | None = None,
) -> None:
    client = CoinsetWebsocketClient(
        ws_url=ws_url,
        reconnect_interval_seconds=reconnect_interval_seconds,
        on_mempool_tx_ids=on_mempool_tx_ids,
        on_confirmed_tx_ids=on_confirmed_tx_ids,
        on_audit_event=on_audit_event,
        recovery_poll=recovery_poll,
    )
    stop_event = threading.Event()

    async def _run_once() -> None:
        deadline = asyncio.get_running_loop().time() + float(max(1, capture_window_seconds))
        await client._run_recovery_poll(reason="once_start")
        while asyncio.get_running_loop().time() < deadline:
            try:
                timeout = aiohttp.ClientTimeout(total=None, connect=15)
                async with aiohttp.ClientSession(timeout=timeout) as session:
                    async with session.ws_connect(ws_url, heartbeat=30) as ws:
                        on_audit_event("coinset_ws_once_connected", {"ws_url": ws_url})
                        while asyncio.get_running_loop().time() < deadline and not stop_event.is_set():
                            try:
                                msg = await ws.receive(timeout=1.0)
                            except asyncio.TimeoutError:
                                continue
                            if msg.type == aiohttp.WSMsgType.TEXT:
                                client._handle_text_message(msg.data)
                                continue
                            if msg.type in {aiohttp.WSMsgType.CLOSED, aiohttp.WSMsgType.CLOSE}:
                                raise RuntimeError("coinset_ws_once_closed")
                            if msg.type == aiohttp.WSMsgType.ERROR:
                                raise RuntimeError(f"coinset_ws_once_error:{ws.exception()}")
            except Exception as exc:
                on_audit_event("coinset_ws_once_disconnected", {"error": str(exc)})
            if asyncio.get_running_loop().time() >= deadline:
                break
            await asyncio.sleep(float(max(1, reconnect_interval_seconds)))

    asyncio.run(_run_once())
