"""Shared PyO3 bridge for the Rust deterministic policy engine.

The compiled extension is published as ``greenfloor_engine`` (ADR 0010). Python
callers should use :func:`import_engine`.

``policy_engine``, ``coin_ops_engine``, and ``bootstrap_engine`` are typed views of
the same PyO3 module — use the name that matches the ``Protocol`` at the call site.

Deterministic policy bridges bind :func:`engine_method_getter` with the matching
typed view and ``missing`` label. Adapters and signing paths call ``import_engine()``
directly.
"""

from __future__ import annotations

import importlib
from collections.abc import Callable
from typing import TYPE_CHECKING, Any, Literal, overload

if TYPE_CHECKING:
    from greenfloor.core.coin_ops.engine_protocol import CoinOpsEngineProtocol
    from greenfloor.core.engine_protocol import BootstrapEngineProtocol, PolicyEngineProtocol

ENGINE_MODULE = "greenfloor_engine"

_MATURIN_INSTALL = (
    "`maturin develop --manifest-path greenfloor-engine-pyo3/Cargo.toml` from the repo root"
)

__all__ = [
    "ENGINE_MODULE",
    "bootstrap_engine",
    "coin_ops_engine",
    "import_engine",
    "engine_method_getter",
    "engine_rebuild_hint",
    "policy_engine",
    "require_engine_method",
]


def engine_rebuild_hint(*, module: str, missing: str = "required engine") -> str:
    """Operator-facing rebuild message for stale or incomplete PyO3 wheels."""
    return (
        f"{module} extension is missing {missing} symbols. "
        f"Rebuild it (for example: {_MATURIN_INSTALL})."
    )


def import_engine() -> Any:
    try:
        return importlib.import_module(ENGINE_MODULE)
    except ImportError as exc:
        raise ImportError(
            "Rust engine extension is not available "
            f"(tried {ENGINE_MODULE}). "
            f"Install via {_MATURIN_INSTALL}. {ENGINE_MODULE}: {exc}"
        ) from exc


def _loaded_engine_module() -> Any:
    return import_engine()


@overload
def typed_engine_view() -> PolicyEngineProtocol: ...


@overload
def typed_engine_view(*, kind: Literal["coin_ops"]) -> CoinOpsEngineProtocol: ...


@overload
def typed_engine_view(*, kind: Literal["bootstrap"]) -> BootstrapEngineProtocol: ...


def typed_engine_view(*, kind: str | None = None) -> Any:
    del kind
    return _loaded_engine_module()


def _engine_module_label(engine: Any) -> str:
    name = getattr(engine, "__name__", None)
    if isinstance(name, str) and name:
        return name
    return ENGINE_MODULE


def require_engine_method(engine: Any, method_name: str, *, missing: str) -> Any:
    method = getattr(engine, method_name, None)
    if method is None:
        raise RuntimeError(
            f"{engine_rebuild_hint(module=_engine_module_label(engine), missing=missing)} "
            f"Missing symbol: {method_name}"
        )
    return method


def engine_method_getter(
    load_engine: Callable[[], Any],
    *,
    missing: str,
) -> Callable[[str], Any]:
    def get_engine_method(method_name: str) -> Any:
        return require_engine_method(load_engine(), method_name, missing=missing)

    return get_engine_method


def policy_engine() -> PolicyEngineProtocol:
    return typed_engine_view()


def coin_ops_engine() -> CoinOpsEngineProtocol:
    return typed_engine_view(kind="coin_ops")


def bootstrap_engine() -> BootstrapEngineProtocol:
    return typed_engine_view(kind="bootstrap")
