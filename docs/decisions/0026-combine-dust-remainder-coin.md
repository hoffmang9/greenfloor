# ADR 0026: Combine dusty overshoot takes a remainder coin

## Status

Accepted (2026-08-12).

## Context

Two-sided spread (ADR 0025) moved BYC buy clips off whole CAT units: size 10 is 9,990
mojos and size 25 is 24,975. Combine-first then preferred the tightest covering set
(two 25,025 coins → 49,950 needed, change 100). That leftover is below the 1 CAT
(1,000 mojo) dust floor, so the shaper returned `CannotFund` even with a remainder
coin that could have absorbed legal change.

Lowering the dust floor to 0.1 CAT would allow the 100-mojo case but still block
20-mojo (size-10) and 50-mojo (single size-25) remainders, and would mint awkward
dust clips.

## Decision

1. **Keep the 1 CAT dust floor.** `coin_op_min_amount_mojos` stays 1,000 for CATs.
2. **Retry dusty covers once.** When a covering pick would leave CAT dust change
   (solo oversize or a tight multi-coin set), re-select while skipping dusty
   overshoots (`MinOvershoot`, cap intact) so leftover change lands on an extra
   remainder coin — or on a different pair whose change is already legal.
3. **Fail closed** when no covering set within `combine_input_cap` leaves legal
   change. Do not emit sub-CAT outputs.

## Consequences

- Two 25.025 clips plus a 4.930 remainder can fund two 24.975 buy clips (change 5.030).
- Two 25.025 clips alone still cannot fund that target.
- Daemon flat combine (no dust context) is unchanged: a solo covering pick is still
  "not a combine."
