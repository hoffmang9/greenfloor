# GreenFloor

Operator policy for posting, shaping, and reconciling Chia offers from a vault — without running a full node or in-process wallet sync.

## Language

**Unique maker pin session**:
The in-batch protocol that pins a distinct exact-size receive-address maker coin for each new Direct offer under `unique_maker_coins`, seeding excludes from binding makers and committing pins only after a successful venue publish.
_Avoid_: Unique pin helpers, binding exclusion set, Direct coin picker
