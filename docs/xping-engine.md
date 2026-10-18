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
| Breakdown | 2 s of forcing on an empty engine (`XPING_BREAK_AFTER_EMPTY_MS`) |
| Broken | 12 h, empty, then whole and full (`XPING_BROKEN_MS`) |
| Paid repair | 50 Seeds ₴€£ for an empty or broken engine, in proportion, rounded up (25 for half) |
| Smoke | the missing energy, in permille: it stays after the flight until recharged or repaired |

## State and time

The stored engine is `{ "energy_permille": 1000, "at_ms": "…", "broken_at_ms"?: "…" }`.
The empty string is a new, full engine. Core keeps no clock: the service that
stores the engine decides `now_ms`, and records a breakdown at its own time.

## Wire

- `xping_engine_view(state, now_ms)` answers `{"ok":true,"view":{energy_permille, smoke_permille, broken, full_at_ms, repair_seeds}}`.
- `xping_engine_command(state, command, now_ms)` takes one of
  - `{"op":"force","forced_ms":"…"}`: spends that much forcing (never below empty);
  - `{"op":"break_down"}`: only an empty, working engine breaks;
  - `{"op":"repaired"}`: a paid repair, whole and full;

  and answers `{"ok":true,"state":…,"view":…}` with the next state to store, or
  `{"ok":false,"error":"invalid"|"broken"|"not_empty"|"full"}`.

## Payment

A paid repair is paid from the inventory with
`{"op":"pay_xping_repair","missing_permille":N}` ([economy](economy.md)), which
spends `repair_seeds_for(N)`. The host pays first, then records `repaired` on
the engine. The engine never moves Seeds itself.

## What it never is

Forcing, breakdown and repair are presentation and game state. They are never
a purchase, a subscription, a `BondChain` interaction or proof of presence.
