<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# StationPubDress contract

Status: v0.1  
Owner: `nilxone-contracts`  
Companion type: `PubDress`

A business Bond keeps one stationed Avaia per shop. A station never walks: it
stands at its shop. `StationPubDress` is the address of such a station, and
`StationPubDress::from_caption` is the one way a station address is formed
from what a business calls it.

Core owns the grammar and the derivation. Persistence owns allocation in the
global namespace, the business role, where a station stands and whether a
business stands at one of its stations. Web owns display and interaction.

## Address grammar

A station is named after its business, not as a companion Avaia:

```text
<owner pub_dress> "-" <place>
```

- The whole address is a valid `PubDress`, so it shares the one global
  namespace with every Bond: no person can hold a station's address, and no
  station can take a person's.
- There is no Avaia `x` prefix and no `ai` ending. A companion Avaia
  (`AvaiaPubDress`, `x…ai`) is a different identity and contract.
- The place is at least one scalar after the `-`. The owner's own address is
  never one of its stations.
- The slug bound of `PubDress` (2–32 scalars) bounds the whole address, owner
  name included.

`StationPubDress::for_owner(owner, value)` reads an address as a station of
that owner, and refuses anything else.

## Caption derivation

A business names each station by a caption, which is stored as written
(trimmed, inner whitespace collapsed, at most 64 scalars). The address is
formed from it once, when the station is made; a later caption never renames
the address.

1. A leading business name, the owner's `pub_dress` (`0xda-market`) or its
   Avaia spelling without `0` (`xda-market`, any case), is the prefix already
   and is dropped.
2. What remains is the place. A caption with no place is refused.
3. The place is lowercased and transliterated: Ukrainian by the official 2010
   national table (Cabinet of Ministers resolution 55), with the word-initial
   forms of `є ї й ю я` and `зг` as `zgh`; the soft sign and apostrophes are
   dropped. Other scalars are kept, and the `PubDress` allowlist decides them.
4. Whitespace between words becomes `-`.
5. The address is the owner's `pub_dress`, `-`, then the place.

| Caption                        | Address                          |
| ------------------------------ | -------------------------------- |
| `xda-market Золоті Ворота`     | `0xda-market-zoloti-vorota`      |
| `0xda-market Батьківщина-Мати` | `0xda-market-batkivshchyna-maty` |
| `В'ячеслав`                    | `0xda-market-viacheslav`         |
| `Поділ 2`                      | `0xda-market-podil-2`            |

## Stable failures

`StationPubDressError::code()`:

- `empty_caption`
- `caption_too_long`
- `no_place`
- `not_owners_address`
- any `PubDressError` code, when the formed or given address is not a
  `PubDress`

## Bindings

Both binding surfaces expose the same two calls:

- `validate_station_pub_dress(owner, value)` answers `valid`, a failure code,
  or `invalid_owner`.
- `station_pub_dress_from_caption(owner, caption)` answers
  `station:<address>` or `error:<code>`.

A station address is presentation and allocation input. It is not presence,
attendance, delivery or a BondChain fact.
