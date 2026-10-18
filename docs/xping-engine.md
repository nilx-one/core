<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# xPing engine

Status: v1 rules (`XPING_ENGINE_VERSION` 1)  
Owner: Core (rules); the identity service stores the state and owns the clock

xPing can **force** a flight: climb higher and fly faster for as long as the
person holds the force control. Forcing spends the engine's energy. This
contract fixes how much, how it comes back, and the breakdown that forcing on
an empty engine causes. How a forced flight looks and steers is the
[flyover](xping-flyover.md)'s.

| Rule | Value |
| --- | --- |
| Full engine | 1000 permille (`XPING_FULL_ENERGY`) |
| Forcing | 30 s of forcing spend a full engine (`XPING_FORCE_FULL_MS`) |
| Recharge | evenly, empty to full in 12 h (`XPING_RECHARGE_FULL_MS`) |
| Breakdown | 2 s of forcing on an empty engine, counted by Core across reports; any energy coming back starts the count over (`XPING_BREAK_AFTER_EMPTY_MS`) |
| Broken | 12 h, empty, then whole and full (`XPING_BROKEN_MS`) |
| Paid repair | 50 Seeds ₴€£ for an empty or broken engine, in proportion to the energy missing at the moment of the repair, rounded up (25 for half) |
| Smoke | the missing energy, in permille: it stays after the flight until recharged or repaired |

## State and time

The stored engine is `{ "energy_permille": 1000, "at_ms": "…", "empty_forced_ms"?: "…", "broken_at_ms"?: "…" }`.
The empty string is a new, full engine. Core keeps no clock: the service that
stores the engine decides `now_ms`, so a breakdown is recorded at its time.

## Wire

- `xping_engine_view(state, now_ms)` answers `{"ok":true,"view":{energy_permille, smoke_permille, broken, empty_margin_ms, full_at_ms, repair_seeds}}`.
- `xping_engine_command(state, command, now_ms)` takes one of
  - `{"op":"force","forced_ms":"…"}`: spends that much forcing. Forcing past empty is counted, and the report that brings it to 2 s breaks the engine. There is no separate breakdown command: a host cannot break a working engine, nor keep forcing an exhausted one;
  - `{"op":"repaired","paid_seeds":"…"}`: whole and full, refused as `underpaid` below the engine's price at `now_ms` and as `full` when nothing is missing;

  and answers `{"ok":true,"state":…,"view":…}` with the next state to store, or
  `{"ok":false,"error":"invalid"|"broken"|"full"|"underpaid"}`.
- `xping_repair(engine, inventory, now_ms)` is the payment: one transition over
  both states, priced from the engine itself. It answers
  `{"ok":true,"inventory":…,"engine":…,"outcome":…}` or
  `{"ok":false,"error":"not_due"|"not_enough_seeds"|"invalid"}`. There is no
  host-priced payment command.

## Hosting obligations

The inventory is device-side (`nilx-one/web` keeps it in its committed
journal); the engine is stored by the identity service. So a repair crosses
two stores, and the hosts keep it whole:

1. The device reads the engine and the service's `now_ms`, and computes
   `xping_repair` against its inventory at that time.
2. It sends `repaired` with `paid_seeds` from that outcome and the engine
   state it priced from. The service applies it only if its stored engine is
   still exactly that state (compare-and-set), so a replay or a concurrent
   payment from another device repairs nothing twice. Since a price only
   falls as the engine recharges, a payment priced at step 1 still covers the
   repair recorded at the service's later time.
3. Only once the service accepted does the device store the inventory from
   step 1. A refusal stores nothing on either side.

The service cannot see the device's Seeds, so the payment itself has
`authority: "client"`, as experience totals do; Core guarantees the price,
the service guarantees one repair per engine state.

The starter Seeds are a device-side inventory transition too. Their
once-only flag (`starter`) lives in the inventory state, and the host applies
the grant inside its per-owner inventory queue, so a replay answers
`not_due` and grants nothing.

## What it never is

Forcing, breakdown and repair are presentation and game state. They are never
a purchase, a subscription, a `BondChain` interaction or proof of presence.
