"""Tests for greenfloor.core.kernel_bridge ADR 0010 import hygiene."""

from __future__ import annotations

import importlib
import sys
from types import ModuleType

import pytest

from greenfloor.core import kernel_bridge


def test_import_signer_is_import_kernel_alias() -> None:
    assert kernel_bridge.import_signer is kernel_bridge.import_kernel


def test_kernel_rebuild_hint_uses_module_argument() -> None:
    hint = kernel_bridge.kernel_rebuild_hint(
        module=kernel_bridge.KERNEL_MODULE_LEGACY,
        missing="offer-request",
    )
    assert kernel_bridge.KERNEL_MODULE_LEGACY in hint
    assert "maturin develop" in hint
    assert "offer-request" in hint


def test_import_kernel_prefers_legacy_module(monkeypatch) -> None:
    calls: list[str] = []

    def _fake_import(name: str) -> ModuleType:
        calls.append(name)
        if name == kernel_bridge.KERNEL_MODULE_LEGACY:
            return ModuleType(name)
        raise ImportError(f"missing {name}")

    monkeypatch.setattr(importlib, "import_module", _fake_import)
    mod = kernel_bridge.import_kernel()
    assert mod.__name__ == kernel_bridge.KERNEL_MODULE_LEGACY
    assert calls == [kernel_bridge.KERNEL_MODULE_LEGACY]


def test_import_kernel_falls_back_to_target_module(monkeypatch) -> None:
    calls: list[str] = []

    def _fake_import(name: str) -> ModuleType:
        calls.append(name)
        if name == kernel_bridge.KERNEL_MODULE_TARGET:
            return ModuleType(name)
        raise ImportError(f"missing {name}")

    monkeypatch.setattr(importlib, "import_module", _fake_import)
    mod = kernel_bridge.import_kernel()
    assert mod.__name__ == kernel_bridge.KERNEL_MODULE_TARGET
    assert calls == [kernel_bridge.KERNEL_MODULE_LEGACY, kernel_bridge.KERNEL_MODULE_TARGET]


def test_import_kernel_error_lists_candidates(monkeypatch) -> None:
    def _always_fail(name: str) -> ModuleType:
        raise ImportError(f"missing {name}")

    monkeypatch.setattr(importlib, "import_module", _always_fail)
    with pytest.raises(ImportError, match="greenfloor_signer") as exc_info:
        kernel_bridge.import_kernel()
    message = str(exc_info.value)
    assert kernel_bridge.KERNEL_MODULE_TARGET in message
    assert "maturin develop" in message


def test_require_kernel_method_uses_loaded_module_name() -> None:
    module = ModuleType(kernel_bridge.KERNEL_MODULE_LEGACY)
    with pytest.raises(RuntimeError, match=kernel_bridge.KERNEL_MODULE_LEGACY) as exc_info:
        kernel_bridge.require_kernel_method(module, "missing_symbol", missing="required policy")
    assert "Missing symbol: missing_symbol" in str(exc_info.value)
    assert "required policy" in str(exc_info.value)


def test_require_kernel_method_with_sys_modules_stub(monkeypatch) -> None:
    """Regression: policy bridges resolve symbols on test stubs without __spec__."""
    stub = ModuleType(kernel_bridge.KERNEL_MODULE_LEGACY)
    monkeypatch.setitem(sys.modules, kernel_bridge.KERNEL_MODULE_LEGACY, stub)
    with pytest.raises(RuntimeError, match="Missing symbol: bootstrap_block_error"):
        kernel_bridge.require_kernel_method(
            stub,
            "bootstrap_block_error",
            missing="required policy",
        )


def test_policy_coin_ops_and_bootstrap_kernels_share_import(monkeypatch) -> None:
    module = ModuleType(kernel_bridge.KERNEL_MODULE_LEGACY)
    monkeypatch.setattr(kernel_bridge, "import_kernel", lambda: module)
    assert kernel_bridge.policy_kernel() is module
    assert kernel_bridge.coin_ops_kernel() is module
    assert kernel_bridge.bootstrap_kernel() is module
