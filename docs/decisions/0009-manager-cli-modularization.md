# 0009 - Manager CLI Modularization

## Status

Accepted (2026-05-26)

## Decision

Split the former monolithic `greenfloor/cli/manager.py` into focused CLI modules with shared runtime layers. CLI modules parse args, call runtime, and print JSON; daemon and tests import runtime, not CLI.

### CLI router

- `greenfloor/cli/manager.py` — argparse + dispatch only.

### Coin operations

- `greenfloor/cli/coin_ops_list.py` — `coins-list`, `coin-status`, `seed-wallet-assets-cache`.
- `greenfloor/cli/coin_ops_split.py` — `coin-split`.
- `greenfloor/cli/coin_ops_combine.py` — `coin-combine`.
- `greenfloor/cli/coin_ops.py` — thin re-exports for manager/tests.

### Other CLI modules

- `greenfloor/cli/cats.py`, `keys_onboard.py`, `manager_setup.py`, `offers_lifecycle.py`, `offer_build_post.py`, `prompts.py`.

### Shared runtime

- `greenfloor/runtime/cloud_wallet/coin_op_errors.py` — unified `coin_op_error_payload()` and named error builders.
- `greenfloor/runtime/cloud_wallet/coin_ops_runtime.py` — setup, fee resolution, iteration loop, typed `MarketConfig`/`ProgramConfig` boundaries.
- `greenfloor/runtime/cloud_wallet/coin_ops_steps.py` — split/combine step bodies; returns `CoinOpIterationNeedsConfirmation` (no CLI prompts).
- `greenfloor/runtime/cloud_wallet/coin_ops_cli.py` — shared CLI orchestration (`execute_coin_op_cli`, split confirmation wrapper).
- `greenfloor/runtime/cloud_wallet/coin_ops_execution.py` — `combine_coins_with_retry()` (CLI combine + daemon).
- `greenfloor/runtime/cloud_wallet/coin_ops_refresh.py` — on-chain refresh split after off-chain cancel.
- `greenfloor/runtime/cloud_wallet/coins.py` — spendable/scoped coin selection (`filter_spendable_scoped_coins`, etc.; CLI + daemon).
- `greenfloor/runtime/cloud_wallet/offers.py` — offer list/cancel selection helpers.
- `greenfloor/runtime/cloud_wallet/assets.py` — canonical wallet asset GraphQL helpers (`RESOLVE_WALLET_ASSETS_QUERY`, asset amounts).
- `greenfloor/runtime/offer_reconciliation.py` — Dexie/Coinset offer reconciliation (used by CLI and daemon tests).
- `greenfloor/core/coin_ops_policy.py` — deterministic min-amount policy (CLI + daemon).

### Config helpers (`greenfloor/config/io.py`)

- `resolve_state_db_path()` — canonical SQLite path for daemon and CLI.
- `resolve_market_for_build()` — market selection by id or pair (CLI + runtime).

### `CoinOpDeps`

`CoinOpDeps` is an explicit **test/DI seam**, not a production abstraction layer. Methods delegate at call time (not import time) so tests can monkeypatch underlying module functions. Production code uses `DEFAULT_COIN_OP_DEPS`; tests patch `greenfloor.runtime.coinset_runtime`, `greenfloor.runtime.cloud_wallet.adapter`, etc.

### Import rules

- Daemon and integration tests **must not** import `greenfloor.cli.*`.
- Operator scripts should depend on `runtime/` and `config/`, not CLI internals.
- `format_json_output()` is the public JSON formatting helper on `greenfloor.runtime.cloud_wallet.adapter`.

## Consequences

- No single CLI coin-op file may exceed ~600 lines; split/combine share `execute_coin_op_cli()` and `run_coin_op_iteration_loop()`.
- Interactive confirmations are handled in `coin_ops_cli.py`; runtime steps return `CoinOpIterationNeedsConfirmation`.
- New coin-op or reconcile behavior lands in runtime first; CLI wraps it.
- Daemon coin-op selection uses the same `filter_spendable_scoped_coins()` helpers as CLI steps.
