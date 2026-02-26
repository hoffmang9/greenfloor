# Chia Cloud Wallet Docs & API Reference

This file summarizes the Cloud Wallet API surface observed from `../ent-wallet` (primarily `apps/api/src/graphql` and related docs).

**As of:** 2026-02-25

## Overview

- Cloud Wallet exposes a GraphQL API (Pothos/Relay style), not REST endpoints.
- HTTP GraphQL endpoint: `/graphql` (served by the API service host).
- WebSocket subscriptions are also served on `/graphql` (GraphQL WS).
- Schema supports queries, mutations, and subscriptions with custom scalars and Relay global IDs.
- Local API docs are generated from schema SDL via Magidoc (`apps/api/magidoc.mjs`).

## Docs Structure (Observed)

- API doc generation guide: `../ent-wallet/docs/generating-api-docs.md`
- Magidoc config: `../ent-wallet/apps/api/magidoc.mjs`
- GraphQL schema assembly: `../ent-wallet/apps/api/src/graphql/schema.ts`
- GraphQL server wiring (HTTP + WS): `../ent-wallet/apps/api/src/server.ts`
- Rate limiting notes: `../ent-wallet/docs/api-rate-limiting.md`
- Admin-specific notes: `../ent-wallet/docs/administrative-queries-and-mutations.md`

## API Conventions

- Transport:
  - `POST /graphql` for queries/mutations
  - `ws://.../graphql` or `wss://.../graphql` for subscriptions
- Auth styles seen in schema:
  - public calls (`authentication: { public: true }`)
  - authenticated viewer calls
  - signer-app authenticated calls
- Relay/global ID usage:
  - Many inputs use `globalID` for entities (`walletId`, `userId`, `offerId`, etc.).
  - Connection fields commonly expose pagination + `totalCount`.
- Custom scalars:
  - `BigInt` (often mojo amounts/fees)
  - `DateTime`
  - `Bytea`
  - `JSON`
- Rate limiting:
  - Default query/mutation rate limits plus operation-level overrides via directives.
  - Per-user operation point multipliers are documented in DB/admin notes.

## Operation Catalog (Verified)

Cloud Wallet is broad; below is the practical call map by feature family.

### Authentication, Account, Identity

- `logIn`, `logOut`
- `viewer`, `user`, `users`
- Signup + email/OTP flows:
  - `userSignup`
  - `requestSignupEmailVerification`, `verifyUserOtp`
  - `requestUserCredentialsResetLink`, `resetUserCredentials`
  - `requestUserChangeEmailVerification`, `verifyEmailChangeOtp`
  - additional email flows (`requestUserAddAdditionalEmail`, `verifyAdditionalEmailOtp`, etc.)
- Passkeys:
  - `startPasskeyAssign`, `verifyPasskeyAssign`
  - `startPasskeyAuthentication`, `verifyPasskeyAuthentication`
  - `updatePasskey`, `deletePasskey`

### Wallets, Vaults, Signers

- Wallet lifecycle:
  - `createWallet`, `updateWallet`, `deleteWallet`, `resyncWallet`
  - `wallet`, `vaultConfigDownload`, `restoreVaultFromConfig`
- Wallet actions and signing:
  - `walletAction` (vault-specific actions, fee/auto-submit controls)
  - `signatureRequest`, `submitSignatureRequest`, `signSignatureRequest`, `resendSignatureRequest`
- Signers/keys:
  - `createSigner`, `createSigners`, `updateSigner`, `deleteSigner`
  - signer public-key attach/detach calls
  - signer-app key/auth-key flows (`addSignerAppKey`, `createSignerAppAuthKeyByPublicKey`, etc.)

### Transactions, Coins, Offers, NFTs

- Transactions:
  - `createTransaction`, `finalizeTransaction`, `clawBackTransaction`, `deleteTransaction`
  - `walletTransaction`
- Coins:
  - `coins`, `splitCoins`, `combineCoins`
- Offers:
  - `createOffer`, `addOffer`, `takeOffer`, `cancelOffer`
  - `walletOffer`
- NFTs:
  - `nft`, `nftOnChain`, `transferNfts`, `bulkMintNfts`

### Buy Program, Pricing, Stripe

- Market data:
  - `quote`, `price`
