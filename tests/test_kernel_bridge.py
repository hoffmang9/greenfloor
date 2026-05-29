"""Tests for greenfloor.core.kernel_bridge ADR 0010 import hygiene."""

from __future__ import annotations

import importlib
import importlib.util
import sys
from types import ModuleType

import pytest

from greenfloor.core import kernel_bridge


def test_import_signer_is_import_kernel_alias() -> None:
    assert kernel_bridge.import_signer is kernel_bridge.import_kernel


def test_kernel_rebuild_hint_mentions_resolved_module(monkeypatch) -> None:
    monkeypatch.setattr(
        kernel_bridge,
        "resolved_kernel_module_name",
        lambda: kernel_bridge.KERNEL_MODULE_LEGACY,
    )
    hint = kernel_bridge.kernel_rebuild_hint(missing="offer-request")
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
    assert calls == list(kernel_bridge._KERNEL_MODULE_CANDIDATES)


def test_import_kernel_error_lists_candidates(monkeypatch) -> None:
    def _always_fail(name: str) -> ModuleType:
        raise ImportError(f"missing {name}")

    monkeypatch.setattr(importlib, "import_module", _always_fail)
    with pytest.raises(ImportError, match="greenfloor_signer") as exc_info:
        kernel_bridge.import_kernel()
    message = str(exc_info.value)
    assert kernel_bridge.KERNEL_MODULE_TARGET in message
    assert "maturin develop" in message


def test_resolved_kernel_module_name_uses_find_spec(monkeypatch) -> None:
    monkeypatch.delitem(sys.modules, kernel_bridge.KERNEL_MODULE_LEGACY, raising=False)
    monkeypatch.delitem(sys.modules, kernel_bridge.KERNEL_MODULE_TARGET, raising=False)

    def _find_spec(name: str) -> object | None:
        if name == kernel_bridge.KERNEL_MODULE_TARGET:
            return object()
        return None

    monkeypatch.setattr(importlib.util, "find_spec", _find_spec)
    assert kernel_bridge.resolved_kernel_module_name() == kernel_bridge.KERNEL_MODULE_TARGET


def test_resolved_kernel_module_name_defaults_to_legacy(monkeypatch) -> None:
    monkeypatch.setattr(importlib.util, "find_spec", lambda _name: None)
    assert kernel_bridge.resolved_kernel_module_name() == kernel_bridge.KERNEL_MODULE_LEGACY
