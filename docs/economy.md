# Economy: Seeds, selling, repairs, crafting

Status: Core-owned rules, `ECONOMY_VERSION` = 1. These are pure rules over an `Inventory` (two grids, Seeds, one running craft) that a host keeps. Core does not store an inventory, does not know where a Bond is standing, and does not attest that a workshop was visited.

## Seed ₴€£

There is one in-game currency everywhere: **Seed** (Ukrainian: **Зерно**). The code is `seed` and the emblem is `₴€£`. The emblem is one mark for one currency, not three currencies. Balances are whole Seeds (`u64`), with no fractions.

The name always appears together with the emblem: "Seed ₴€£", never a bare "seed". In crypto, "seed" means a recovery phrase, and a Bond has no such phrase and never will (see `nilx-one/web` `docs/bnd-file-lifecycle.md`). The emblem keeps a request like "send me your seeds" from reading as the real thing. In code, "seed" also means a seed for random generation. The money is `SEED_CODE`, `Inventory::seeds`, `FindItem::seeds`.

Seeds come from:

- **picking up money:** small change (a tier 1 find) credits 15 Seeds and is never kept as a thing;
- **selling** a thing.

Seeds are spent on repairs.

## Selling

Every thing with a price can be sold anywhere, for now. A flyer has no price and nobody buys it. A sale removes the things and credits `price × count`.

| Thing                                                       | Seeds ₴€£ |
| ----------------------------------------------------------- | --------: |
| bottle cap                                                  |         2 |
| can                                                         |         4 |
| bottle                                                      |         5 |
| metro token                                                 |         8 |
| scratched CD, broken headphones                             |        10 |
| blank cassette                                              |        15 |
| a cassette of a local band                                  |        20 |
| headphones (repaired)                                       |        40 |
| broken cassette player, broken CD player, broken dictaphone |        50 |
| CD radio                                                    |        80 |
| field recording                                             |       150 |
| dictaphone, microphone, cassette player                     |       200 |
| album                                                       |       350 |
| Kyiv mixtape                                                |       500 |
| CD player                                                   |       600 |
| reel-to-reel                                                |      2000 |
| test pressing                                               |      2500 |
| Kyiv anthology (crafted, legendary)                         |      5000 |

In Kyiv, without the legendary tier, a kilometre of walking brings in about 18 Seeds on average if everything is sold. So a CD player repair (200) costs about 11 km. Legendary finds add about 22 Seeds a kilometre on average, but they are lumpy: one every 100 km.

## Repairs and crafting

A recipe **consumes** things, needs **tools** that are kept, costs Seeds, makes one thing and pays experience **to the Bond**. A repair or a craft is the person's own doing, so it never pays the Avaia. The colours are the host's: blue for the Bond's experience, purple for the Avaia's.

| Recipe                   | Consumes                                      | Tools                  | Seeds | Makes           | Experience | Time   | Where           |
| ------------------------ | --------------------------------------------- | ---------------------- | ----: | --------------- | ---------: | ------ | --------------- |
| `repair_headphones`      | broken headphones                             |                        |    20 | headphones      |         15 | 15 min | anywhere        |
| `repair_dictaphone`      | broken dictaphone                             |                        |    80 | dictaphone      |         40 | 30 min | repair workshop |
| `repair_cassette_player` | broken cassette player                        |                        |   100 | cassette player |         40 | 30 min | repair workshop |
| `repair_cd_player`       | broken CD player                              |                        |   200 | CD player       |         50 | 45 min | repair workshop |
| `craft_field_recording`  | blank cassette                                | dictaphone, microphone |     0 | field recording |         60 | 30 min | anywhere        |
| `craft_album`            | scratched CD ×3                               | CD player              |     0 | album           |         75 | 45 min | anywhere        |
| `craft_kyiv_mixtape`     | blank cassette, the three Kyiv band cassettes | cassette player        |     0 | Kyiv mixtape    |        120 | 45 min | anywhere        |
| `craft_kyiv_anthology` ★ | album, Kyiv mixtape, field recording          | reel-to-reel           |     0 | Kyiv anthology  |        500 | 7 days | anywhere        |

★ is a legendary craft. The Kyiv anthology sells for 5000.

The example path: you find an old disc (25 experience) and a broken CD player (60), walk to a real workshop, and confirm the repair there (−200 Seeds). 45 minutes later the player is done (+50 experience). Now it plays: three discs become an album (+75).

