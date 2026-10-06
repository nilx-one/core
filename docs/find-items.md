# Find items

Status: Core-owned catalog. The web client rolls a chance find and its tier (`rollSegment`, `packages/artifact-contract` in `nilx-one/web`), and the identity service rolls the same find to check claims. This module answers the next question for both: **which item that find is**, what picking it up pays, and whether the Bond wants that rarity picked up.

## Rarity

| Tier | Rarity    | Settings group |
| ---- | --------- | -------------- |
| 1–3  | common    | "1–3"          |
| 4    | uncommon  | "4"            |
| 5    | rare      | "5"            |
| 6    | legendary | "6"            |

`PickupRarities` is the Bond's setting: which rarities get picked up. By default, all of them. A find of a rarity left out is still seen and still pays its sighting. It is only not picked up. The wire form is the included codes, commonest first, joined by `,` (`common,uncommon,rare,legendary`). An unknown or repeated code is refused.

## The catalog

`CATALOG`, version `FIND_CATALOG_VERSION` = 1. Music connects the items: cassettes of local bands, broken and working players, and the things that later repairs and crafting are built from. The bands are invented, like the rest of the pack: no real artist or brand.

An item's experience is its own, not its tier's. A CD radio is a rare find (tier 5) and pays 25 on its own, because what it plays is the point. `coins` is in-game money the item is worth on the spot (small change). For every other item it is `0`.

| Tier | Item                                                  | Experience | Notes                          |
| ---- | ----------------------------------------------------- | ---------: | ------------------------------ |
| 1    | bottle ×3, can ×3, bottle cap ×2, flyer, small change |         10 | small change is worth 5 coins  |
| 1    | metro token                                           |         10 | Kyiv                           |
| 2    | scratched CD ×2, blank cassette, broken headphones    |         25 |                                |
| 2    | cassettes: Podil at Dawn, Left Bank Echo, Trukhaniv Summer |         25 | Kyiv, invented local bands     |
| 3    | broken cassette player, broken CD player, broken dictaphone |         60 | broken: a repair starts here   |
| 4    | dictaphone, microphone, cassette player               |        150 |                                |
| 5    | CD player                                             |        400 |                                |
| 5    | CD radio                                              |         25 | rare to find, worth little     |
| 6    | reel-to-reel                                          |       1000 |                                |
| 6    | test pressing                                         |       1000 | Kyiv                           |

`×n` is the item's weight among its tier. Unmarked items weigh 1.

## The pick

`item_for_find(artifact_id, tier)`:

1. The artifact id `art:seg:<row>:<column>:e<epoch>:<packVersion>:<slot>` is checked to be canonical.
2. Its segment decides the city. Cities are inclusive ranges of segment rows and columns, so no coordinate is reconstructed and no floating point is involved. Kyiv, Saint Petersburg and Barcelona are defined. Today only Kyiv has items of its own; elsewhere only shared items turn up.
3. The pool is that tier's items that are shared or belong to the city.
4. A 32-bit FNV-1a hash of `nilx-one.find-item.v1:` followed by the id, modulo the pool's total weight, picks the item by weight.

The pick is integer arithmetic over bytes, so every runtime gets the same item. The golden test pins one Kyiv and one non-city find per tier. Changing an item, a weight, the city bounds or the hash changes the golden test and must raise `FIND_CATALOG_VERSION`. The web client and the service follow.

## Not here yet

- Experience by item on the server (the service still prices a pick-up by tier).
- The inventory, in-game money and its balance.
- Repairs at a real workshop and crafting.
- Item names: hosts name items in their own languages by `id`.

---

© 2026 aiaiaiai · aiaiaiai.org
