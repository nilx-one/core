# Find attestation: verified pick-ups and sales

Status: Core-owned rules (`nilxone-identity::find_attestation`, `IdentityRepository::attest_pickup` and `sell_attested`). Owner decision of 2026-11-08 (nilx-one/web#567).

## Why

Seeds are spent only from the Bond's Identity wallet. For a sale to credit that wallet, the service has to know that the thing exists, whose it is, and that it was not sold before. The inventory itself stays on the device (grids, layout, crafting state): only an opaque **attestation id** travels between the device and the service.

## Attesting a pick-up

When the Bond or its Avaia picks a find up, the device reports:

| Field         | Meaning                                                      |
| ------------- | ------------------------------------------------------------ |
| `artifact_id` | The find, `art:seg:<row>:<column>:e<epoch>:<pack>:<slot>`.   |
| `finder`      | `bond` or `avaia`.                                           |
| `found_at_ms` | When it was picked up, on the device's clock.                |
| position      | Where the finder stood, degrees × 10⁷.                       |

The service attests it only if all of these hold (`verify_pickup`):

1. **The find is real.** The service rolls the id itself (`roll_artifact`) and must get exactly that id. The tier is the roll's, never the device's.
2. **The finder may take it.** The Avaia picks up tiers 1–3 only.
3. **The time fits.** Inside the find's week, at most a minute ahead of the service's clock, and at most a day old (`PICKUP_MAX_AGE_MS`), so an offline device can still report it.
4. **The place fits.** Within `PICKUP_RADIUS_METERS` (50 m) of the find's segment cell: 25 m of walk offset, 15 m of perception and 10 m for the phone's fix.
5. **It is not someone else's.** A Bond picks up a find once. A find of a claimed tier (4–6) is one Bond's, whether it was attested or claimed through a committed award first. Commons (1–3) are each Bond's own copy.

Core then names the item (`item_for_find`). The answer is the **attestation id** (a UUID v4) and the item id. A retry, from this or another device, answers the same id and changes nothing.

**Money** (small change) is never a thing: its seeds are credited in the attestation's transaction, once.

## Selling

A sale names only the attestation id. The service credits Core's price of the attested item (`sale_price`, from `economy.md`), once, in one `BEGIN IMMEDIATE` transaction with the mark that it is sold. A retry answers the first credit. Money and things nobody buys (a flyer) are `not_for_sale`. Another Bond's attestation id is `unknown`.

The device removes the thing from its grid only after the service answered `sold` or `already_sold`.

Crafted things have no attestation yet and cannot be sold until their provenance contract exists.

A thing used up in a craft or repair answers `used_up` and is never sold.

## Crafting and repairs

A craft or repair is paid from the Identity wallet (`start_craft_with_seeds`). The device sends a craft id it chose (8–64 characters, its idempotency key), the recipe and the attestation ids of the things it uses up. Core's recipe says the price and which things it consumes; the device only says which of its attested finds they are. In one `BEGIN IMMEDIATE` transaction, under the active client session (like any spend), the service:

1. checks the inputs are exactly the recipe's found things, attested to this Bond, unsold and not used before;
2. debits Core's price (`not_enough_seeds` otherwise);
3. records the receipt and marks each input used up.

A retry with the same craft id, recipe and inputs answers `already_started` and debits nothing; the same id with anything else is `conflicting_replay`. A refusal commits nothing. A recipe that uses up a crafted thing (the Kyiv anthology) is `unverifiable_inputs` until crafted things have attestations. Tools are kept and are not checked.

Running the craft (its timer, the finished thing, its experience) stays on the device under Core's `start_craft` and `finish_craft`.

## What is stored

`find_attestations` keeps, per attestation: the Bond, the artifact id and its sha, week, tier, item, finder, pick-up time and **position**, when it was attested, the money it credited, and when it was sold and for how much. The row is the provenance of every seeds credit a find makes.

This deliberately changes the earlier rule that a common find stays on the device. The pick-up place and time now reach the service for every attested pick-up. Retention and any later deletion of positions are a separate decision.

## What it does not prove

The position is the device's report. The service checks that it is consistent with the find, not that the person was really there; a spoofed location can still pick finds up. What it does prove: every credited seed comes from a real find, of Core's price, once per Bond (once overall for a claimed tier), and never from a number a client sent.

---

© 2026 aiaiaiai · aiaiaiai.org
