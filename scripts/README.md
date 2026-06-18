# Operator scripts

Python utilities for Coinset vault scans, one-time vault setup, and test fixtures.
Operator coin ops and offer lifecycle use native Rust binaries (`greenfloor-manager`,
`greenfloord`); see `docs/runbook.md`.

## Config access (scripts)

Scripts must not walk operator YAML for policy fields. Use `scripts/lib/config_subprocess.py`:

| Need                         | Adapter                                           | Rust command                                     |
| ---------------------------- | ------------------------------------------------- | ------------------------------------------------ |
| Program/signer/vault fields  | `load_program_fields()`                           | `greenfloor-manager program-fields --json`       |
| Enabled markets              | `load_markets_fields()` + `enabled_market_rows()` | `greenfloor-manager markets-fields --json`       |
| All markets (incl. disabled) | `all_market_rows()`                               | same (`markets` array in JSON)                   |
| CAT catalog / symbol map     | `load_cats_fields()` + `symbol_to_asset_id_map()` | `greenfloor-manager cats-fields --json`          |
| Vault launcher id            | `launcher_id_from_program_config()`               | via `program-fields`                             |
| Validate before scan         | `ensure_program_config_valid()`                   | `greenfloor-manager config-validate`             |
| Test minimal program.yaml    | `materialize_minimal_program_template()`          | `greenfloor-manager materialize-minimal-program` |

`load_yaml()` in `io.py` is for reading YAML files back in tests after materialization;
it is not operator config validation.

## Coinset vault inventory and coin ops

- `list_vault_coins_coinset.py` — scan vault singleton member puzzle hashes via Coinset.
- `vault_coinset_scan_coinset.py` — checkpointed vault coin scan.
- `vault_coinset_scan_checkpoint.py` — resume or inspect scan checkpoints.
- `vault_coinset_scan_lib.py` — shared scan helpers (imported by scripts above).
- `combine_market_cat_dust_coinset.py` — batch dust combine for enabled market CAT assets.
- `probe_coinset_capabilities.py` — probe Coinset height-window API support for vault scans.

These scripts resolve vault identity and market/CAT metadata through the config adapters
above (`--program-config`, `--markets-config`, optional `--cats-config`). Legacy
`cloud_wallet:` blocks are rejected at Rust config load.

See also `docs/coinset-validation.md`.

## Vault bootstrap

- `create_kms_vault.py` — create a new ent-wallet vault with KMS P-256 custody + BLS
  recovery (one-time operator setup).

## Fixtures

- `export_signer_fixtures.py` — export signer golden fixtures for tests.

## Removed scripts (Cloud Wallet retirement)

The following read-only diagnostics required the deleted Cloud Wallet GraphQL API and
were removed:

- `reconcile_byc_wusdc.py` — wallet asset totals vs creator offers reconciliation.
- `trace_locked_quote_coins.py` — locked quote coin lineage / reservation inspection.
- `combine_coinset_direct.py` — superseded by `greenfloor-manager coin-combine` and
  `combine_market_cat_dust_coinset.py`.

For inventory and coin operations on the signer path, use `greenfloor-manager coins-list`,
`coin-split`, and `coin-combine`.
