# Coinset Validation Runbook

This runbook defines the operator-side validation loop for Coinset-backed vault scans.

## Scope

- Keep GreenFloor runtime logic in Python adapters/scripts.
- Use Coinset CLI as an external verification and triage tool.
- Validate endpoint capability on each host before relying on incremental scan mode.

## 1) Probe endpoint capabilities on host

Run this first on each runtime host (for example John-Deere):

```bash
cd ~/greenfloor
.venv/bin/python scripts/probe_coinset_capabilities.py \
  --network mainnet \
  --coinset-base-url https://api.coinset.org \
  --launcher-id-file ~/.greenfloor/cache/vault_launcher_id.txt \
  --cloud-wallet-base-url "$(yq -r '.cloud_wallet.base_url' ~/.greenfloor/config/program.yaml)" \
  --cloud-wallet-user-key-id "$(yq -r '.cloud_wallet.user_key_id' ~/.greenfloor/config/program.yaml)" \
  --cloud-wallet-private-key-pem-path "$(yq -r '.cloud_wallet.private_key_pem_path' ~/.greenfloor/config/program.yaml)" \
  --vault-id "$(yq -r '.cloud_wallet.vault_id' ~/.greenfloor/config/program.yaml)"
```

Expected: `capabilities.*.range_supported` should be `true` for batched endpoints.

## 2) Run full scan with checkpoint

```bash
cd ~/greenfloor
.venv/bin/python scripts/list_vault_coins_coinset.py \
  --network mainnet \
  --coinset-base-url https://api.coinset.org \
  --launcher-id-file ~/.greenfloor/cache/vault_launcher_id.txt \
  --asset-type cat \
  --cat-ticker wUSDC.b \
  --max-nonce 64 \
  --nonce-batch-size 32 \
  --parent-lookup-batch-size 64 \
  --checkpoint-file ~/.greenfloor/cache/vault_coinset_checkpoint.json \
  --checkpoint-save-interval 32 \
  --combine-dust --combine-dry-run
```

## 3) Run incremental checkpoint scan

```bash
cd ~/greenfloor
.venv/bin/python scripts/list_vault_coins_coinset.py \
  --network mainnet \
  --coinset-base-url https://api.coinset.org \
  --launcher-id-file ~/.greenfloor/cache/vault_launcher_id.txt \
  --asset-type cat \
  --cat-ticker wUSDC.b \
  --max-nonce 64 \
  --nonce-batch-size 32 \
  --parent-lookup-batch-size 64 \
  --checkpoint-file ~/.greenfloor/cache/vault_coinset_checkpoint.json \
  --incremental-from-checkpoint \
  --combine-dust --combine-dry-run
```

Expected: output `checkpoint.resumed=true`, narrowed `scan_window`, and significantly lower runtime than first full scan.

## 4) Optional Coinset CLI parity checks

Use Coinset CLI for spot verification against script output:

```bash
coinset get_coin_records_by_puzzle_hashes <p2_hash_hex> --include-spent-coins
coinset get_coin_records_by_hints <p2_hash_hex> --include-spent-coins
coinset get_coin_record_by_name <coin_id_hex>
```

Reference CLI skill:
[coinset CLI SKILL.md](https://raw.githubusercontent.com/coinset-org/cli/refs/heads/main/SKILL.md)

## 5) Failure handling

- If batched range support is false, run full-window scans without incremental mode.
- If Coinset returns transient TLS/edge errors, rerun with existing checkpoint to resume quickly.
- If `scan_window_exhausted`, no new height range is available since last sync.
