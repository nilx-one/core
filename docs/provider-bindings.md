<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# Provider bindings

Status: v1 decision contract  
Owner: Core

Which Telegram, Discord or GitHub account is connected to a human Bond, and
the rules for connecting, disconnecting and looking one up. The rules are the
0x1
[provider-binding rules](https://github.com/nilx-one/0x1/blob/master/documents/04-identity.md#provider-binding-rules);
this module is their executable form.

## Core decides, the host stores

Core reads no storage. A host (the identity server) loads a **snapshot** of
what storage holds, asks Core for a **decision**, and carries it out in the
same transaction it read the snapshot in. The decision says what to store or
what to show. The host does not re-derive it.

```text
host: read snapshot ──▶ Core: decide ──▶ host: store / show what Core said
```

## Connecting: `decide_link(bond, account, snapshot)`

The snapshot holds whether the Bond exists, which Bond already owns the
account (if any), and which subject the Bond already holds for that provider
type (if any).

| Decision | Code | Meaning |
| --- | --- | --- |
| `Insert` | `linked` | store the binding |
| `AlreadyLinked` | `already_linked` | this exact binding exists |
| `ProviderAlreadyLinked` | `provider_already_linked` | the account belongs to another Bond (rule 3) |
| `ProviderTypeAlreadyLinked` | `provider_type_already_linked` | the Bond already holds another account of this type (rule 2) |
| `IdentityMissing` | `identity_missing` | there is no human Bond to link |

Rule 3 is checked before rule 2. The two refusals carry the codes the sign-in
window turns into "This Telegram account is already linked to another Bond"
and "This Bond account is already linked to another Telegram account".

## Disconnecting: `decide_self_disconnect(snapshot)`

| Decision | Code | Meaning |
| --- | --- | --- |
| `Delete` | `disconnected` | remove the binding |
| `NotLinked` | `not_linked` | the Bond holds no account of this provider |
| `SoleAccessPath` | `sole_access_path` | refused: no other provider and no active password, so the Bond would be unreachable |

## Looking up: `provider_lookup_view(account, owner)`

A partner, for example a business in the 0x1 world, asks which Bond an
account belongs to.

- `Linked { pub_dress }`: show the Bond's public handle. Nothing else is
  disclosed, and never an internal identifier.
- `SignInRequired { provider }`: the account is not bound. Ask the person to
  sign in with that provider to connect it.

When a person tried to connect the account and Core answered
`ProviderTypeAlreadyLinked` (the Bond they signed in to already holds another
account of that provider), Identity remembers that refusal for the account
until it is linked anywhere (`ProviderLinkRepository::recent_link_refusal`).
A partner that sent the person to sign in can then say why the account is
still unbound ("this Bond already has another Telegram account") instead of
asking again. The refusal names no Bond.

A lookup that cannot reach storage is not a lookup result. The host reports it
as unavailable and never as `SignInRequired`, so a person is not sent to link
an account that is already linked.

## Values

- `ProviderType`: `telegram`, `discord`, `github`. Unknown names fail.
- `ProviderSubject`: the provider's opaque account id, 1–256 bytes, with no
  whitespace or control characters, kept exactly as issued.
