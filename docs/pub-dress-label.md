<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# PubDressLabel contract

Status: v0.1
Owner: `ox1-contracts`
Companion type: `PubDress`

`PubDressLabel` is the canonical DNS-label form allocated to a Bond public address. It is a protocol contract, not a deployment rule: Core owns folding and validation, while persistence, collision allocation, DNS zones, TLS, HTTP routing, and UI remain outside this crate.

## Why the label is stored separately

`PubDress` is case-sensitive while DNS label comparison is case-insensitive. The mapping is therefore intentionally non-injective: distinct canonical identities such as `0x0Sky` and `0x0sky` both fold to the stem `0x0sky`.

A label must never be reverse-computed into a `PubDress`. Allocation is authoritative only when a persistence owner atomically claims the unique label. Resolution before that transaction is advisory. The first successful claim owns the label; a later colliding Bond must allocate a distinct suffix.

## v0.1 mapping

The v0.1 rule is deliberately small and deterministic:

- preserve the literal `0x` prefix;
- preserve the already-lowercase hexadecimal discriminator;
- fold ASCII `A-Z` to `a-z` in the slug;
- fold ASCII `A-Z` to `a-z` in a collision suffix;
- allow only `[a-z0-9-]` in the resulting label;
- reject a leading or trailing hyphen;
- reject a label longer than 63 octets;
- reject a suffix longer than 8 ASCII characters;
- keep every resulting Bond label inside the `0x` namespace.

No general Unicode lowercase operation is part of the contract.

## Non-ASCII policy

v0.1 resolves the non-ASCII decision as **refuse**.

Core does not apply IDNA/UTS-46, Punycode, or transliteration. A canonical `PubDress` containing a non-ASCII scalar that the current `PubDress` grammar permits remains a valid identity, but `PubDressLabel::stem` returns `NonAscii` and no DNS label is derived.

This decision is intentionally versioned. A future contract may define a pinned IDNA mapping, but changing the mapping of already allocated labels would break address ownership and therefore requires an explicit protocol revision and migration design.

## Canonical `PubDress` boundary

`PubDressLabel` does not broaden the `PubDress` grammar.

The current `PubDress` contract rejects Cyrillic identities such as `0x0небо`, so a typed call to `PubDressLabel::stem(&PubDress)` can never receive that value. The raw-string convenience boundary classifies it as `NotAPubDress`.

By contrast, the current `PubDress` grammar does permit some non-ASCII scalars such as `₴`, `€`, `£`, and `•`. A canonical value containing one of those scalars reaches the typed label boundary and is rejected as `NonAscii`.

This distinction is normative for v0.1:

- invalid identity syntax -> `NotAPubDress` at the raw-string boundary;
- valid identity with no v0.1 DNS representation -> `NonAscii` at label derivation;
- `stem(&PubDress)` accepts only an already-canonical identity.

## API

`ox1-contracts` exposes:

- `PubDressStem`, containing the canonical folded stem and `was_folded()` state;
- `PubDressLabel`, a validated allocatable label;
- `PubDressLabelError`, the stable error classification;
- `PubDressLabel::stem(&PubDress)`;
- `PubDressLabel::stem_from_str(&str)` for raw boundaries;
- `PubDressLabel::compose(&PubDressStem, suffix)`;
- `PubDressLabel::parse(label)` for resolution boundaries;
- `PubDressLabel::as_str()`;
- `PUB_DRESS_LABEL_MAX_OCTETS = 63`;
- `PUB_DRESS_LABEL_SUFFIX_MAX_LENGTH = 8`.

Construction is the validation boundary. Consumers should not create or interpret label strings using a parallel rule.

## Ownership boundary

Core owns only deterministic label semantics. It must not depend on:

- a DNS zone;
- wildcard DNS or TLS;
- HTTP endpoints;
- a database or uniqueness index;
- collision retry policy;
- Web UI or host-specific behavior.

Those concerns consume this contract downstream. In particular, persistence must enforce label uniqueness atomically rather than using a check-then-insert flow.

## Compatibility vectors

The implementation is expected to preserve these representative results:

| Input | Result |
| --- | --- |
| `0xda-sha` | stem `0xda-sha`, not folded |
| `0xdA-Sha` | stem `0xda-sha`, folded |
| `0x0Sky` | stem `0x0sky`, folded |
| `0x0sky` | stem `0x0sky`, not folded |
| `sky` | `NotAPubDress` at raw boundary |
| `0xgsky` | `NotAPubDress` at raw boundary |
| `0xDsky` | `NotAPubDress` at raw boundary |
| `0x0sky_one` | `InvalidCharacter` |
| `0x0sky-` | `BoundaryHyphen` |
| `0x0небо` | `NotAPubDress` at raw boundary under the current `PubDress` grammar |
| canonical non-ASCII `PubDress` such as `0x0₴€` | `NonAscii` |

Composition preserves the same ASCII-only fold. For example, stem `0x0sky` plus suffix `TWO` produces `0x0skytwo`; invalid punctuation, boundary hyphens, overlong suffixes, and labels beyond 63 octets fail closed.
