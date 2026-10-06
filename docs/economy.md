# Economy: Seeds, selling, repairs, crafting

Status: Core-owned rules, `ECONOMY_VERSION` = 1. These are pure rules over an `Inventory` that a host keeps. Core does not store an inventory, does not know where a Bond is standing, and does not attest that a workshop was visited.

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

In Kyiv, without the legendary tier, a kilometre of walking brings in about 18 Seeds on average if everything is sold. So a CD player repair (200) costs about 11 km. Legendary finds add about 22 Seeds a kilometre on average, but they are lumpy: one every 100 km.

## Repairs and crafting

A recipe **consumes** things, needs **tools** that are kept, costs Seeds, makes one thing and pays experience **to the Bond**. A repair or a craft is the person's own doing, so it never pays the Avaia. The colours are the host's: blue for the Bond's experience, purple for the Avaia's.

| Recipe                  | Consumes                                         | Tools                  | Seeds | Makes           | Experience | Where           |
| ----------------------- | ------------------------------------------------ | ---------------------- | ----: | --------------- | ---------: | --------------- |
| `repair_cd_player`      | broken CD player                                 |                        |   200 | CD player       |         50 | repair workshop |
| `repair_cassette_player`| broken cassette player                           |                        |   100 | cassette player |         40 | repair workshop |
| `repair_dictaphone`     | broken dictaphone                                |                        |    80 | dictaphone      |         40 | repair workshop |
| `repair_headphones`     | broken headphones                                |                        |    20 | headphones      |         15 | anywhere        |
| `craft_album`           | scratched CD ×3                                  | CD player              |     0 | album           |         75 | anywhere        |
| `craft_kyiv_mixtape`    | blank cassette, the three Kyiv band cassettes    | cassette player        |     0 | Kyiv mixtape    |        120 | anywhere        |
| `craft_field_recording` | blank cassette                                   | dictaphone, microphone |     0 | field recording |         60 | anywhere        |

The example path: you find an old disc (25 experience) and a broken CD player (60), walk to a real workshop, and repair the player there (+50 experience, −200 Seeds). Now the player plays: three discs become an album (+75).

A recipe is all or nothing. A refused recipe (wrong place, a missing thing or tool, too few Seeds) leaves the inventory exactly as it was.

**The repair workshop** is a real place the Bond physically walked to. Which places count is the host's decision: a stall on the radio market, an electronics repair shop on the map. Core takes only the host's answer, `Place::RepairWorkshop` or `Place::Anywhere`. A workshop can also do what needs no particular place.

## What the host still owns

- **Storing the inventory.** It lives on the device, in the encrypted finds journal, like the finds themselves.
- **The authoritative total.** The service holds the Seed balance and the experience against a commitment (an HMAC), as with committed experience. It does not store which things the Bond holds or where it sold them.
- **Item and recipe names** in each language, by `id`.
- **Deciding whether the Bond is at a workshop.**

Changing a price, a recipe or a crafted thing raises `ECONOMY_VERSION`.

---

© 2026 aiaiaiai · aiaiaiai.org
