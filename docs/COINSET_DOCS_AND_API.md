# Coinset Docs & API Reference

## Status

This file is kept as a local snapshot/reference and should not be removed.
Use it for offline notes and endpoint quick-lookups, but treat the Coinset CLI
skill as the canonical operator workflow reference:

- [coinset CLI SKILL.md](https://raw.githubusercontent.com/coinset-org/cli/refs/heads/main/SKILL.md)

For agent/IDE blockchain lookups (read-only), use the Coinset MCP server:

- Project config: `.cursor/mcp.json` (enable **coinset** in Cursor Settings → MCP after clone)
- Endpoint: `https://mcp.coinset.org/` (Streamable HTTP MCP transport; not a browser page)
- Server: `coinset-mcp` v0.1.0
- Upstream docs: `https://www.coinset.org/docs/mcp`

For repo-specific execution and host validation, use:

- `docs/coinset-validation.md`
- `greenfloor-engine coinset probe`

This file summarizes the public docs currently available from `https://www.coinset.org/docs`, the Coinset MCP server at `https://mcp.coinset.org/`, and linked usage pages.

**As of:** 2026-07-08

## Overview

- Coinset positions itself as a free, fast, reliable Chia blockchain API service.
- Mainnet API base URL: `https://api.coinset.org`.
- Testnet11 base URL: `https://testnet11.api.coinset.org`.
- Most documented endpoints use `POST` + JSON body.
- The API supports **HTTP/2** (negotiated automatically by modern HTTP clients).
- Real-time updates are documented via WebSocket at `wss://api.coinset.org/ws` (also referenced as `wss://coinset.org/ws` in older notes).
- Docs site is hosted separately at `https://www.coinset.org/docs`.
- **MCP server** (read-only, agent-friendly): `https://mcp.coinset.org/` — one endpoint serves both mainnet and testnet11; pass `network` on a tool or infer from `xch1…`/`txch1…` addresses.
- **OpenAPI specs** (exact request/response schemas for every proxied endpoint):
  - Full node: `https://www.coinset.org/openapi/full_node.yaml`
  - Coinset extensions: `https://www.coinset.org/openapi/coinset.yaml`

### Network Routing (Explicit)

Use these exact hosts:

```bash
# Docs
curl https://www.coinset.org/docs

# Mainnet API
curl https://api.coinset.org

# Testnet API
curl https://testnet11.api.coinset.org
```

## Docs Structure (Observed)

- `Introduction`: `https://www.coinset.org/docs`
- Endpoint categories observed:
  - `usage/blocks`
  - `usage/coins`
  - `usage/fees`
  - `usage/full--node`
  - `usage/mempool`
  - `usage/web-socket`

## API Conventions

- Typical response envelope:
  - `success` (boolean)
  - `error` (string)
  - plus endpoint-specific payload keys
- Hash-like fields are usually documented as hex strings (for example `header_hash`, `coin_id`, `tx_id`).
- In live responses, some numeric fields may appear as:
  - integers
  - decimal strings
  - hex strings (`0x...`)
  - occasionally bare `0x` for zero-like values
- Some fields that are semantically numeric/opcode-like may appear as either strings or integers.
- Common optional filters on coin queries:
  - `start_height` (`uint32`)
  - `end_height` (`uint32`)
  - `include_spent_coins` (boolean)

### Coin-record pagination (Coinset extension)

The public docs at `coinset.org` do **not** document cursor pagination, but the live Coinset API (and `chia-sdk-coinset`) support it on coin-record query endpoints:

| Direction | Field         | Type               | Meaning                                                                                                   |
| --------- | ------------- | ------------------ | --------------------------------------------------------------------------------------------------------- |
| Request   | `cursor`      | string (optional)  | Opaque resume token from a prior response. Omit on the first page.                                        |
| Response  | `truncated`   | boolean (optional) | `true` when the server hit its per-stream key limit (`scan_max_keys_per_stream`) and more records remain. |
| Response  | `next_cursor` | string (optional)  | Pass back as request `cursor` to fetch the next page. Present when `truncated` is `true`.                 |

Applies to:

- `get_coin_records_by_puzzle_hash`
- `get_coin_records_by_puzzle_hashes`
- `get_coin_records_by_hint` / `get_coin_records_by_hints`
- `get_coin_records_by_names`
- `get_coin_records_by_parent_ids`

**Pagination loop:**

1. POST with the normal filters (`puzzle_hash`, `include_spent_coins`, optional `start_height` / `end_height`).
2. Append `coin_records` from the response.
3. If `truncated` is `true`, repeat with `"cursor": "<next_cursor>"` (same endpoint and filters).
4. Stop when `truncated` is absent or `false`.
5. If `truncated` is `true` but `next_cursor` is missing, treat the scan as incomplete.

**Height windows** (`start_height` / `end_height`) remain a separate, documented filter strategy for narrowing scans by confirmation height. They complement cursor pagination but do not replace it on very large single-puzzle-hash wallets.

GreenFloor operator inventory (`coinset/cats/list.rs`, `coinset/xch.rs`), vault singleton fetch (`coinset/vault_fetch.rs`), script scans (`coinset/scan_client.rs`), and the `post_coinset_coin_records` CLI adapter follow cursor pages automatically.

## Endpoint Catalog (Verified)

### Blocks

- `POST /get_additions_and_removals` - block additions/removals by `header_hash`
- `POST /get_block` - full block by `header_hash`
- `POST /get_block_count_metrics` - aggregate block metrics (`compact_blocks`, `uncompact_blocks`, `hint_count`)
- `POST /get_block_record` - block record by `header_hash`
- `POST /get_block_record_by_height` - block record by `height`
- `POST /get_block_records` - block records in `[start, end]`
- `POST /get_block_spends` - spends in block by `header_hash`
- `POST /get_block_spends_with_conditions` - spends + conditions by `header_hash`
- `POST /get_blocks` - blocks in `[start, end]` with optional `exclude_header_hash`, `exclude_reorged`
- `POST /get_unfinished_block_headers` - unfinished block headers (empty body)

#### Fee Analysis Notes (Block + Spend Level)

- `get_blocks` exposes block-level fee totals via `transactions_info.fees`.
- For spend-level inspection, use `get_block_spends_with_conditions`.
- A practical spend-fee estimator is:
  - `coin_spend.coin.amount - sum(CREATE_COIN output amounts)`
- In practice, `get_blocks` payloads can occasionally omit `header_hash`; if needed, resolve via:
  - `POST /get_block_record_by_height`

### Coins

- `POST /get_coin_record_by_name` - one coin record by coin `name`
- `POST /get_coin_records_by_hint` - coin records by single `hint`
- `POST /get_coin_records_by_hints` - coin records by multiple `hints`
- `POST /get_coin_records_by_names` - coin records by multiple coin `names`
- `POST /get_coin_records_by_parent_ids` - coin records by `parent_ids`
- `POST /get_coin_records_by_puzzle_hash` - coin records by one `puzzle_hash`
- `POST /get_coin_records_by_puzzle_hashes` - coin records by `puzzle_hashes`
- `POST /get_memos_by_coin_name` - memos for coin `name`
- `POST /get_puzzle_and_solution` - coin solution by `coin_id` and optional `height`
- `POST /get_puzzle_and_solution_with_conditions` - coin solution + conditions
- `POST /push_tx` - submit a `spend_bundle` to mempool/blockchain

### Fees

- `POST /get_fee_estimate` - fee estimation for `spend_bundle` (supports `target_times`, `spend_count`)

### Full Node

- `POST /get_aggsig_additional_data` - network AGG_SIG additional data
- `POST /get_network_info` - network metadata (`network_name`, prefix, genesis challenge)
- `POST /get_blockchain_state` - full node/blockchain summary state
- `POST /get_network_space` - estimated network space between two block header hashes

### Mempool

- `POST /get_all_mempool_items` - all mempool items
- `POST /get_all_mempool_tx_ids` - all mempool tx ids (spend bundle hashes)
- `POST /get_mempool_item_by_tx_id` - one mempool item by `tx_id`
- `POST /get_mempool_items_by_coin_name` - mempool items by `coin_name`

### WebSocket

- `GET /ws` - realtime stream for new transactions, mempool items, and offer files
- GreenFloor daemon connects with query filters (always applied; configured URL query is replaced):
  - `events=transaction,offer`
  - `tx_status=pending,confirmed`
  - repeatable `p2=<puzzle_hash>` for each enabled market receive puzzle and CAT outer hash
    (CAT id from hex `base_asset` or `cats.yaml` ticker index)
- Documented `transaction` frames carry tx `ids` + `p2s`. Optional coin-name fields
  (`coin_ids` / `removals` / …) are parsed when present but are not required by Coinset docs.
  Offer frames carry `offer_id` + `status` (+ optional `tx_id` / `p2s`).
- Non-envelope / legacy flat payloads are ignored. Mainnet operators should confirm live frames match this envelope.
- Frame routing (GreenFloor):
  - **Transaction** frames: record tx signals; inventory-index `p2` hits **and** durable
    maker watch hits mark markets stale (90s freshness gate). Pending-frame durable
    maker **coin** watch hits drive `mempool_observed`; confirmed-frame maker coin
    watch hits promote to `tx_block_confirmed` via the frame's confirmed tx ids (so
    ladder slots do not age free before an offer-frame `confirmed` arrives).
    P2-only durable watch hits mark inventory stale only (shared maker puzzle hashes
    must not fan out takes). Shared market inventory p2s are not stored on per-offer
    watches. HTTP enrichment uses `get_coin_records_by_puzzle_hashes` when needed.
  - **Offer** frames: drive offer lifecycle by `offer_id` / status for `confirmed`,
    `cancelled`, and `expired` only (those also mark the offer's market inventory stale).
    Offer-frame `pending` / `cancel_pending` seed `tx_signal_state` when `tx_id` is present
    but do **not** advance to `mempool_observed` (avoids aging live listings out of
    active-slot counts). Offer-frame `p2s` must **not** mark inventory stale or apply
    watch-hit lifecycle.
- HTTP webhooks are out of scope; cancel and other spends use `POST /push_tx`, not the WebSocket.

### Offers

- `POST /push_offer` - body `{ "offer": "offer1..." }`; success returns 64-hex `offer_id` (spend-bundle hash / Dexie `trade_id`)
- GreenFloor default publish venue is `coinset` via this endpoint; Dexie/Splash remain explicit opt-ins
- After a successful push, GreenFloor persists `OfferCancelFields` (maker input coin id +
  mode-specific hints) so on-chain cancel can reclaim without an offer blob. Direct
  offers use exactly one input coin equal to the offer amount.

## Integration Cheat Sheet (Required Body Fields)

Use this as a quick "minimum payload" guide when wiring clients.

### Blocks

| Endpoint                                 | Required body fields |
| ---------------------------------------- | -------------------- |
| `POST /get_additions_and_removals`       | `header_hash`        |
| `POST /get_block`                        | `header_hash`        |
| `POST /get_block_count_metrics`          | none (`{}`)          |
| `POST /get_block_record`                 | `header_hash`        |
| `POST /get_block_record_by_height`       | `height`             |
| `POST /get_block_records`                | `start`, `end`       |
| `POST /get_block_spends`                 | `header_hash`        |
| `POST /get_block_spends_with_conditions` | `header_hash`        |
| `POST /get_blocks`                       | `start`, `end`       |
| `POST /get_unfinished_block_headers`     | none (`{}`)          |

### Coins

| Endpoint                                        | Required body fields                                                                     |
| ----------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `POST /get_coin_record_by_name`                 | `name`                                                                                   |
| `POST /get_coin_records_by_hint`                | `hint`                                                                                   |
| `POST /get_coin_records_by_hints`               | `hints`                                                                                  |
| `POST /get_coin_records_by_names`               | `names`                                                                                  |
| `POST /get_coin_records_by_parent_ids`          | `parent_ids`                                                                             |
| `POST /get_coin_records_by_puzzle_hash`         | `puzzle_hash` (+ optional `cursor`, `start_height`, `end_height`, `include_spent_coins`) |
| `POST /get_coin_records_by_puzzle_hashes`       | `puzzle_hashes` (+ optional `cursor`, height filters, `include_spent_coins`)             |
| `POST /get_memos_by_coin_name`                  | `name`                                                                                   |
| `POST /get_puzzle_and_solution`                 | `coin_id`                                                                                |
| `POST /get_puzzle_and_solution_with_conditions` | `coin_id`                                                                                |
| `POST /push_tx`                                 | `spend_bundle`                                                                           |
| `POST /push_offer`                              | `offer` (`offer1...`)                                                                    |

### Fees / Full Node / Mempool / WebSocket

| Endpoint                               | Required body fields                                 |
| -------------------------------------- | ---------------------------------------------------- |
| `POST /get_fee_estimate`               | `spend_bundle`                                       |
| `POST /get_aggsig_additional_data`     | none (`{}`)                                          |
| `POST /get_network_info`               | none (`{}`)                                          |
| `POST /get_blockchain_state`           | none (`{}`)                                          |
| `POST /get_network_space`              | `newer_block_header_hash`, `older_block_header_hash` |
| `POST /get_all_mempool_items`          | none (`{}`)                                          |
| `POST /get_all_mempool_tx_ids`         | none (`{}`)                                          |
| `POST /get_mempool_item_by_tx_id`      | `tx_id`                                              |
| `POST /get_mempool_items_by_coin_name` | `coin_name`                                          |
| `GET /ws`                              | none (upgrade to WebSocket)                          |

## Common Pitfalls

- Prefer explicit `Content-Type: application/json` on all `POST` calls.
- Send `{}` for endpoints with empty request bodies instead of omitting the body.
- Treat hash-like fields as hex strings exactly as documented (for example `0x...` values).
- `push_tx` appears in multiple docs paths, but the route is the same API call (`POST /push_tx`).
- Coin query defaults may return only unspent records unless `include_spent_coins` is set.
- Keep API host and docs host separate:
  - docs: `https://www.coinset.org/docs`
  - API: `https://api.coinset.org`

## Known Runtime Quirks

- Some runtime environments can get blocked by upstream protections when using
  generic/default HTTP clients. Using a stable, explicit `User-Agent` header
  improves reliability for scripted calls.
- Expect mixed field typing in some responses (int/decimal-string/hex-string).
  Parse defensively in automation.
- For long scans over many blocks, build fail-soft behavior so single malformed
  or transiently failing blocks do not abort the entire run.

## Quick Request Patterns

```bash
curl -X POST "https://api.coinset.org/<endpoint>" \
  -H "Content-Type: application/json" \
  -d '<json body>'
```

```bash
wscat -c wss://coinset.org/ws
```

## Coinset MCP Server (`https://mcp.coinset.org/`)

Read-only MCP server backed by Coinset. **No wallet, no keys, no signing, no `push_tx` / `push_offer`.**
GreenFloor operator submit paths (`greenfloor-engine coinset push-tx`, offer cancel broadcast) use the direct HTTP API, not MCP.

### Connection

- Transport: MCP over HTTP with Server-Sent Events (SSE).
- Clients must send `Accept: application/json, text/event-stream` and `Content-Type: application/json`.
- Session: server returns `Mcp-Session-Id` on `initialize`; include it on subsequent requests.
- Protocol version: `2024-11-05`.

### Operating rules

- Always check the `success` field on every result; surface `error.kind` / `error.message` on failure.
- Addresses (`xch1…` / `txch1…`) and `0x`-hex puzzle hashes are interchangeable inputs everywhere.
- Amounts are in mojos: 1 XCH = 1,000,000,000,000 mojos; 1 CAT unit = 1,000 mojos.
- Prefer confirmed block data over mempool data for definitive answers; mempool items can disappear.
- When tracing coin history, pass `include_spent=true` so spent coins are returned.
- Explain spends from conditions/additions/removals outward; then puzzle layers; then raw CLVM.
- For wallet questions: `get_address_balance` + `list_address_assets` + `list_transactions` (`include_pending` merges mempool).
- For "what does this tx/offer do": `get_transaction` / `decode_offer`, then `inspect` for deep analysis.

### Tool catalog (24 tools)

| Tool                    | Description                                                                                                                                             |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `search`                | Universal search: route any string (address, tx id, coin id, block, offer id, NFT id) to its match.                                                     |
| `find_coins`            | Find coins by `name`, `names`, `puzzle_hash`, `parent_ids`, or `hint`. Supports `include_spent`, height filters, `cursor` pagination, `limit`, `order`. |
| `get_address_balance`   | Combined balance for an address: XCH (confirmed/locked/pending), CAT balances, NFT counts.                                                              |
| `list_address_assets`   | List distinct CAT asset ids or NFT launcher ids held at an address.                                                                                     |
| `list_transactions`     | List transactions by `address`, `coin`, `block`, `cat_asset`, or `nft`. `include_pending` merges mempool txs (address only).                            |
| `get_transaction`       | Parsed tx summary (transfers, swaps, AMM, royalties, fee, memos); optional `include_state`, `include_graph`.                                            |
| `wait_for_confirmation` | Poll tx lifecycle until confirmed/rejected/removed or timeout (`poll_interval_secs`, `timeout_secs`).                                                   |
| `get_block`             | Block record by height or header hash; optional coin spends.                                                                                            |
| `get_block_changes`     | All coins created and destroyed in a block (paginated).                                                                                                 |
| `list_reorgs`           | Recent chain reorganization events (paginated).                                                                                                         |
| `get_chain_state`       | Peak height/hash, netspace, difficulty, sync status, mempool size.                                                                                      |
| `mempool`               | Actions: `list`, `get_by_tx`, `get_by_coin`, `is_pending`.                                                                                              |
| `get_asset_info`        | Asset metadata for `cat`, `nft`, or `singleton` by id.                                                                                                  |
| `get_asset_coins`       | Coins for a CAT asset id (optional owner address) or NFT id.                                                                                            |
| `get_clawback_coins`    | Pending time-locked (clawback) coins for a receiver address.                                                                                            |
| `decode_offer`          | Decode an `offer1…` string locally into offered vs requested assets.                                                                                    |
| `get_offer`             | Offer state and lifecycle for an offer id.                                                                                                              |
| `list_offers`           | List offers by `address`, `cat_asset`, or `nft`; optional `status` filter (`open`, `pending`, `confirmed`, `cancel_pending`, `cancelled`, `expired`).   |
| `inspect`               | Decode conditions, additions/removals, puzzle layers. Source: `coin`, `block`, `mempool_tx`, or `offer`.                                                |
| `estimate_fee`          | Recommended fee (mojos) for a transaction cost and target inclusion times.                                                                              |
| `get_price`             | Current XCH price in USD; optionally convert a mojo amount to USD.                                                                                      |
| `convert_address`       | Convert between Chia address and puzzle hash (local, no network).                                                                                       |
| `compute_coin_id`       | Compute coin id = sha256(parent \|\| puzzle_hash \|\| amount) (local, no network).                                                                      |
| `clvm`                  | Local CLVM: `tree_hash`, `run`, `uncurry`, `curry`.                                                                                                     |

### MCP resources (reference data)

| URI                                     | Name              | Contents                                                                       |
| --------------------------------------- | ----------------- | ------------------------------------------------------------------------------ |
| `coinset://guide/operating-rules`       | operating-rules   | Read-only usage guide (mirrors section above).                                 |
| `coinset://reference/openapi`           | openapi           | Links to full_node.yaml and coinset.yaml OpenAPI specs.                        |
| `coinset://reference/conditions`        | conditions        | CLVM condition opcode reference (CREATE*COIN, AGG_SIG*\*, timelocks, etc.).    |
| `coinset://reference/puzzle-mod-hashes` | puzzle-mod-hashes | Well-known puzzle module hashes (p2, CAT v2, singleton, settlement, NFT, DID). |

### MCP prompts (guided workflows)

| Prompt                | Description                                                         |
| --------------------- | ------------------------------------------------------------------- |
| `network_status`      | Summarize current Chia network state and XCH price.                 |
| `address_summary`     | Summarize an address: balances, assets, recent activity, USD value. |
| `transaction_summary` | Explain a transaction's transfers, swaps, and fee.                  |
| `offer_summary`       | Decode an offer and assess what is given vs received.               |
| `block_summary`       | Summarize a block: counts and notable coins.                        |

### MCP ↔ HTTP API mapping (selected)

| MCP tool                         | Underlying Coinset HTTP endpoints (conceptual)          |
| -------------------------------- | ------------------------------------------------------- |
| `find_coins` (`by=puzzle_hash`)  | `get_coin_records_by_puzzle_hash`                       |
| `find_coins` (`by=hint`)         | `get_coin_records_by_hint`                              |
| `find_coins` (`by=parent_ids`)   | `get_coin_records_by_parent_ids`                        |
| `find_coins` (`by=name`)         | `get_coin_record_by_name`                               |
| `mempool` (`action=list`)        | `get_all_mempool_tx_ids` / `get_all_mempool_items`      |
| `mempool` (`action=get_by_tx`)   | `get_mempool_item_by_tx_id`                             |
| `mempool` (`action=get_by_coin`) | `get_mempool_items_by_coin_name`                        |
| `get_block`                      | `get_block_record` / `get_block_spends_with_conditions` |
| `get_chain_state`                | `get_blockchain_state`                                  |
| `estimate_fee`                   | `get_fee_estimate`                                      |

MCP tools add parsed summaries, lifecycle state, pagination helpers, and local CLVM/offer decode that raw HTTP responses do not provide.

### Well-known puzzle module hashes

Match against `clvm` `uncurry` `mod_hash`:

| Puzzle                               | Module hash                                                          |
| ------------------------------------ | -------------------------------------------------------------------- |
| standard tx (p2_delegated_or_hidden) | `0xe9aaa49f45bad5c889b86ee3341550c155cfdd10c3a6757de618d20612fffd52` |
| CAT v2                               | `0x37bef360ee858133b69d595a906dc45d01af50379dad515eb9518abb7c1d2a7a` |
| singleton top layer v1.1             | `0x7faa3253bfddd1e0decb0906b2dc6247bbc4cf608f58345d173adb63e8b47c9f` |
| singleton launcher                   | `0xeff07522495060c066f66f32acc2a77e3a3e737aca8baea4d1a64ea4cdc13da9` |
| settlement payments (offers)         | `0xcfbfdeed5c4ca2de3d0bf520b9cb4bb7743a359bd2e6a188d19ce7dffc21d3e7` |
| NFT state layer                      | `0xa04d9f57764f54a43e4030befb4d80026e870519aaa66334aef8304f5d0393c2` |
| NFT ownership layer                  | `0xc5abea79afaa001b5427dfa0c8cf42ca6f38f5841b78f9b3c252733eb2de2726` |
| DID inner puzzle                     | `0x33143d2bef64f14036742673afd158126b94284b4530a28c354fac202b0c910e` |

CAT `asset_id` is its TAIL program hash; singleton/NFT/DID `launcher_id` identifies the singleton.

## GreenFloor operator usage

- Offer lifecycle reads (coin lookup, vault singleton, mempool tx ids): `greenfloor-engine`
  `coinset` subcommands and `greenfloor-engine/src/coinset/`.
- **Offer cancel submit:** `offers-cancel` broadcasts reclaim spend bundles via the direct Coinset HTTP API
  (`push_tx` / broadcast helpers). Dexie is not used for cancel submission (ADR 0015).
- Cancel tx ids are observed for reconcile; operator DB state is `cancel_submitted` until
  confirmation promotes to `cancelled`.

## Notes

- Some docs links point to additional pages not yet verified in this file (for example, several "Previous/Next" links outside this fetched set).
- The docs mention a CLI utility on the intro page, but a dedicated setup guide was not discovered in the fetched pages.

## Source Pages

- https://mcp.coinset.org/ (MCP server; SSE transport, not a browser page)
- https://www.coinset.org/openapi/full_node.yaml
- https://www.coinset.org/openapi/coinset.yaml
- https://www.coinset.org/docs
- https://coinset.org/
- https://www.coinset.org/docs/usage/blocks/get_additions_and_removals
- https://www.coinset.org/docs/usage/blocks/get_block
- https://www.coinset.org/docs/usage/blocks/get_block_count_metrics
- https://www.coinset.org/docs/usage/blocks/get_block_record
- https://www.coinset.org/docs/usage/blocks/get_block_record_by_height
- https://www.coinset.org/docs/usage/blocks/get_block_records
- https://www.coinset.org/docs/usage/blocks/get_block_spends
- https://www.coinset.org/docs/usage/blocks/get_block_spends_with_conditions
- https://www.coinset.org/docs/usage/blocks/get_blocks
- https://www.coinset.org/docs/usage/blocks/get_unfinished_block_headers
- https://www.coinset.org/docs/usage/coins/get_coin_record_by_name
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_hint
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_hints
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_names
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_parent_ids
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_puzzle_hash
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_puzzle_hashes
- https://www.coinset.org/docs/usage/coins/get_memos_by_coin_name
- https://www.coinset.org/docs/usage/coins/get_puzzle_and_solution
- https://www.coinset.org/docs/usage/coins/get_puzzle_and_solution_with_conditions
- https://www.coinset.org/docs/usage/coins/push_tx
- https://www.coinset.org/docs/usage/fees/get_fee_estimate
- https://www.coinset.org/docs/usage/full--node/full_node_get_aggsig_additional_data
- https://www.coinset.org/docs/usage/full--node/full_node_get_network_info
- https://www.coinset.org/docs/usage/full--node/get_blockchain_state
- https://www.coinset.org/docs/usage/full--node/get_network_space
- https://www.coinset.org/docs/usage/full--node/push_tx
- https://www.coinset.org/docs/usage/mempool/get_all_mempool_items
- https://www.coinset.org/docs/usage/mempool/get_all_mempool_tx_ids
- https://www.coinset.org/docs/usage/mempool/get_mempool_item_by_tx_id
- https://www.coinset.org/docs/usage/mempool/get_mempool_items_by_coin_name
- https://www.coinset.org/docs/usage/web-socket/websocket_connect
