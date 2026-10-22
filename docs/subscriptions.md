# Shared subscription status

Core owns `SubscriptionTier`, `SubscriptionSnapshot` and the deterministic
`SubscriptionStatus` resolver. Web, iOS and other clients consume this same
status for every surface: profile, Dock, identity labels, lists and avatars.
The source contract is 0x1 `documents/product-direction/special-exclusive-subscriptions.md`.

The identity service owns durable entitlements and revocations. A trusted adapter
supplies a snapshot for the current authenticated Bond, including a bounded
freshness window and either one effective entitlement or confirmed absence.
Core does not authenticate snapshots, process payments, grant access or persist
anything. Never accept a snapshot from settings, pickup preferences, browser
storage, a payment-in-progress, loot rarity or an arbitrary client assertion.

`subscription_status(snapshot_json, owner_pub_dress, now_ms)` is identical in
Wasm and UniFFI. Times are canonical decimal u64 strings. Snapshot schema:

```json
{
  "bond_pub_dress": "0x0sky",
  "checked_at_ms": "100",
  "refresh_after_ms": "400",
  "entitlement": {
    "tier": "exclusive",
    "starts_at_ms": "150",
    "expires_at_ms": "300"
  }
}
```

At `200` the answer is `{"kind":"active","bond_pub_dress":"0x0sky","tier":"exclusive","valid_until_ms":"300"}`.
At `300` it is `{"kind":"none","valid_until_ms":"400"}`; at `400`, unknown.
`entitlement: null` means confirmed absence/revocation. A missing snapshot
(`null`), malformed input, wrong owner, future observation or stale snapshot
means `{"kind":"unknown"}`. Unknown must never be relabelled as free or drive
an upsell. Start is inclusive; expiry and freshness deadlines are exclusive.
Clients reevaluate at `valid_until_ms` and refresh through their authority
adapter. Clock rollback before the observation fails closed. A fresh downgrade
or revocation replaces the old status immediately. There is no retained fallback
entitlement after an authority read fails.

Subscriptions belong to the Bond. Its currently associated Avaia may display
the same status; association must come from the authenticated identity context,
never an address-prefix guess. Switching owners or Avaia associations clears
the previous presentation immediately. An Avaia does not acquire an independent
subscription. Exclusive is one tier, not Exclusive plus a second Special grant.

Core contains no gold/violet values, glow intensity or renderer data. Those are
client presentation. These results also do not change XP, pickup authorization,
BondChain, Relationship, consent, or presence. Server access checks remain
authoritative. This additive API is version-one capability, independent of the
existing contract version; an older runtime without the export is unknown.

## Exclusive priority and paused Special

`SubscriptionClock` is the pure authority-side duration policy. Exclusive has
priority: while it has remaining time, Special's remaining time is paused.
At Exclusive's exact expiry, Special resumes automatically with its unconsumed
duration. Only one tier is effective at any instant.

For example, Special has 8 days remaining when 3 days of Exclusive are granted.
For the next 3 days only Exclusive runs; afterwards Special still has 8 days.
Extending Exclusive keeps Special paused. Special time granted while Exclusive
is active is added to the paused balance, not consumed concurrently.

The clock stores a Bond owner, settlement timestamp and both remaining durations
as canonical decimal-u64 values. `advance` consumes elapsed Exclusive time first,
then Special; this works across offline intervals without scheduled jobs.
`grant` settles before adding an authorized duration and rejects zero time,
rollback and overflow without changing the original state. `snapshot` projects
the effective period into the existing Wasm/UniFFI status contract. Consumers
must refresh at the returned deadline to observe resumed Special; they must not
infer it from an expired Exclusive snapshot.

The identity authority must persist the clock transactionally, authenticate the
owner, deduplicate payment/grant events, and supply its own trusted time. These
pure methods do not provide persistence, payment fulfillment or permission to
grant subscriptions. Do not expose `grant` as an unauthenticated client action.
Early revocation/refund handling remains a separate authority decision; this
policy specifies natural expiry, not refund conversion.

Delivery remains staged: no production entitlement reader or payment fulfillment
exists yet. An implementation of this resolver alone must not be reported as
live paid subscriptions.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
