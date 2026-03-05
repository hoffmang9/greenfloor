# Draft Upstream Issue: BYC asset-scoped coin query returns stray row and mismatched totals

## Proposed title

`coins(assetId=...)` can include a stray coin with incoherent asset lineage; per-row asset resolution and wallet asset totals also diverge

## Summary

While debugging a BYC vault on `John-Deere`, `greenfloor-manager coins-list --asset BYC` reported:

- live coin-row sum: `50200` mojos (`50.200 BYC`)
- wallet asset total: `50300` mojos (`50.300 BYC`)
- expected real vault value: `50000` mojos (`50.000 BYC`)

The `+200` mojo overcount in the live row set localizes to a single suspicious current coin:

- coin: `4344df4191e68429233d787130b7eff6e2655673840edfa6feecfdcfc920933d`
- amount: `310`
- puzzle hash: `7ff9f7e13048e191717a34ff04c31b951254aced5cd93e1caac1e8849f700144`

Its ancestry is not a coherent BYC-only conservation chain. The lineage also flips from BYC to XCH resolution several generations up, which strongly suggests an indexing / asset-association problem rather than real extra BYC in the vault.

Separately, `walletAsset.totalAmount` is `100` mojos higher than the summed live coin rows, which appears to be an independent `balanceRecords` drift.

## Why this matters

`greenfloor` queries `coins(assetId=...)` to list vault inventory for a single CAT. If the backend can return a stray row with inconsistent asset ancestry, inventory accounting is wrong even before any client-side display logic runs.

## Reproduction context

- Host: `John-Deere`
- Asset: `BYC`
- Expected vault inventory: `50000` mojos (`50.000 BYC`)
- Observed live branch under puzzle hash `7ff9...0144`: `20200` mojos

That suspicious branch breaks down as:

- `19480` mojo pending leaf: `1a1d1e8e9ea204e7f5c94a8f9665a934955bf5c4ff13bbbae77c2e69b022b539`
- `41` separate leaves of `10` mojos each
- `310` mojo settled leaf: `4344df4191e68429233d787130b7eff6e2655673840edfa6feecfdcfc920933d`

So the bad excess is concentrated in the `310`-mojo row.

## Suspicious lineage

Tracing parent links for the stray `310` coin yields this leading chain:

1. `4344df4191e68429233d787130b7eff6e2655673840edfa6feecfdcfc920933d` -> `310`
2. `6e15e693ac02c62c066da59e413a0b1070be41f616441c54bc9823b1892fc123` -> `270`
3. `d49e2f717da21e1ebed1612dfc542a7c26bcbc01e7338d6b4cacfd6189e4cb61` -> `140`
4. `9a3a5667c112b8af09d53386c7ec5b7a8519959cf25114bfdb44ed2123a8eb8a` -> `100`
5. `0fbeb6dda5ad099d149c9563917e2ed14d13ebda21ee68974539a1bc0051682f` -> `0`
6. `62fe778efe3b88b99ce4fa37772ab1732eba14fba402921190afee439e99a9e0` -> `900`
7. `fafe0e302e92bbd624974d4dd77059152f249d13537f305c45f57e6796548a15` -> `1000`

That is not a sane conservation pattern for a single-asset leaf chain.

Even more suspiciously:

- the first few nodes in this chain resolve as BYC,
- by the `62fe...` ancestor, the per-row resolver is already returning XCH (`Asset_huun64oh7dbt9f1f9ie8khuw`),
- so the current BYC-scoped result set appears to contain a row whose ancestry crosses into XCH-resolved history.

## Expected

- `coins(walletId: ..., assetId: BYC, ...)` should return only current BYC coin rows that are coherently part of the BYC asset lineage.
- Summing returned rows should match the actual current BYC inventory for the vault.
- If a per-row asset cannot be resolved, the API should not silently relabel it as XCH.
- `walletAsset.totalAmount` should match the same current inventory snapshot used by the coin query, or clearly document snapshot lag / sync semantics.

## Actual

- `coins(assetId=BYC)` returned rows summing to `50200`, not the expected `50000`.
- The excess localizes to a single `310`-mojo row with incoherent ancestry.
- Per-row asset resolution can flip to XCH inside the suspicious chain.
- `walletAsset.totalAmount` reports `50300`, which is `100` mojos above the live coin-row sum.

## Relevant code paths

### Coin filtering uses `outerPuzzleId`

`coins(assetId=...)` scopes rows using `puzzleHashes.outerPuzzleId`:

- `../ent-wallet/apps/api/src/dataSources/coinRecords.ts`
- `getCoinRecords(...)`
- current filter:

```ts
if (asset?.identifier) {
  whereConditions.push(
    eq(puzzleHashes.outerPuzzleId, Buffer.from(asset.identifier, "hex")),
  );
}
```

### Per-row asset resolution falls back to XCH

`getByCoinName()` returns base currency when no asset is found:

- `../ent-wallet/apps/api/src/dataSources/assets.ts`

```ts
if (!asset) {
  const xch = await findByIdentifier(ctx, ctx.network.genesisChallenge);
  return xch;
}
```

That fallback hides asset-association failures and can make mixed-asset output look superficially valid.

### First-party client does not ask for `node.asset`

The Cloud Wallet UI coin list query omits per-row `asset` entirely:

- `../ent-wallet/apps/app/src/components/Wallet/WalletCoins.graphql`

This suggests the intended stable contract is the scoped query itself, not `node.asset` on each row.

### Aggregate totals appear to come from a different snapshot path

The extra `+100` mojo in `walletAsset.totalAmount` looks like a separate `balanceRecords` / wallet sync drift rather than the same bug as the stray `310` row.

## Suggested fix surface

### Primary bug

Investigate why `coins(assetId=...)` can include the stray `310`-mojo row at all.

Things to inspect:

- whether `puzzleHashes.outerPuzzleId` can point to stale or ambiguous asset mappings for recycled / transformed coin histories,
- whether the join path can associate a current row with an ancestor-side asset classification that no longer reflects the current coin,
- whether puzzle-hash lineage transitions involving CAT/XCH wrappers can leak non-CAT rows into CAT-scoped results.

### Secondary bug

Stop silently defaulting unresolved per-row assets to XCH in `getByCoinName()`.

Better options:

- return `null` / unresolved,
- raise an explicit error for debugging,
- or expose an `unknown` asset state that does not masquerade as chain base currency.

### Separate follow-up

Investigate why `walletAsset.totalAmount` is `100` mojos above the live row sum for the same BYC vault snapshot.

Likely area:

- wallet sync / `balanceRecords` update timing or stale-state accumulation.

## Notes for `greenfloor`

As a client-side mitigation, `greenfloor` should avoid requesting per-row `asset` when the query is already scoped by `assetId`, matching the first-party Cloud Wallet UI pattern. That does not fix the upstream stray-row bug, but it avoids surfacing misleading XCH fallback metadata.