- Buy orders:
  - `createAndConfirmBuyOrder`, `cancelBuyOrder`
  - `retryBuyOrder`, `verifyMicrodepositBuyOrder`, `setBuyOrderAutoFinalizeOverride`, `resendClawedBackXch`
  - `buyOrder`, `buyOrders`, `adminBuyOrders`
- Stripe/customer management:
  - `customerSession`

### Admin/Platform Configuration

- Organizations/plans/feature flags:
  - `createOrganization`, `updateOrganization`, `deleteOrganization`
  - `createPlan`, `updatePlan`
  - `assignFeatureFlagToOrganization`, `assignFeatureFlagToPlan`
- Buy limits + tiers:
  - `createBuyLimit`, `updateBuyLimit`, `deleteBuyLimit`
  - `purchaseLimitTiers`, `savePurchaseLimitTiers`
  - `validatePurchaseAmount`
- Recovery providers/templates/reviews:
  - recovery provider CRUD and options queries
  - vault config template CRUD + signer notifications
  - vault recovery review approve/reject

### Subscriptions (WebSocket)

- `walletUpdated`, `walletAdded`
- `offerUpdated`
- `buyOrderUpdated`
- `signatureRequestCreated`, `signatureRequestUpdated`
- `signerAppKeyLinked`
- `identityVerificationStatusUpdated`
- `purchaseProgramStateUpdated`

## Integration Cheat Sheet (Core Calls + Input Options)

Use this as a quick guide for commonly integrated operations.

| Operation                  | Required input                                                                                                         | Common optional input/options                                                                                            |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `logIn`                    | `input.email`, `input.password`                                                                                        | none                                                                                                                     |
| `logOut`                   | `input`                                                                                                                | `input.invalidateAllSessions`                                                                                            |
| `userSignup`               | `input`                                                                                                                | OTP flow fields (`email`, `code`), profile fields (`name`, `organizationName`, `passkeyName`)                            |
| `createWallet`             | `input.custodyConfig`                                                                                                  | `name`, `userId`, `networkId`, `keys`, `watchtower`                                                                      |
| `walletAction`             | `input.walletId`                                                                                                       | `vaultAction`, `fee`, `autoSubmit`, `requestId`, `networkId`, `userId`                                                   |
| `restoreVaultFromConfig`   | `input.configJson`, `input.name`                                                                                       | none                                                                                                                     |
| `createTransaction`        | `walletId`, `address`, `amount`                                                                                        | `assetId`, `fee`, `memos`, `autoSubmit`, `clawback`                                                                      |
| `finalizeTransaction`      | `walletTransactionId`                                                                                                  | `fee`, `autoSubmit`                                                                                                      |
| `clawBackTransaction`      | `walletTransactionId`                                                                                                  | `fee`, `autoSubmit`                                                                                                      |
| `createOffer`              | `walletId`, `offered[]`, `requested[]`                                                                                 | `offeredNftIds`, `requestedNftIds`, `expiresAt`, `fee`, `autoSubmit`, `splitInputCoins`, `splitInputCoinsFee`            |
| `addOffer`                 | `walletId`, `offer`                                                                                                    | none (`offer` is bech32m `offer1...`)                                                                                    |
| `takeOffer`                | `walletId`, `offerId`                                                                                                  | `fee`, `autoSubmit`                                                                                                      |
| `cancelOffer`              | `walletId`, `offerId`                                                                                                  | `cancelOffChain`, `fee` (for on-chain cancel), `autoSubmit`                                                              |
| `splitCoins`               | `walletId`, `coinIds[]`, `numberOfCoins`, `amountPerCoin`                                                              | `fee`                                                                                                                    |
| `combineCoins`             | `walletId`                                                                                                             | `inputCoinIds[]`, `assetId`, `numberOfCoins`, `largestFirst`, `targetAmount`, `maxNumberOfCoins`, `maxCoinAmount`, `fee` |
| `quote`                    | `asset`                                                                                                                | none                                                                                                                     |
| `price`                    | `coin`, `currency`                                                                                                     | none                                                                                                                     |
| `createAndConfirmBuyOrder` | `fiatAmount`, `fiatCurrency`, `xchPricePerUnit`, `xchAmount`, `quoteId`, `address`, `confirmationTokenId`, `returnUrl` | none                                                                                                                     |
| `cancelBuyOrder`           | `buyOrderId`, `cancellationReason`                                                                                     | none                                                                                                                     |
| `vaultConfigDownload`      | `id` (wallet global ID)                                                                                                | none                                                                                                                     |

