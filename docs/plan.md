# GreenFloor V1 Plan

## Scope

- Run a long-lived daemon (`greenfloord`) plus manager CLI (`greenfloor-manager`) for deterministic CAT/XCH market-making operations.
- Keep policy logic deterministic in `greenfloor/core`; keep side effects in adapters and CLI/daemon orchestration layers.
- Ship only low-inventory alert notifications for v1.

## Rollout Steps

1. Install project and validate baseline config in-repo (`config/program.yaml`, `config/markets.yaml`).
2. Bootstrap runtime home dir (`~/.greenfloor`) with `greenfloor-manager bootstrap-home` (required before first real deployment run).
3. Onboard signing key selection (`greenfloor-manager keys-onboard`) and verify key routing/registry mappings.
4. Start daemon (`greenfloord`) and monitor audit/state DB events for market, offer-lifecycle, and coin-op behavior.

## Manager CLI Commands (V1 Core)

Seven commands in scope. Do not add commands without explicit need tied to testnet proof.

1. `bootstrap-home` — create `~/.greenfloor` runtime layout, seed config, initialize state DB.
2. `config-validate` — validate program + markets config and key routing.
3. `doctor` — readiness check (config, key routing, DB, env overrides).
4. `keys-onboard` — interactive key selection and onboarding persistence.
5. `build-and-post-offer` — build offer via `chia-wallet-sdk` and post to venue (Dexie or Splash).
6. `offers-status` — compact view of current offer states and recent events.
7. `offers-reconcile` — refresh offer states from venue API and flag orphaned/unknown.

Operator output/coin-op behavior updates on current delivery path:

- Manager JSON responses are pretty-formatted by default; use global `--json` for compact single-line output.
- `coin-combine` now supports repeatable `--coin-id` for deterministic exact-coin targeting (with `coins-list` hex-name to `Coin_*` resolution).

## Signing Architecture

- All signing logic lives in `greenfloor/signing.py` — a single module handling coin discovery, coin selection, additions planning, spend-bundle construction, AGG_SIG signing, and broadcast.
- Coin discovery, chain-history reads (CAT parent lineage), and `push_tx` broadcast use `greenfloor/adapters/coinset.py` (`CoinsetAdapter`) as the side-effect boundary.
- `CoinsetAdapter` defaults to mainnet endpoints and routes to testnet11 endpoints when `network=testnet11`; optional override: `GREENFLOOR_COINSET_BASE_URL`.
- `WalletAdapter` (daemon coin-op path) calls `signing.sign_and_broadcast()` directly.
- `offer_builder_sdk` (manager offer-build path) calls `signing.build_signed_spend_bundle()` directly.
- One env-var escape hatch each: `GREENFLOOR_WALLET_EXECUTOR_CMD` (WalletAdapter), `GREENFLOOR_OFFER_BUILDER_CMD` (manager).
- No intermediate subprocess layers. See `AGENTS.md` for the design discipline rules.

## Offer File Contract

- Offer files are text files containing a Bech32m offer string (prefix `offer1...`), not JSON.
- Per `chia-wallet-sdk`, offer text is an encoded/compressed `SpendBundle` (`encode_offer` / `decode_offer`).
- Before Dexie submission, GreenFloor validates offer text through `chia-wallet-sdk` parse semantics (`Offer::from_spend_bundle`) and blocks submission on verification failure.
- Before Dexie submission, GreenFloor rejects offers that do not include at least one `ASSERT_BEFORE_*` expiration condition (time or block-height).
- Adapter/test paths should treat offer files as opaque serialized artifacts: read file text, submit text to venue API, and persist IDs/status separately.

## Offer Lifecycle Strategy

- All market-making offers must always include an expiry.
- Stable-vs-unstable markets use shorter offer expiries than other pair types, so stale pricing is naturally rotated faster.
- Offer cancellation is intentionally rare and should not be a routine refresh mechanism.
- Cancellation applies only to stable-vs-unstable pairs, and only when there is strong price movement on the unstable side.
- In normal conditions, expiry-based replacement is preferred over explicit cancellation.

## Delivery Constraints

- Python 3.11+.
- Deterministic test suite (`pytest`) should stay under 10 minutes wall clock (prefer under 5).
- Required checks: `ruff check`, `ruff format --check`, `pyright`, `pytest`.
- Local convenience gate: `pre-commit run --all-files` (configured to run `ruff`, `ruff-format`, `prettier`, `yamllint`, `pyright`, and `pytest`).

## Plan TODOs (Current State)

