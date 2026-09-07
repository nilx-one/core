<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# PubDressLabel contract

Status: v0.1  
Owner: `ox1-contracts`  
Companion type: `PubDress`

`PubDress` and `PubDressLabel` are deliberately different contracts.

`PubDress` is the exact, case-sensitive Bond identity chosen by a person. `PubDressLabel` is a deterministic DNS A-label projection of that identity. DNS is one transport surface; it must never redefine the identity itself.

Core owns both contracts. Persistence owns atomic label allocation. Web owns presentation. DNS/TLS deployment remains infrastructure.

## PubDress identity boundary

Canonical `PubDress` syntax remains:

- literal `0x`;
- one lowercase hexadecimal discriminator (`0`–`f`);
- a slug of 2–32 Unicode scalar values.

The existing accepted slug scalars remain valid. In addition, Unicode letters, combining marks, and decimal digits are valid human-name scalars. Examples include:

- `0x0небо`
- `0x0Небо`
- `0xdпривіт`
- `0x0ΟΔΟΣ`
- `0x0日本`
- `0x0café`

Accepted input is stored exactly. Core performs no transliteration, case folding, or silent Unicode normalization at the `PubDress` boundary.

Therefore `0x0небо` and `0x0Небо` are distinct identities, just as `0x0Sky` and `0x0sky` are distinct identities.

Emoji remains outside the identity grammar. Leading/trailing whitespace and an uppercase discriminator remain invalid.

A valid `PubDress` is not guaranteed to have a DNS representation. Identity validity and DNS representability are separate decisions.

## UTS-46 DNS projection

`PubDressLabel` derives one lowercase ASCII DNS A-label from a canonical `PubDress`.

Core uses the Rust `idna` UTS-46 implementation with these normative parameters:

- `Transitional_Processing = false`;
- `UseSTD3ASCIIRules = true`;
- `CheckHyphens = true`;
- `CheckBidi = true`;
- `CheckJoiners = true`;
- `VerifyDnsLength = true`.

The encoder owns mapping and case handling. Callers must not pre-lowercase Unicode or normalize the identity first. General Unicode lowercase is not an equivalent operation; in particular, case handling around characters such as Greek sigma can otherwise drift from UTS-46.

The implementation is pinned by exact Core dependencies:

- `idna = 1.1.0`;
- `idna_adapter = 1.1.0` (the unicode-rs backend);
- `idna_mapping = 1.1.0`;
- `unicode-bidi = 0.3.18`;
- `unicode-joining-type = 1.0.0`;
- `unicode-normalization = 0.1.25`.

The `PubDress` general-category table is separately pinned to `unicode-general-category = 1.1.0`, generated from Unicode 16.0 data. These pins are part of the contract: dependency upgrades that can alter mapping or accepted scalar categories require explicit compatibility review.

## Stable failures

`PubDressLabelError` exposes:

- `NotAPubDress`
- `InvalidCharacter`
- `DisallowedScalar`
- `BidiRule`
- `NotEncodable`
- `BoundaryHyphen`
- `TooLong`
- `SuffixTooLong`

The raw convenience boundary may classify a structurally valid-looking value containing a scalar outside the Unicode human-name policy (for example `0x0🌍`) as `DisallowedScalar`, even though the value itself is not a valid `PubDress`. The typed `stem(&PubDress)` boundary can only receive canonical identities.

## Length and collision suffixes

The DNS limit is 63 octets and is measured on the final ASCII A-label after UTS-46 encoding.

A collision suffix:

- is at most 8 characters;
- contains only lowercase ASCII `[a-z0-9]`;
- is appended to the exact `PubDress` source before encoding;
- is never appended to an already encoded `xn--` A-label.

The complete Unicode source is then encoded once.

This matters because distinct identities may map to the same DNS label.

## Non-injective mapping and allocation

DNS projection is not injective:

| Identity A | Identity B | Shared A-label |
| --- | --- | --- |
| `0x0Sky` | `0x0sky` | `0x0sky` |
| `0x0Небо` | `0x0небо` | `xn--0x0-dddt1cj` |

A label must never be reverse-computed to identify a Bond.

The authoritative rule is:

1. derive the candidate A-label in Core;
2. atomically claim that label in persistence inside the identity-registration transaction;
3. the first successful claim owns it;
4. a later collision requires a distinguishing suffix and another atomic claim.

A check-then-insert flow is forbidden because it races.

## Compatibility vectors

Representative normative results:

| Input | Result |
| --- | --- |
| `0xda-sha` | `0xda-sha` |
| `0xdA-Sha` | `0xda-sha` |
| `0x0Sky` | `0x0sky` |
| `0x0sky` | `0x0sky` |
| `0x0небо` | `xn--0x0-dddt1cj` |
| `0x0Небо` | `xn--0x0-dddt1cj` |
| `0xdпривіт` | `xn--0xd-hdd3a5bhs3p` |
| `0x0café` | `xn--0x0caf-gva` |
| `0x0日本` | `xn--0x0-v08fl0d` |
| `0x0straße` | `xn--0x0strae-wya` |
| `0x0🌍` at the raw label boundary | `DisallowedScalar` |
| `0x0א` | `BidiRule` |
| `0x0ء` | `BidiRule` |

`0x0straße` must not be transitionally mapped to `0x0strasse`.

## RTL consequence of the 0x prefix

UTS-46 Bidi validation is mandatory. Bond labels begin with the ASCII `0x` namespace, so identities whose DNS label would be governed by the right-to-left Bidi rule can fail even though their Unicode letters are valid `PubDress` scalars.

That is a DNS projection consequence, not a reason to rewrite or silently reject the identity at storage time. `BidiRule` reports this explicitly.

## Confusables are a separate trust policy

UTS-46 mapping is not identity verification and not a confusable-defense policy.

Visually similar identities may remain distinct. DNS label uniqueness prevents address collisions; it does not by itself prevent impersonation. A future UTS-39 restriction, script policy, or display warning belongs in a separate display/trust contract and must not be smuggled into UTS-46 mapping.

## Binding boundary

Wasm and UniFFI remain thin translation layers. They expose canonical `PubDress` validation plus the same Core-owned label derivation/composition.

The label wire result is deliberately small and stable:

- success: `label:<a-label>`;
- failure: `error:<stable_error_code>`.

Bindings do not implement Unicode or DNS semantics themselves.

## Ownership boundary

Core must not depend on:

- a DNS zone;
- wildcard DNS or TLS;
- HTTP endpoints;
- a database or uniqueness index;
- collision retry policy;
- Web UI or host-specific behavior.

Persistence, Web, and infrastructure consume the already-decided contract downstream.
