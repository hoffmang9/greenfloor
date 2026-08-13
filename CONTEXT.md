# GreenFloor

Operator policy for posting, shaping, and reconciling Chia offers from a vault — without running a full node or in-process wallet sync.

## Language

**Unique maker pin session**:
The in-batch protocol that pins a distinct exact-size receive-address maker coin for each new Direct offer under `unique_maker_coins`, seeding excludes from binding makers and committing pins only after a successful venue publish.
_Avoid_: Unique pin helpers, binding exclusion set, Direct coin picker

**Remaining-shape ownership**:
Who finishes ladder shape after the primary row is on-chain — offer-post bootstrap versus daemon coin ops — including the inverse handoff when a low-watermark split still belongs to bootstrap combine-first.
_Avoid_: Shape deferral, sub-primary policy, bootstrap vs coin-ops bounce

**Exact-denomination combine**:
Managed and CLI combine that spends only coins of the same clip size. Shape combine-first may cover a target with mixed sizes.
_Avoid_: mixed-denomination cover (for the managed/CLI path)
