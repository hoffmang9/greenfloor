# Operator scripts

Python utilities for one-time vault setup (`create_kms_vault.py`) and subprocess
adapters under `greenfloor_scripts/`. Operator coin ops, vault scans, and Coinset
probes use native Rust binaries (`greenfloor-engine`, `greenfloor-manager`);
see `docs/runbook.md`.

## Config access

Scripts must not walk operator YAML for policy fields. Call native manager commands
directly:

| Need                        | Rust command                                     |
| --------------------------- | ------------------------------------------------ |
| Program/signer/vault fields | `greenfloor-manager program-fields --json`       |
| Enabled markets             | `greenfloor-manager markets-fields --json`       |
| CAT catalog / symbol map    | `greenfloor-manager cats-fields --json`          |
| Validate before operations  | `greenfloor-manager config-validate`             |
| Test minimal program.yaml   | `greenfloor-manager materialize-minimal-program` |

## Native binary resolution

Scripts resolve `greenfloor-engine` / `greenfloor-manager` via `scripts/greenfloor_scripts/binaries.py`.
`resolve_*_binary(build_if_missing=True)` can auto-run `cargo build` when binaries are missing.
`engine_subprocess.run_engine_json()` uses `build_if_missing=False` so Coinset/hex/KMS CLI calls
fail fast with `engine_cli_binary_unavailable` unless binaries were built or env overrides are set
(`GREENFLOOR_ENGINE_BIN`, etc.).

## Coinset vault inventory and coin ops (Rust)

- `greenfloor-engine coinset probe` — probe Coinset height-window API support for vault scans.
- `greenfloor-engine vault-coinset-scan` — nonce member puzzle hash scan via Coinset (checkpointed).
- `greenfloor-manager combine-market-cat-dust` — batch dust combine for enabled market CAT assets.

Legacy `cloud_wallet:` blocks are rejected at Rust config load.

See also `docs/coinset-validation.md`.

## Vault bootstrap

- `create_kms_vault.py` — create a new ent-wallet vault with KMS P-256 custody + BLS
  recovery (one-time operator setup).

## Test fixtures

Export signer golden fixtures with the Rust test harness:

```bash
EXPORT_SIGNER_FIXTURES=1 cargo test -p greenfloor-engine export_signer_fixtures_to_disk
```

## Remaining Python adapters

`greenfloor_scripts/` keeps subprocess bridges used by `create_kms_vault.py` and adapter
unit tests:

- `binaries.py` — resolve native operator binaries
- `engine_subprocess.py` — `greenfloor-engine` JSON CLI bridge
- `coinset_subprocess.py` — `greenfloor-engine coinset …` bridge
- `hex_subprocess.py` — `greenfloor-engine hex …` bridge
- `kms_subprocess.py` — KMS public-key CLI bridge
- `ent_wallet_graphql.py` — one-time ent-wallet GraphQL client

## Removed scripts (Cloud Wallet retirement)

The following read-only diagnostics required the deleted Cloud Wallet GraphQL API and
were removed:

- `reconcile_byc_wusdc.py` — wallet asset totals vs creator offers reconciliation.
- `trace_locked_quote_coins.py` — locked quote coin lineage / reservation inspection.
- `combine_coinset_direct.py` — superseded by `greenfloor-manager coin-combine`.

Removed Python passthroughs and dead adapters (use native binaries instead):

- `list_vault_coins_coinset.py` → `greenfloor-engine vault-coinset-scan`
- `combine_market_cat_dust_coinset.py` → `greenfloor-manager combine-market-cat-dust`
- `probe_coinset_capabilities.py` → `greenfloor-engine coinset probe`
- `export_signer_fixtures.py` → `EXPORT_SIGNER_FIXTURES=1 cargo test …`
- `config_subprocess.py` / `manager_subprocess.py` — no remaining script callers

For inventory and coin operations on the signer path, use `greenfloor-manager coins-list`,
`coin-split`, and `coin-combine`.