## `cancelOffer` Deep Dive

`cancelOffer` supports two operational modes with different side effects and return behavior.

### Input Shape

- Required:
  - `walletId` (global ID)
  - `offerId` (global ID)
- Optional:
  - `cancelOffChain` (boolean, default `false`)
  - `fee` (`BigInt`, used for on-chain cancellation)
  - `autoSubmit` (boolean, used for on-chain cancellation flow)

### Mode A: On-chain cancel (`cancelOffChain: false`)

- Builds an on-chain cancellation spend and returns `signatureRequest` (non-null on success).
- `fee` defaults to `0n` if omitted.
- `autoSubmit` defaults to `false` if omitted.
- Can fail if the wallet cannot cancel the offer or there is not enough XCH to pay the cancellation fee.

### Mode B: Off-chain cancel (`cancelOffChain: true`)

- Performs an off-chain state cancellation only (no spend bundle created).
- Returns `signatureRequest: null`.
- Requires org feature flag `OFFER_CANCEL_OFF_CHAIN`.
- Used for immediate cancellation semantics without creating an on-chain cancel transaction.

### State and Validation Rules

- Offer must be in `OPEN` or `PENDING` state.
- `PENDING` offers can only be canceled off-chain:
  - If `state === PENDING` and `cancelOffChain !== true`, the call errors.
- Permission is enforced against the provided `walletId`.

### Practical Usage Notes

- Use on-chain cancel when you want the cancellation represented on-chain and are prepared to pay a fee.
- Use off-chain cancel for quick internal state cancellation where the feature flag is enabled.
- GraphQL input does not expose `requestId` for `cancelOffer` even though helper internals support it.

### Copy/Paste GraphQL Examples

On-chain cancel (returns a `signatureRequest`):

```graphql
mutation CancelOfferOnChain($input: CancelOfferInput!) {
  cancelOffer(input: $input) {
    signatureRequest {
      id
    }
  }
}
```

```json
{
  "input": {
    "walletId": "V2FsbGV0OjEyMw==",
    "offerId": "T2ZmZXI6YWJjZGVm...",
    "cancelOffChain": false,
    "fee": "1000",
    "autoSubmit": true
  }
}
```

Off-chain cancel (returns `signatureRequest: null`):

```graphql
mutation CancelOfferOffChain($input: CancelOfferInput!) {
  cancelOffer(input: $input) {
    signatureRequest {
      id
    }
  }
}
```

```json
{
  "input": {
    "walletId": "V2FsbGV0OjEyMw==",
    "offerId": "T2ZmZXI6YWJjZGVm...",
    "cancelOffChain": true
  }
}
```

## Example Request Patterns

```bash
curl -X POST "https://<cloud-wallet-api-host>/graphql" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{"query":"query { viewer { id email } }"}'
```

```bash
curl -X POST "https://<cloud-wallet-api-host>/graphql" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{"query":"mutation($input: LogInInput!){ logIn(input:$input){ token user { id email } } }","variables":{"input":{"email":"user@example.com","password":"***"}}}'
```

## Common Pitfalls

- Treat IDs as Relay global IDs in GraphQL arguments, not raw DB IDs.
- `BigInt` fields (amounts/fees) should be handled as precise integers (mojo-style units).
- Many write calls return a `signatureRequest`; final on-chain action may require additional signing workflow.
- Offer ingestion (`addOffer`) expects a bech32m offer string (`offer1...`) and validates parsing.
- Subscriptions require authenticated context and use `/graphql` WebSocket with server-side filtering by viewer.
- Rate limits are applied globally and may be stricter on security-sensitive operations.

## How to Generate Full Local API Docs

From `../ent-wallet`:

1. Start API server.
2. Run `npm run generate:apiDoc`.
3. Generated static docs are written to `apps/api/magidoc/generatedDocs`.

## Source Files Reviewed

- `../ent-wallet/docs/generating-api-docs.md`
- `../ent-wallet/docs/api-rate-limiting.md`
- `../ent-wallet/docs/administrative-queries-and-mutations.md`
- `../ent-wallet/apps/api/magidoc.mjs`
- `../ent-wallet/apps/api/src/server.ts`
- `../ent-wallet/apps/api/src/graphql/builder.ts`
- `../ent-wallet/apps/api/src/graphql/schema.ts`
- `../ent-wallet/apps/api/src/graphql/**/{queries,mutations,subscriptions}/*.ts`
