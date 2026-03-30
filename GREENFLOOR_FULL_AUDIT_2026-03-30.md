# GreenFloor Full Audit (2026-03-30)

## Executive Summary

| Severity    | Count |
| ----------- | ----- |
| 🔴 CRITICAL | 0     |
| 🟠 HIGH     | 2     |
| 🟡 MEDIUM   | 2     |
| 🟢 LOW      | 2     |

**Overall Risk:** HIGH  
**Recommendation:** CONDITIONAL (address HIGH items before production rollout)

**Key Metrics:**

- Python files in repo: 102
- Focused deep review files: 11 high-risk modules + architecture docs
- Runtime validation: `pytest -q` passed (`648 passed, 3 skipped`)
- Dirty working tree observed during audit: `greenfloor/cloud_wallet_offer_runtime.py`, `tests/test_cloud_wallet_offer_runtime.py`, `scripts/repro_eco1812020_cloud_wallet_query.py`, and `chia-wallet-sdk` submodule pointer

## Scope and Methodology

- Strategy: **FOCUSED** (medium-size Python codebase, risk-first pass)
- Automated sweep: subprocess usage, external calls/timeouts, broad exception handling, infinite loops
- Manual deep review:
  - `greenfloor/cloud_wallet_offer_runtime.py`
  - `greenfloor/daemon/main.py`
  - `greenfloor/cli/manager.py`
  - `greenfloor/cli/offer_builder_sdk.py`
  - `greenfloor/adapters/wallet.py`
  - `greenfloor/adapters/cloud_wallet.py`
  - `greenfloor/moderate_retry.py`
  - `greenfloor/signing.py`
  - `greenfloor/adapters/dexie.py`
  - `greenfloor/adapters/coinset.py`
  - `greenfloor/storage/sqlite.py`
- Policy-compliance checks against `AGENTS.md` + `docs/decisions/*`

## Findings

### 🟠 HIGH: Full signed offer content is logged at INFO level

**File:** `greenfloor/cloud_wallet_offer_runtime.py`  
**Evidence:** `log_signed_offer_artifact()` logs entire `offer_text` via `signed_offer_file:%s`.

**Why this matters:** `offer1...` artifacts can be commercially sensitive and operationally actionable. Logging full offer bodies increases leakage surface through log aggregation, operator consoles, and incident snapshots.

**Blast radius:** medium (offer-generation path; shared runtime logging pipeline)

**Recommendation:**

- Stop logging full offer text.
- Log only a safe fingerprint (e.g., prefix + length + hash) and non-sensitive metadata.
- Add regression test asserting full offer string is never emitted.

---

### 🟠 HIGH: Daemon imports CLI module (explicit architecture boundary violation)

**File:** `greenfloor/daemon/main.py`  
**Evidence:** `_build_offer_for_action()` imports `build_offer_text` from `greenfloor.cli.offer_builder_sdk`.

**Why this matters:** `AGENTS.md` requires daemon not to import CLI. This increases coupling and creates a path for CLI-specific side effects/env behavior to leak into daemon runtime.

**Blast radius:** low-to-medium (single import point, but central daemon path)

**Recommendation:**

- Extract offer building entry to a shared non-CLI module (e.g. `greenfloor/offer_builder.py`).
- Have both daemon and CLI import that shared module.
- Keep CLI file as thin wrapper for command-line integration only.

---

### 🟡 MEDIUM: Subprocess escape hatches pass full payloads to external commands

**Files:**

- `greenfloor/cli/offer_builder_sdk.py` (`GREENFLOOR_OFFER_BUILDER_CMD`)
- `greenfloor/adapters/wallet.py` (`GREENFLOOR_WALLET_EXECUTOR_CMD`)
- payload source includes Cloud Wallet fields in `greenfloor/cli/manager.py`

**Evidence:** Both subprocess paths send `json.dumps(payload)` to child process stdin.

**Why this matters:** When these env overrides are enabled, child binaries/scripts receive sensitive operational context (including keyring path and Cloud Wallet config values). This is a deliberate operator hook, but the security model is not formally documented.

**Blast radius:** medium (applies to all operator workflows that enable env hooks)

**Recommendation:**

- Document threat model and expected use in a new decision note under `docs/decisions/`.
- Add explicit redaction guidance for wrapper scripts.
- Optionally gate these overrides behind an explicit "unsafe override" flag in production mode.

---

### 🟡 MEDIUM: Retry-loop module lacks dedicated deterministic tests

**File:** `greenfloor/moderate_retry.py`  
**Evidence:** Contains `while True` + sleep loops (`call_with_moderate_retry`, `poll_with_exponential_backoff_until`), but no direct tests found for these symbols.

**Why this matters:** `AGENTS.md` requires deterministic tests for polling/sleep loops. Regressions in retry math/timeouts can silently impact cloud-wallet stability and incident handling.

**Blast radius:** medium-high (used across manager and cloud-wallet runtime)

**Recommendation:**

- Add targeted tests for:
  - first-try success
  - bounded retries then failure
  - exponential backoff progression
  - timeout boundary behavior
  - rate-limit message parsing path

---

### 🟢 LOW: Cloud Wallet OpenSSL signing path writes private key to temporary file

**File:** `greenfloor/adapters/cloud_wallet.py`  
**Evidence:** `_sign_canonical_with_openssl()` writes PEM to `NamedTemporaryFile(..., delete=False)` before running `openssl`.

**Why this matters:** Short-lived secret-on-disk exposure risk on multi-tenant hosts.

**Recommendation:**

- Prefer in-memory signing path if available (KMS/native).
- If OpenSSL path remains, add explicit file-permission assertion and operational hardening note.

---

### 🟢 LOW: Interactive mnemonic entry uses plain `input()`

**File:** `greenfloor/cli/manager.py`  
**Evidence:** key onboarding prompt reads mnemonic using visible terminal input.

**Why this matters:** Shoulder-surfing / terminal recording risk during onboarding.

**Recommendation:**

- Use hidden prompt (`getpass.getpass`) for mnemonic entry.
- Warn user before accepting secrets in interactive mode.

## Test Coverage Assessment (Audit Session)

- Existing repo tests are strong overall (648 passing).
- Gaps identified for this audit:
  - No direct deterministic tests found for `greenfloor/moderate_retry.py` loop functions.
  - No regression test found to enforce redaction of full offer text in runtime logs.
  - No policy test enforcing daemon->CLI import boundary.

## Historical / Policy Context Notes

- `docs/decisions/0002-signing-pipeline-consolidation.md` emphasizes reducing subprocess boundaries.
- Current subprocess hooks are present and functional but not clearly documented as explicit exceptions in decision records.

## Recommended Action Plan

### Immediate (blocking)

- [ ] Remove/redact full offer text logging in `cloud_wallet_offer_runtime.py`.
- [ ] Eliminate daemon import from CLI by extracting shared offer-builder module.

### Before next production rollout

- [ ] Add deterministic tests for `moderate_retry.py`.
- [ ] Add/update decision note for subprocess override threat model and safe usage.

### Technical debt / hardening

- [ ] Improve OpenSSL temp-key handling guidance or migrate away from file-based key material.
- [ ] Hide mnemonic input in CLI onboarding.

## Confidence and Limitations

**Confidence:** HIGH for reviewed scope, MEDIUM for full-system behavior.  
**Limitations:**

- No live external API fault-injection during this audit.
- No deep audit of dependencies (`chia_wallet_sdk`, `greenfloor_native`, external API providers).
- Audit focused on Python service/reliability/security and architecture policies, not economic-strategy correctness.
