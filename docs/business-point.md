<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# BusinessPoint contract

Status: v0.1  
Owner: `nilxone-contracts`  
Normative source: 0x1 `13-business-bonds-and-presence.md`

A `BusinessPoint` is a business-owned, addressable location record with a
claim verification state. It is not a Bond, an Avaia, a `pub_dress`, a
station, or a physical/digital presence right.

Core owns the record shape, its validation and every state transition. The
host owns authentication of the business representative and of the
verification authority, the organization's own verification status,
persistence, notifications and the map projection. Core never decides how
manual verification is carried out.

## Lifecycle

```text
draft -- submit (verified organization) --> pending
pending -- decide verified ---------------> verified
pending -- decide rejected ---------------> rejected
pending -- deadline reached --------------> expired
pending | verified -- withdraw -----------> withdrawn
rejected | expired | withdrawn -- resubmit --> pending
```

- `deadline_at = submitted_at + 72 h`, and the deadline is inclusive: at
  `deadline_at` an unresolved point is already `expired`, whether or not a
  cleanup job has run.
- A repeated `submit` inside the window returns the same state and never
  restarts the clock.
- A decision at or after the deadline is refused; it never revives a claim.
- `decided_at` is the verification outcome (or the deadline, for `expired`).
  `withdrawn_at` is the withdrawal and never overwrites `decided_at`.
- A location change returns the point to `draft`; a rename keeps the claim.

## Binding surface

Both bindings expose the same two functions. Clocks are the service clock in
**seconds**, as a decimal string.

`apply_business_point_command(state, command, now_s)` applies one command to
a stored point, `state` being the empty string only for `create`:

```json
{"op":"create","owner":"0xda-market","identity":{"id":"point-1","name":"Zoloti Vorota"},"location":{"address":{"country":"UA","locality":"Kyiv","street":"Volodymyrska","number":"40"},"coordinate":{"longitude_e7":"305131000","latitude_e7":"504489000"}}}
{"op":"submit","organization_verified":true}
{"op":"resubmit","organization_verified":true}
{"op":"decide","decision":"verified"}
{"op":"expire"}
{"op":"withdraw"}
{"op":"amend_location","location":{…}}
{"op":"rename","name":"New caption"}
```

It answers `{"ok":true,"state":…,"status":…}` with the next state to store
and its effective status, or `{"ok":false,"error":"<code>"}`. A refused
command leaves the stored state as it was.

`business_point_view(state, now_s, organization_verified)` answers
`{"ok":true,"status":…,"visibility":…}`, where `visibility` is `hidden`,
`pending_preview` or `verified_listing`. A point of an organization that is
not verified is always `hidden`.

`organization_verified` and `decide` are host assertions: the host MUST NOT
take them from a client.

## Failure codes

`invalid` (malformed state, command or clock), `invalid_identity`,
`invalid_address`, `invalid_state`, `invalid_transition`,
`unverified_organization`, `expired`, `not_expired`, `clock_reversal`,
`timestamp_overflow`.
