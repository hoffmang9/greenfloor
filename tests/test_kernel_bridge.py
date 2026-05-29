"""Tests for greenfloor.core.kernel_bridge ADR 0010 import hygiene."""

from __future__ import annotations

import importlib
import sys
from types import ModuleType
from typing import Any

import pytest

from greenfloor.core import kernel_bridge


def test_import_signer_is_import_kernel_alias() -> None:
    assert kernel_bridge.import_signer is kernel_bridge.import_kernel


def test_kernel_rebuild_hint_uses_module_argument() -> None:
    hint = kernel_bridge.kernel_rebuild_hint(
        module=kernel_bridge.KERNEL_MODULE,
        missing="offer-request",
    )
    assert kernel_bridge.KERNEL_MODULE in hint
    assert "maturin develop" in hint
    assert "offer-request" in hint


def test_import_kernel_loads_target_module(monkeypatch) -> None:
    calls: list[str] = []

    def _fake_import(name: str) -> ModuleType:
        calls.append(name)
        if name == kernel_bridge.KERNEL_MODULE:
            return ModuleType(name)
        raise ImportError(f"missing {name}")

    monkeypatch.setattr(importlib, "import_module", _fake_import)
    mod = kernel_bridge.import_kernel()
    assert mod.__name__ == kernel_bridge.KERNEL_MODULE
    assert calls == [kernel_bridge.KERNEL_MODULE]


def test_import_kernel_error_lists_module(monkeypatch) -> None:
    def _always_fail(name: str) -> ModuleType:
        raise ImportError(f"missing {name}")

    monkeypatch.setattr(importlib, "import_module", _always_fail)
    with pytest.raises(ImportError, match="greenfloor_kernel") as exc_info:
        kernel_bridge.import_kernel()
    message = str(exc_info.value)
    assert kernel_bridge.KERNEL_MODULE in message
    assert "maturin develop" in message


def test_require_kernel_method_uses_loaded_module_name() -> None:
    module = ModuleType(kernel_bridge.KERNEL_MODULE)
    with pytest.raises(RuntimeError, match=kernel_bridge.KERNEL_MODULE) as exc_info:
        kernel_bridge.require_kernel_method(module, "missing_symbol", missing="required policy")
    assert "Missing symbol: missing_symbol" in str(exc_info.value)
    assert "required policy" in str(exc_info.value)


def test_require_kernel_method_with_sys_modules_stub(monkeypatch) -> None:
    """Regression: policy bridges resolve symbols on test stubs without __spec__."""
    stub = ModuleType(kernel_bridge.KERNEL_MODULE)
    monkeypatch.setitem(sys.modules, kernel_bridge.KERNEL_MODULE, stub)
    with pytest.raises(RuntimeError, match="Missing symbol: bootstrap_block_error"):
        kernel_bridge.require_kernel_method(
            stub,
            "bootstrap_block_error",
            missing="required policy",
        )


def test_policy_coin_ops_and_bootstrap_kernels_share_import(monkeypatch) -> None:
    module = ModuleType(kernel_bridge.KERNEL_MODULE)
    monkeypatch.setattr(kernel_bridge, "_loaded_kernel_module", lambda: module)
    assert kernel_bridge.policy_kernel() is module
    assert kernel_bridge.coin_ops_kernel() is module
    assert kernel_bridge.bootstrap_kernel() is module


def test_kernel_method_getter_delegates_to_require_kernel_method(monkeypatch) -> None:
    module = ModuleType(kernel_bridge.KERNEL_MODULE)
    calls: list[tuple[Any, str, str]] = []
    sentinel = object()

    def _record_require(kernel: Any, method_name: str, *, missing: str) -> object:
        calls.append((kernel, method_name, missing))
        return sentinel

    monkeypatch.setattr(kernel_bridge, "require_kernel_method", _record_require)
    getter = kernel_bridge.kernel_method_getter(lambda: module, missing="offer-request")
    assert getter("normalize_offer_side") is sentinel
    assert calls == [(module, "normalize_offer_side", "offer-request")]
