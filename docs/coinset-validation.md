# Coinset Validation Runbook

Operator-side validation for Coinset-backed vault scans and coin inventory checks.
GreenFloor operator coin ops use the native manager (`greenfloor-manager coins-list`,
`coin-split`, `coin-combine`); this doc covers **scripts** and external CLI parity.

## Scope

- Scripts under `scripts/` use `scripts/greenfloor_scripts/` subprocess adapters for Coinset IO via
  `greenfloor-engine coinset` (`post` and `push-tx` subcommands).
- Vault identity and script config fields come from Rust policy via `greenfloor_scripts/config_subprocess.py`
  adapters (`program-fields` for `vault_launcher_id`; `cats-fields` / `markets-fields` for
  ticker→asset metadata in vault scans). Legacy `cloud_wallet:` blocks are rejected at
  Rust config load.
- Use Coinset CLI for spot verification against script output when debugging.

## 1) Probe endpoint capabilities

```bash
cd ~/greenfloor
.venv/bin/python scripts/probe_coinset_capabilities.py \
  --network mainnet \
  --coinset-base-url https://api.coinset.org \
  --program-config ~/.greenfloor/config/program.yaml
```

Optional: pass `--launcher-id-file ~/.greenfloor/cache/vault_launcher_id.txt` instead
of resolving `vault_launcher_id` via `program-fields` from `--program-config`.

Expected: batched height-range endpoints report `range_supported: true` when the
host can run incremental vault scans.

## 2) List vault coins (script)

```bash
.venv/bin/python scripts/list_vault_coins_coinset.py \
  --network mainnet \
  --coinset-base-url https://api.coinset.org \
  --program-config ~/.greenfloor/config/program.yaml \
  --asset-type cat \
  --cat-ticker wUSDC.b
```

Checkpointed and incremental scan logic lives in `greenfloor-engine vault-coinset-scan`
(Rust). The Python script forwards CLI flags unchanged. Ticker→asset indexes are built from
`config/cats.yaml` and `config/markets.yaml` inside the engine (same metadata as
`cats-fields` / `markets-fields`).

Direct engine usage:

```bash
greenfloor-engine vault-coinset-scan \
  --network mainnet \
  --coinset-base-url https://api.coinset.org \
  --program-config ~/.greenfloor/config/program.yaml \
  --asset-type cat \
  --cat-ticker wUSDC.b
```

## 3) Manager inventory (preferred for operators)

```bash
greenfloor-manager coins-list
greenfloor-manager coin-status
greenfloor-manager coins-list --asset wUSDC.b
```

These use the Rust engine Coinset client against the market receive address scope.

## 4) CAT dust combine (script)

For sub-unit CAT dust on enabled markets:

```bash
.venv/bin/python scripts/combine_market_cat_dust_coinset.py \
  --program-config ~/.greenfloor/config/program.yaml \
  --markets-config ~/.greenfloor/config/markets.yaml \
  --dry-run
```

See also runbook §2 steady-state operations.

## 5) Coinset CLI parity checks

```bash
coinset get_coin_records_by_puzzle_hashes <p2_hash_hex> --include-spent-coins
coinset get_coin_records_by_hints <p2_hash_hex> --include-spent-coins
coinset get_coin_record_by_name <coin_id_hex>
```

Reference: [coinset CLI SKILL.md](https://raw.githubusercontent.com/coinset-org/cli/refs/heads/main/SKILL.md)

## 6) Failure handling

- If batched range support is false, run full-window scans without incremental mode.
- If Coinset returns transient TLS/edge errors, rerun with an existing checkpoint to resume.
- Override endpoint with `GREENFLOOR_COINSET_BASE_URL` (see `docs/runbook.md` §6).
