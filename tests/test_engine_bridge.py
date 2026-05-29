"""Tests for greenfloor.core.engine_bridge ADR 0010 import hygiene."""

from __future__ import annotations

import importlib
import sys
from types import ModuleType
from typing import Any

import pytest

from greenfloor.core import engine_bridge


def test_import_signer_is_import_engine_alias() -> None:
    assert engine_bridge.import_signer is engine_bridge.import_engine


def test_engine_rebuild_hint_uses_module_argument() -> None:
    hint = engine_bridge.engine_rebuild_hint(
        module=engine_bridge.ENGINE_MODULE,
        missing="offer-request",
    )
    assert engine_bridge.ENGINE_MODULE in hint
    assert "maturin develop" in hint
    assert "offer-request" in hint


def test_import_engine_loads_target_module(monkeypatch) -> None:
    calls: list[str] = []

    def _fake_import(name: str) -> ModuleType:
        calls.append(name)
        if name == engine_bridge.ENGINE_MODULE:
            return ModuleType(name)
        raise ImportError(f"missing {name}")

    monkeypatch.setattr(importlib, "import_module", _fake_import)
    mod = engine_bridge.import_engine()
    assert mod.__name__ == engine_bridge.ENGINE_MODULE
    assert calls == [engine_bridge.ENGINE_MODULE]


def test_import_engine_error_lists_module(monkeypatch) -> None:
    def _always_fail(name: str) -> ModuleType:
        raise ImportError(f"missing {name}")

    monkeypatch.setattr(importlib, "import_module", _always_fail)
    with pytest.raises(ImportError, match="greenfloor_engine") as exc_info:
        engine_bridge.import_engine()
    message = str(exc_info.value)
    assert engine_bridge.ENGINE_MODULE in message
    assert "maturin develop" in message


def test_require_engine_method_uses_loaded_module_name() -> None:
    module = ModuleType(engine_bridge.ENGINE_MODULE)
    with pytest.raises(RuntimeError, match=engine_bridge.ENGINE_MODULE) as exc_info:
        engine_bridge.require_engine_method(module, "missing_symbol", missing="required policy")
    assert "Missing symbol: missing_symbol" in str(exc_info.value)
    assert "required policy" in str(exc_info.value)


def test_require_engine_method_with_sys_modules_stub(monkeypatch) -> None:
    """Regression: policy bridges resolve symbols on test stubs without __spec__."""
    stub = ModuleType(engine_bridge.ENGINE_MODULE)
    monkeypatch.setitem(sys.modules, engine_bridge.ENGINE_MODULE, stub)
    with pytest.raises(RuntimeError, match="Missing symbol: bootstrap_block_error"):
        engine_bridge.require_engine_method(
            stub,
            "bootstrap_block_error",
            missing="required policy",
        )


def test_policy_coin_ops_and_bootstrap_engines_share_import(monkeypatch) -> None:
    module = ModuleType(engine_bridge.ENGINE_MODULE)
    monkeypatch.setattr(engine_bridge, "_loaded_engine_module", lambda: module)
    assert engine_bridge.policy_engine() is module
    assert engine_bridge.coin_ops_engine() is module
    assert engine_bridge.bootstrap_engine() is module


def test_engine_method_getter_delegates_to_require_engine_method(monkeypatch) -> None:
    module = ModuleType(engine_bridge.ENGINE_MODULE)
    calls: list[tuple[Any, str, str]] = []
    sentinel = object()

    def _record_require(engine: Any, method_name: str, *, missing: str) -> object:
        calls.append((engine, method_name, missing))
        return sentinel

    monkeypatch.setattr(engine_bridge, "require_engine_method", _record_require)
    getter = engine_bridge.engine_method_getter(lambda: module, missing="offer-request")
    assert getter("normalize_offer_side") is sentinel
    assert calls == [(module, "normalize_offer_side", "offer-request")]
