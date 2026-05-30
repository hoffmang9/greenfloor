"""Watchlist timing constants shared with the Rust engine (must stay in sync)."""

# Mirrors `greenfloor-engine/src/daemon/watchlist/time.rs`.
RESEED_MEMPOOL_MAX_AGE_SECONDS = 3 * 60

__all__ = ["RESEED_MEMPOOL_MAX_AGE_SECONDS"]
