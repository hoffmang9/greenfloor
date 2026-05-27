# Operator scripts

## Coinset vault inventory and coin ops

- `list_vault_coins_coinset.py` — scan vault singleton member puzzle hashes via Coinset.
- `combine_coinset_direct.py` — combine explicit CAT coin IDs via BLS mixed-split + Coinset broadcast.
- `combine_market_cat_dust_coinset.py` — batch dust combine for enabled market CAT assets.
- `probe_coinset_capabilities.py` — probe Coinset height-window API support for vault scans.

These scripts read `vault.launcher_id` and signer settings from `program.yaml` (`--program-config`).

## Vault bootstrap

- `create_kms_vault.py` — create a new ent-wallet vault with KMS P-256 custody + BLS recovery (one-time operator setup).

## Fixtures

- `export_signer_fixtures.py` — export signer golden fixtures for tests.

## Removed scripts (Cloud Wallet retirement)

The following read-only diagnostics required the deleted Cloud Wallet GraphQL API and were removed:

- `reconcile_byc_wusdc.py` — wallet asset totals vs creator offers reconciliation.
- `trace_locked_quote_coins.py` — locked quote coin lineage / reservation inspection.

For inventory and coin operations on the signer path, use `greenfloor-manager coins-list`, `coin-split`, `coin-combine`, and the Coinset scripts above.
