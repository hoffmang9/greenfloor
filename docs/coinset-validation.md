# Coinset Validation Runbook

Operator-side validation for Coinset-backed vault scans and coin inventory checks.
GreenFloor operator coin ops use the native manager (`greenfloor-manager coins-list`,
`coin-split`, `coin-combine`); this doc covers **Rust CLI** commands and external CLI parity.

## Scope

- Coinset IO uses `greenfloor-engine coinset` (`post` and `push-tx` subcommands).
- Vault identity and launcher resolution use Rust config load (`program.yaml` via
  `--program-config`). Legacy `cloud_wallet:` blocks are rejected at Rust config load.
- Use Coinset CLI or the **Coinset MCP server** (`.cursor/mcp.json` → `https://mcp.coinset.org/`) for spot
  verification against engine output when debugging. MCP is read-only (no `push_tx`);
  see `docs/COINSET_DOCS_AND_API.md` → **Coinset MCP Server** for tool catalog and
  operating rules. Enable **coinset** in Cursor Settings → MCP after cloning the repo.

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

For vault-wide lineage (all member puzzle hashes, including spent coins), use
`vault-asset-trace` (§4) instead of `coins-list`.

## 4) Per-asset vault lineage trace (manager)

Trace one asset from external **reception** through intermediate vault coins to
**current balance**. Works for XCH and CAT; invoke once per asset.

```bash
greenfloor-manager \
  --program-config ~/.greenfloor/config/program.yaml \
  --markets-config ~/.greenfloor/config/markets.yaml \
  vault-asset-trace --asset xch

greenfloor-manager \
  --program-config ~/.greenfloor/config/program.yaml \
  --markets-config ~/.greenfloor/config/markets.yaml \
  vault-asset-trace --asset wUSDC.b

greenfloor-manager vault-asset-trace --asset <64-char-cat-asset-id-hex>
```

`--asset` accepts `xch` / `txch`, a CAT ticker from `cats.yaml`, or a CAT asset id
hex string (same resolution as `coins-list --asset`).

The command runs an internal vault Coinset scan with **`include_spent: true`**, filtered
to the requested asset, then builds:

- a **parent→child tree** within the scanned coin set (`parent_coin_id` / `child_coin_ids`)
- **merge edges** for combine spends: inputs co-spent at the same block with the same
  vault puzzle hash (`merges[]`, plus `co_input_coin_ids` on each coin)

Output includes `lineage_model: "parent_tree_with_same_block_merge_edges"`.

### JSON output (selected fields)

| Field                                  | Meaning                                                                        |
| -------------------------------------- | ------------------------------------------------------------------------------ |
| `lineage_model`                        | Graph semantics for clients (`parent_tree_with_same_block_merge_edges`)        |
| `lineage_coin_count`                   | Coins in the lineage graph (after row normalization)                           |
| `scan.scanned_row_count`               | Raw vault scan rows returned for this asset filter                             |
| `current_balance.unspent_coin_count`   | Live unspent coins for this asset in the scan                                  |
| `current_balance.unspent_amount_mojos` | Sum of unspent amounts (mojos; 1000 mojos = 1 CAT unit)                        |
| `reception_count`                      | Coins whose parent is outside this asset’s scanned set (external entry)        |
| `merge_count`                          | Same-block multi-input combine events detected in the scan                     |
| `coins[]`                              | Every matching vault coin with lineage and merge metadata                      |
| `chains[]`                             | Paths from each reception coin to a terminal coin (`path` is ordered coin ids) |
| `merges[]`                             | Combine events: `input_coin_ids` co-spent → `output_coin_ids`                  |

**Coin fields** in `coins[]`:

- `parent_coin_id` / `child_coin_ids` — parent-link tree edges
- `co_input_coin_ids` — sibling inputs in the same combine (same spent block + puzzle hash)

**Coin roles** in `coins[].role` (same value on matching `chains[].terminal_role`):

- `reception` — first vault-visible coin in a branch (parent not in this asset scan).
  Unspent reception coins still count toward `current_balance`.
- `current` — unspent internal coin; contributes to `current_balance`
- `internal` — spent with descendants still in the vault for this asset
- `exit` — spent with no descendants in the scan (left vault or terminal spend)

### Scope and limits

- **Vault-wide**, not scoped to a single market `receive_address` (same nonce-member
  puzzle hash discovery as `greenfloor-engine vault-coinset-scan`).
- Requires signer config in `program.yaml` (KMS + `vault.launcher_id`).
- Default `--max-nonce 100`; raise if the vault has many member keys and the scan
  stops early (`scan.scan_stop_reason` in output).
- Optional overrides match other vault scan commands: `--network`, `--coinset-base-url`,
  `--launcher-id`, `--launcher-id-file`.
- **Merge heuristics:** `merges[]` clusters inputs co-spent in the same block with the
  same vault puzzle hash. Unrelated coins of the same asset batch-spent in the same block
  may appear as a merge even when they were not combined; combines spanning blocks or with
  only partial inputs in the scan set may be missed. Treat `merge_count` as a hint, not
  spend-bundle truth.

Compare spot totals against market inventory when debugging:

```bash
greenfloor-manager coin-status --asset wUSDC.b --market-id <id>
greenfloor-manager vault-asset-trace --asset wUSDC.b
```

## 5) CAT dust combine (manager)

For sub-unit CAT dust on enabled markets:

```bash
PATH="$(pwd)/.venv/bin:$PATH" greenfloor-manager combine-market-cat-dust \
  --program-config ~/.greenfloor/config/program.yaml \
  --markets-config ~/.greenfloor/config/markets.yaml \
  --json \
  --dry-run
```

See also runbook §2 steady-state operations.

## 6) Coinset CLI parity checks

```bash
coinset get_coin_records_by_puzzle_hashes <p2_hash_hex> --include-spent-coins
coinset get_coin_records_by_hints <p2_hash_hex> --include-spent-coins
coinset get_coin_record_by_name <coin_id_hex>
```

Reference: [coinset CLI SKILL.md](https://raw.githubusercontent.com/coinset-org/cli/refs/heads/main/SKILL.md)

### MCP parity checks (read-only, agent/IDE)

When debugging from Cursor or another MCP client, equivalent spot checks:

| Goal                      | MCP tool                | Key parameters                                                |
| ------------------------- | ----------------------- | ------------------------------------------------------------- |
| Coins at puzzle hash      | `find_coins`            | `by=puzzle_hash`, `id=<hex or address>`, `include_spent=true` |
| Coins by hint             | `find_coins`            | `by=hint`, `id=<hint hex>`, `include_spent=true`              |
| Single coin record        | `find_coins`            | `by=name`, `id=<coin_id_hex>`                                 |
| Address balance           | `get_address_balance`   | `address=<xch1…>`                                             |
| Mempool tx ids            | `mempool`               | `action=list`                                                 |
| Tx lifecycle after submit | `wait_for_confirmation` | `tx_id=<0x-hex>`                                              |
| Offer decode              | `decode_offer`          | `offer=offer1…`                                               |

MCP operating rules: always check `success`; prefer confirmed block data over mempool;
pass `include_spent=true` when tracing coin history. Full catalog:
`docs/COINSET_DOCS_AND_API.md` → **Coinset MCP Server**.

## 7) Failure handling

- If batched range support is false, run full-window scans without incremental mode.
- If Coinset returns transient TLS/edge errors, rerun with an existing checkpoint to resume.
- Override endpoint with `GREENFLOOR_COINSET_BASE_URL` (see `docs/runbook.md` §6).
