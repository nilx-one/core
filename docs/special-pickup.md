<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# Special paid-item pickup choices

Status: presentation-only contract; owner: 0x1 Core.

An Identity-attested paid Special is an item awaiting collection and separate activation.
This projection decides only which choices the game may *suggest* — it never
dispatches xPing, starts a walk, marks an order collected, starts a subscription
clock, proves presence or creates a BondChain.

## Wasm / UniFFI: special_pickup_choices(request)

The `special_pickup_choices_wire` function accepts canonical E7 decimal-string
coordinates as JSON:

```json
{
  "paid": true,
  "point": { "longitude_e7": "305131000", "latitude_e7": "504489000" },
  "avaia": { "longitude_e7": "305131000", "latitude_e7": "504489000" },
  "walk_route_available": true
}
```

For this co-located example the result is:

```json
{"ok":true,"offer_xping":true,"suggest_avaia_walk":true,"avaia_distance_m":"0"}
```

- The `paid` boolean **must be derived from the authenticated Identity
  paid-item projection**, not a client assertion or a local inventory number.
- The host supplies `walk_route_available: true` only when the existing
  pedestrian graph successfully plans a route. Unknown position or a blocked
  route never offers a walk.
- `avaia` is the character's independent in-world position, not the device's
  geolocation. `point` is the existing Market BusinessPoint.
- **Strictly less than 300 metres is a straight-line in-world radius**, using
  Core's `distance_meters` integer calculation. It is *not* a 300 m walking
  route length: obstacles may produce a much longer route. A valid pedestrian
  route and an explicit user choice are still required.
- `avaia_distance_m` is an integer decimal string or `null` when unpaid or
  the Avaia position is unknown. Distance is informational, not an entitlement.

Unpaid returns `offer_xping: false` and `suggest_avaia_walk: false`.
Malformed or untrusted request types, extra fields, numeric instead of string
E7 coordinates or out-of-range coordinates fail closed:

```json
{"ok":false,"error":"invalid"}
```

## Collection and activation authority

Only Identity owns the durable paid item, receipt and state transitions. A
user-confirmed xPing or manual Bond/Avaia arrival opens an authenticated Market
visit; Identity checks the original order, owner and active session before
`collect`. Collection and activation remain separate operations. A local
map hint or proximity is never proof of payment, real-world location, mutual
consent or completed interaction.
