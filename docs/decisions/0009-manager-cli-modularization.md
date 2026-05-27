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
- `greenfloor/cli/coin_ops_cli.py` — `_run_coin_op_cli()` plus explicit `execute_split_cli` / `execute_combine_cli`.

### Other CLI modules

- `greenfloor/cli/cats.py` — `cats add/list/delete` commands.
- `greenfloor/cli/cats_catalog.py` — catalog I/O (`load_cats_catalog`, Dexie row metadata helpers).
- `greenfloor/cli/keys_onboard.py`, `manager_setup.py`, `offers_lifecycle.py`, `offer_build_post.py`, `prompts.py`.

### Shared runtime

- `greenfloor/runtime/cloud_wallet/coin_op_errors.py` — unified `coin_op_error_payload()` and named error builders.
- `greenfloor/runtime/cloud_wallet/coin_ops_runtime.py` — setup, fee resolution, iteration loop, typed `MarketConfig`/`ProgramConfig` boundaries.
- `greenfloor/runtime/cloud_wallet/coin_ops_steps.py` — split/combine step bodies; returns `CoinOpIterationNeedsConfirmation` (no CLI prompts).
- `greenfloor/runtime/cloud_wallet/coin_ops_planning.py` — shared split/combine planning (`plan_auto_split_selection`, `plan_auto_combine_inputs`, `select_spendable_coins_for_target_amount`, `SplitPlanningProfile`).
- `greenfloor/runtime/cloud_wallet/coin_ops_selection.py` — low-level coin pickers used by planning.
- `greenfloor/runtime/cloud_wallet/coin_ops_models.py` — typed denomination targets (`SplitDenominationTarget`, `CombineDenominationTarget`), `CoinOpSelectionMode`, `filter_spendable_for_coin_ops()`.
- `greenfloor/runtime/cloud_wallet/coin_ops_daemon_execution.py` — daemon split/combine plan execution (`execute_daemon_split_plan`, `execute_daemon_combine_plan`).
- `greenfloor/runtime/cloud_wallet/coin_ops_daemon_ledger.py` — typed daemon ledger rows (`DaemonCoinOpLedgerItem.to_dict()`).
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

### Split/combine planning profiles

`SplitPlanningProfile` controls auto-select behavior in `plan_auto_split_selection()`:

| Profile       | Required amount               | Sub-CAT dust guard | Combine-for-split prereq                                                     |
| ------------- | ----------------------------- | ------------------ | ---------------------------------------------------------------------------- |
| `CLI_AUTO`    | off (largest min-amount coin) | off                | off                                                                          |
| `DAEMON_AUTO` | on                            | on                 | on (first attempt only; caller passes `allow_combine_prereq=False` on retry) |

CLI split is operator-driven: auto-select picks the largest spendable coin meeting min amount; the operator (or explicit `--coin-id`) owns total-value checks. Daemon split is ladder-driven and must enforce total required value, avoid sub-CAT change dust, and may submit a combine-for-split prereq.

Combine auto-select differs by caller:

- **CLI** uses `CombineInputSelectionMode.LARGEST_BY_AMOUNT` and `filter_spendable_for_coin_ops(..., verify_direct_spendable_lookup=True)` so asset-scoped list rows with wrong metadata are dropped via `get_coin_record`.
- **Daemon** uses `EXACT_AMOUNT` selection on denomination-sized coins and excludes watched coin IDs.

Split auto-select does **not** set `verify_direct_spendable_lookup` (operators may pass explicit coin ids).

## Consequences

- No single CLI coin-op file may exceed ~600 lines; split/combine share `_run_coin_op_cli()` and `run_coin_op_iteration_loop()`.
- Interactive confirmations are handled in `greenfloor/cli/coin_ops_cli.py`; runtime steps return `CoinOpIterationNeedsConfirmation`.
- Split/combine planning logic lives in `coin_ops_planning.py`; CLI steps and daemon execution adapt its results via `SplitPlanningProfile`.
- Daemon coin-op execution uses `coin_ops_daemon_execution.py`; selection uses `filter_spendable_for_coin_ops(..., mode=CoinOpSelectionMode.DAEMON)`.
- CLI coin-op selection uses `filter_spendable_for_coin_ops(..., mode=CoinOpSelectionMode.CLI)`; combine auto-select also sets `verify_direct_spendable_lookup=True`.
- New coin-op or reconcile behavior lands in runtime first; CLI wraps it.