- [x] Baseline clarified: `chia-wallet-sdk` is the default stack for sync/sign/offer generation.
- [x] Signing pipeline consolidated into single `greenfloor/signing.py` module with direct function calls. Legacy 13-file subprocess chain removed.
- [x] Dexie adapter offer write paths implemented (`post_offer`, `cancel_offer`) with deterministic fixture tests using real `offer1...` payloads.
- [x] Strategy port completed: legacy carbon XCH sizing logic moved into pure `greenfloor/core/strategy.py` with deterministic tests.
- [x] Coincodex price service implemented with TTL cache and stale fallback, and daemon now records XCH price snapshots each cycle.
- [x] XCH strategy is price-gated: no XCH offer planning when price snapshot is unavailable/invalid.
- [x] Home-dir bootstrap implemented via `greenfloor-manager bootstrap-home`.
- [x] P1: Wire `strategy_actions_planned` outputs into daemon offer execution path.
- [x] P1-followup: In-process `chia-wallet-sdk` offer construction via `greenfloor/cli/offer_builder_sdk.py`.
- [x] P2: Policy-gated cancel execution for unstable-leg markets on strong price moves.
- [x] P3: Runbook-level operator docs (`docs/runbook.md`).
- [x] Manager CLI simplified from 21 commands to 7 core commands. Non-essential commands (metrics, config history, ladder/bucket tuning, etc.) deferred until after testnet proof.
- [x] Offer builder subprocess boundary eliminated — manager calls `offer_builder_sdk.build_offer()` directly.

## Remaining Gaps Before First Production-Like User Test

These are the only priorities. Do not start new feature work until G1-G3 are complete.

- [x] G1: Replace deterministic/synthetic manager offer build output with coin-backed `chia-wallet-sdk` offer construction that passes venue validation on `testnet11`.
  - Status update (2026-02-22): in-process offer-plan signing path is implemented in `greenfloor/signing.py` (including CAT lineage reconstruction and mixed-asset action building), and manager offer-builder now emits offer-plan payloads.
  - Current blocker: production-like proof depends on funded inventory for the exact signer/address context used by the execution environment. Local runs may fail if mnemonic/key material is CI-only.
  - Mitigation in place: `.github/workflows/live-testnet-e2e.yml` now supports a CI-only manager proof path (`pair`, `size_base_units`, `dry_run`) using `TESTNET_WALLET_MNEMONIC` and uploads proof logs as artifacts.
  - Latest status (2026-02-23): branch `feat/greenfloor-native-upstream-migration` completed repeated live workflow proofs on the current head (`run_id=22325031449`, `run_id=22325053517`), including successful `dry_run=false` Dexie posts with offer IDs and uploaded artifacts.
  - Verification hardening in place: manager now validates offers via wallet-sdk before Dexie post to catch malformed offers on the primary path.
  - Closure evidence: repeated `dry_run=false` runs now show venue-valid post outcomes on `testnet11` with offer IDs in workflow logs/artifacts.
- [x] G2: Add operator helper workflow for `testnet11` asset discovery + inventory bootstrap (Dexie testnet liquidity discovery + market snippet generation).
  - Implemented minimal manual workflow: `.github/workflows/testnet11-asset-bootstrap-helper.yml`.
  - Workflow contract: discover Dexie testnet tokens, normalize/rank candidates, and upload bootstrap artifacts (`raw-tokens.json`, `normalized-tokens.json`, `selected-assets.json`, `markets-snippet.yaml`, `summary.md`).
- [x] G3: Run and document an end-to-end `testnet11` proof (build -> post -> status -> reconcile) using a live test asset pair.
  - CI path now executes this sequence when `dry_run=false` in `live-testnet-e2e`.
  - Latest status (2026-02-23): current native-migration branch runs (`run_id=22325031449`, `run_id=22325053517`) executed the full manager proof sequence successfully with artifacts, including live Dexie post (`dry_run=false`) and reconcile output.
  - Historical `testnet11` proof pair was `TDBX:txch` (TXCH<->TDBX); BYC04 was not the primary proof target.

## Active Live Testing Target

- Active live testing target is now mainnet `CARBON22:xch`.
- `CARBON22` CAT ID: `4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7`.
- Prior `testnet11` proof artifacts remain valid as historical closure evidence for G1-G3.

## Operational Hardening Follow-up

- [ ] H1: Add explicit Coinset fee-lookup diagnostics and endpoint validation for coin operations.
  - Scope: `coin-split` / `coin-combine` fee lookups (including conservative fee-advice path), not only generic Coinset connectivity.
  - Add a deterministic preflight check that validates fee lookup behavior against the configured/default Coinset endpoint before live coin-op submission.
  - Ensure failure output clearly distinguishes endpoint routing/configuration issues from temporary fee-advice unavailability.
  - Document operator debug steps for fee lookups (endpoint verification, override behavior, expected JSON failure contracts).

## Deferred Backlog (Post-Testnet Proof)

These items were implemented previously but removed during simplification. Re-add only after G1-G3 are proven.

- [ ] Config editing commands: `set-price-policy`, `set-ladder-entry`, `set-bucket-count`, `set-low-watermark`.
- [ ] Config history: `config-history-list`, `config-history-revert`.
- [ ] Operational commands: `keys-list`, `reload-config`, `consolidate`, `register-coinset-webhook`, `list-supported-assets`.
- [ ] Observability: `metrics-export`, `coin-op-budget-report`.

## Upstreaming to GitHub (Repository Setup)

- [x] U1: Create GitHub repository and set `origin` remote.
- [x] U2: Push current branch to `origin` and verify branch tracking.
- [x] U3: Enable branch protection on `main` (require PR, disallow force-push).
- [x] U4: Configure required PR checks to match project gates.
- [x] U5: Verify Actions permissions and secret hygiene.
- [x] U6: Open first PR and verify all required checks pass before merge.