**The repair workshop** is a real place the Bond physically walked to. Which places count is the host's decision: a stall on the radio market, an electronics repair shop on the map. Core takes only the host's answer, `Place::RepairWorkshop` or `Place::Anywhere`. A workshop can also do what needs no particular place.

### Time

A craft runs in the background, like opening a cell. The person **confirms** it, then it runs on the clock:

1. `start_craft` at confirmation takes the inputs and the Seeds at once. The tools stay in the grid. One craft runs at a time.
2. `finish_craft` from `ready_ms` on puts the thing into the Bond's grid and pays the experience. If there is no room, the craft stays done and waits.
3. Everyday crafts take 15–45 minutes. A **legendary** craft takes **a week**, or finishes at once **for real money**: `finish_paid` works only for a legendary recipe. The host calls it only after the service has confirmed the payment, because Core cannot check that.

Everything is all or nothing. A refused start (busy, wrong place, a missing thing or tool, too few Seeds) leaves the inventory exactly as it was. Time is milliseconds from the caller, and the service's clock is the one that counts. `resume_craft` restores a stored craft after a restart.

## Two grids, as in S.T.A.L.K.E.R.

The Bond and its Avaia each have their own grid (`Holder::Bond`, `Holder::Avaia`). Every thing takes a rectangle of cells and is never turned. How many cells a grid has depends on what the things are carried in:

| Carried in | Grid    | Cells |
| ---------- | ------- | ----: |
| pockets    | 5 × 1   |     5 |
| a backpack | 8 × 5   |    40 |
| a whole bag| 12 × 10 |   120 |

By default, both carry a backpack. Sizes: most small things are 1×1. A bottle and a microphone are 1×2, headphones and a CD player are 2×1, a cassette player and a test pressing are 2×2, a CD radio is 3×2, a reel-to-reel is 3×3. A bottle does not fit in a pocket, and a reel-to-reel does not fit in a pocket either.

- **A pick-up** goes to the first free place, row by row from the top left. If it fits nowhere, the find stays where it lay. Money (small change) is credited to the Bond, whoever picked it up. The Avaia has no Seeds.
- **Rearranging** moves a thing to a chosen cell, all or nothing.
- **Handing over** moves a thing from one grid to the other. Whether that is allowed right now (the two met, say) is the host's call.
- **Changing the carry** keeps everything where it lies if it fits. Otherwise it repacks the largest things first, or refuses.
- **Selling and crafting** use only the Bond's grid. What the Avaia found is handed over first.

## The wire

The bindings expose the economy as strings, so the web and other hosts run exactly these rules:

- `find_item(artifact_id, tier)` returns `item:<id>` or `error:<code>`.
- `picks_up(rarities, tier)` returns `yes`, `no` or `error:<code>`, for the pick-up setting.
- `economy_catalog()` returns things, recipes, grids and the currency as JSON. Hosts draw the UI from it and name things by `id`.
- `apply_inventory_command(state, command, now_ms)` takes the stored inventory JSON (the empty string means a new one), one command, and the time as a decimal string. It answers `{"ok":true,"state":…,"outcome":…}` with the next state to store, or `{"ok":false,"error":"<code>"}`. A refused command means the host keeps the old state.

Commands (`op`): `pick_up` (`holder`, `artifact_id`, `tier`: Core decides which item it is), `rearrange` (`holder`, `from`, `to`), `hand_over` (`from`, `x`, `y`), `switch_carry` (`holder`, `carry`), `sell` (`id`, `count`), `start_craft` (`recipe`, `place`), `finish_craft`, `finish_paid`. Every u64 is a decimal string. Unknown members are refused, and so is a stored state with overlapping things or things outside their grid: it is never repaired.

## What the host still owns

- **Storing the inventory** (both grids, the Seeds and the running craft). It lives on the device, in the encrypted finds journal, like the finds themselves.
- **The authoritative total.** The service holds the Seed balance, the experience and the craft's times against a commitment (an HMAC), as with committed experience. It does not store which things the Bond holds or where it sold them.
- **The real-money payment** for a legendary craft, and its confirmation.
- **The confirmation prompt** before a craft, and its notice when the craft is done.
- **Item and recipe names** in each language, by `id`.
- **Deciding whether the Bond is at a workshop**, and whether the Bond and the Avaia have met.

Changing a price, a recipe, a crafted thing or a size raises `ECONOMY_VERSION`.

---

© 2026 aiaiaiai · aiaiaiai.org
