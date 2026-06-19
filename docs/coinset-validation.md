# Coinset Validation Runbook

Operator-side validation for Coinset-backed vault scans and coin inventory checks.
GreenFloor operator coin ops use the native manager (`greenfloor-manager coins-list`,
`coin-split`, `coin-combine`); this doc covers **Rust CLI** commands and external CLI parity.

## Scope

- Coinset IO uses `greenfloor-engine coinset` (`post` and `push-tx` subcommands).
- Vault identity and launcher resolution use Rust config load (`program.yaml` via
  `--program-config`). Legacy `cloud_wallet:` blocks are rejected at Rust config load.
- Use Coinset CLI for spot verification against engine output when debugging.

## 1) Probe endpoint capabilities

```bash
cd ~/greenfloor
greenfloor-engine coinset probe \
  --network mainnet \
  --coinset-base-url https://api.coinset.org \
  --program-config ~/.greenfloor/config/program.yaml
```

Optional: pass `--launcher-id-file ~/.greenfloor/cache/vault_launcher_id.txt` instead
of resolving `vault_launcher_id` via `--program-config`.

Expected: batched height-range endpoints report `range_supported: true` when the
host can run incremental vault scans.

## 2) List vault coins

```bash
greenfloor-engine vault-coinset-scan \
  --network mainnet \
  --coinset-base-url https://api.coinset.org \
  --program-config ~/.greenfloor/config/program.yaml \
  --asset-type cat \
  --cat-ticker wUSDC.b
```

Checkpointed and incremental scan logic lives in `greenfloor-engine vault-coinset-scan`
(Rust). Ticker→asset indexes are built from `config/cats.yaml` and `config/markets.yaml`
inside the engine (same metadata as `cats-fields` / `markets-fields`).

## 3) Manager inventory (preferred for operators)

```bash
greenfloor-manager coins-list
greenfloor-manager coin-status
greenfloor-manager coins-list --asset wUSDC.b
```

These use the Rust engine Coinset client against the market receive address scope.

## 4) CAT dust combine (manager)

For sub-unit CAT dust on enabled markets:

```bash
PATH="$(pwd)/.venv/bin:$PATH" greenfloor-manager combine-market-cat-dust \
  --program-config ~/.greenfloor/config/program.yaml \
  --markets-config ~/.greenfloor/config/markets.yaml \
  --json \
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
