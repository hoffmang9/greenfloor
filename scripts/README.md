# Operator scripts

Python utilities for Coinset vault scans, one-time vault setup, and test fixtures.
Operator coin ops and offer lifecycle use native Rust binaries (`greenfloor-manager`,
`greenfloord`); see `docs/runbook.md`.

## Coinset vault inventory and coin ops

- `list_vault_coins_coinset.py` — scan vault singleton member puzzle hashes via Coinset.
- `vault_coinset_scan_coinset.py` — checkpointed vault coin scan.
- `vault_coinset_scan_checkpoint.py` — resume or inspect scan checkpoints.
- `vault_coinset_scan_lib.py` — shared scan helpers (imported by scripts above).
- `combine_market_cat_dust_coinset.py` — batch dust combine for enabled market CAT assets.
- `probe_coinset_capabilities.py` — probe Coinset height-window API support for vault scans.

These scripts read `vault.launcher_id` and signer settings from `program.yaml`
(`--program-config`). Legacy `cloud_wallet:` blocks are rejected at config load.

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
