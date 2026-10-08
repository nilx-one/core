# Avaia proximity

The host supplies the **observed Bond–Avaia distance in whole metres**, the
number of artifacts in a candidate cell, and whether the previous answer
blocked reveals. Core returns a pure policy projection via
`avaia_proximity(distance_m, artifacts, previously_blocked)` from the same
implementation in Wasm and UniFFI. The host does not decide eligibility.

Core keeps no state. The host stores `!can_reveal` of the last answer and
passes it back; pass `true` when there is none (first sample, new owner,
unknown history) so an unknown past never authorizes work in the band.

| Distance | `level` | New / continuing work |
| --- | --- | --- |
| below 15 m | `near` | allowed |
| 15 m to below 4500 m | `working` | allowed |
| 4500 m to below 5000 m | `restricted` | allowed only if it was not blocked before |
| 5000 m or more | `red` | blocked |

- **Blocking** happens at `red_m` (5000 m), whichever way the Avaia got there.
- **Restoring** happens strictly below `restore_below_m` (4500 m, 90% of
  red). Between the two the previous state holds: an outward trip stays open
  up to red, a return trip stays blocked until 4500 m. The rule is tested in
  both directions.
- `near` is strictly below 15 m (the visual tier); the one-minute reveal
  covers **up to and including** 15 m. At exactly 15 m the level is
  `working` and the duration is one minute.

## Duration

`duration_ms` is `null` when blocked. Otherwise it is one minute up to 15 m
and grows linearly with distance; the far baseline is 5 minutes at 4500 m,
and each artifact adds up to a minute more at the far end (the effect scales
with distance, so it is nil near the Bond). The result is always within
1–10 minutes by construction. In the restricted band the distance term stays
at its 4500 m ceiling. Artifacts above 5 count as 5 (`MAX_COUNTED_ARTIFACTS`):
hosts pass the real count and never mirror the cap.

5000 m is the initial configurable policy boundary derived from the walk
energy cost (1% per 50 m). These bands do not constrain movement, routing or
independent Avaia agency, only manual and autonomous fog reveals.

## Host duties

Invalid or missing position/policy must **not** authorize a reveal; an empty
or unparsable answer is a refusal. For existing jobs, callers must pause
progress while Core reports `can_reveal: false`; a wall-clock completion
cannot override a blocked capability, and time spent closed or unreported is
not authorized time. Previously saved work may resume with its remaining
duration, but may not be completed retroactively while the capability is
blocked.

Proximity does not constitute consent, reciprocal action, BondChain evidence
or a Relationship; this is local Avaia gameplay.

© 2026 aiaiaiai · aiaiaiai.org
