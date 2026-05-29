"""Compatibility shim for coin-op engine protocol names."""

from greenfloor.core.coin_ops.engine_protocol import CoinOpsEngineProtocol

CoinOpsKernelProtocol = CoinOpsEngineProtocol

__all__ = ["CoinOpsKernelProtocol"]
