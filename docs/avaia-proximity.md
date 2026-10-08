# Avaia proximity

The host supplies the **observed Bond–Avaia distance in whole metres** and
the number of artifacts in a candidate cell. Core returns a pure policy
projection via `avaia_proximity(distance_m, artifacts)` from the same
implementation in Wasm and UniFFI. The host does not decide eligibility.

- **Within 15m**: cyan, one-minute reveal, regardless of artifacts.
- **16–4499m**: reveal allowed; baseline grows from 1 toward 5 minutes,
  artifact overhead adds up to another 5 minutes; maximum 10 minutes.
- **4500–4999m**: restricted; cannot start new work.
- **5000m or more**: red; cannot start new work.
- Work becomes available again only **strictly below 4500m** (90% of red).

5000m is the initial configurable policy boundary derived from the
walk energy cost (1% per 50m). These bands do not constrain movement,
routing or independent Avaia agency, only manual and autonomous fog reveals.

Invalid or missing position/policy must **not** authorize a reveal. For
existing jobs, callers must pause progress while Core reports a blocked
tier; a wall-clock completion cannot override a blocked capability.
Previously saved work may resume with its remaining duration, but may not
be completed retroactively while the capability is blocked.

Proximity does not constitute consent, reciprocal action, BondChain evidence
or a Relationship; this is local Avaia gameplay.

© 2026 aiaiaiai · aiaiaiai.org
