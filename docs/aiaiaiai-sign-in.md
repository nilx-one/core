<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# Sign-in through aiaiaiai Identity

Status: v1 client contract  
Owner: Core (client); the current issuer implementation is in `nilx-one/web`.
The shared AIS service is being separated into `aiaiaiai-org/4x-ais`.

The new product service uses [`ais-public-info.md`](ais-public-info.md): a
separate 0x1 store keyed only by verified `(issuer, subject)`. Its
`PublicInfoService` calls `SignInFlow::verify_callback`, then attaches product
state without the legacy address-based link below. Existing hosts still use
the original `finish` path until their adapter is explicitly moved.

aiaiaiai Identity (`auth.aiaiaiai.org`) is the authority for who a person is.
0x1 is one of its clients, and Core is how 0x1 talks to it: over OpenID
Connect, authorization code with PKCE, and nothing else. The issuer's
contract is
[`docs/unified-auth.md`](https://github.com/nilx-one/web/blob/master/docs/unified-auth.md)
in `nilx-one/web`.

## What the host does: forward, nothing more

`SignInFlow` is the whole browser flow. The 0x1 host routes pass the request
in and apply what comes back; they decide nothing:

| Host route        | Calls                                              | Applies                                                                                                             |
| ----------------- | -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `…/start?return=` | `start(return, now)`                               | redirect to `location`, `Set-Cookie: set_cookie`                                                                    |
| `…/callback`      | `finish(repository, Cookie header, Callback, now)` | `clear_cookie`; on `SignedIn` the ordinary 0x1 session and `return_path`; on a failure `/?auth_error=` + `as_str()` |

Core owns the transaction cookie (`__Host-0x1_aiaiaiai`: HMAC-signed under
the native authentication secret with its own domain, Secure, HttpOnly,
SameSite=Lax, fifteen minutes), the `state` check (constant time, before
anything else, so even a decline with a foreign `state` is `Invalid`), the
return-path rule (same-origin paths only) and the failure codes:

| `SignInFailure`   | Code                           | Meaning                                                      |
| ----------------- | ------------------------------ | ------------------------------------------------------------ |
| `Expired`         | `aiaiaiai_sign_in_expired`     | no transaction, or an old one                                |
| `Invalid`         | `aiaiaiai_sign_in_invalid`     | tampered transaction, foreign `state`, refused code or token |
| `Cancelled`       | `aiaiaiai_sign_in_cancelled`   | the person or issuer declined                                |
| `Unavailable`     | `aiaiaiai_sign_in_unavailable` | issuer or storage did not answer; retry                      |
| `AccountUnknown`  | `aiaiaiai_account_unknown`     | no Bond for this Identity account                            |
| `AccountConflict` | `aiaiaiai_account_conflict`    | the Bond is linked to another account                        |

The 0x1 session cookie itself stays the host's: it is the one every 0x1
route already reads.

## The client: `aiaiaiai_sign_in`

```text
host: begin() ──▶ person signs in on auth.aiaiaiai.org ──▶ host: complete(code)
        │ keeps SignInTransaction in a signed cookie        │ checks state first
        ▼                                                    ▼
  authorization URL                            POST /token, GET JWKS, verify
```

- `SignInClient::begin` returns the authorization URL and a
  `SignInTransaction` (`state`, `nonce`, PKCE verifier). The host keeps the
  transaction until the person returns and compares `state` itself.
- `SignInClient::complete` exchanges the code at `/token`, fetches the issuer's
  keys and verifies the `id_token`. A client built with `SignInClientConfig::new`
  holds a secret and authenticates with it (Basic, form-encoded). One built with
  `SignInClientConfig::public` holds none and names itself with `client_id` in
  the form; it must be registered in AIS without a secret, and the code is
  bound to it by the PKCE verifier and the exact redirect URI. Redirects are
  never followed, so a secret only goes where the host configured it.
- `verify_id_token` accepts **only EdDSA** (Ed25519). It refuses any other
  `alg` before it looks at a key. It then requires `iss`, `aud` (with `azp`
  when there are several audiences), `exp`, `iat`, the transaction's `nonce`,
  and an `auth_time` at most ten minutes old. Clock leeway is one minute.

Only `SignInError::Unavailable` is worth a retry. Every other error ends the
attempt, and the person starts again.

## The Bond behind a subject: `resolve_aiaiaiai_account`

`sub` is Identity's **pairwise** subject for the 0x1 client: stable for 0x1,
useless to every other product, and never Identity's private identifier. 0x1
keeps its own `aiaiaiai_accounts` table (`issuer`, `subject`) → Bond:

| Situation                                               | Outcome                                |
| ------------------------------------------------------- | -------------------------------------- |
| subject already linked                                  | `Linked` (follows renames)             |
| new subject; the token's `pub_dress` names a human Bond | `Linked { newly_linked: true }`        |
| new subject; that Bond is linked to another subject     | `BondLinkedElsewhere`, needs the owner |
| new subject; no `pub_dress`, or it names no human Bond  | `Unknown`                              |

The address is used only on first sight, because it is public and mutable.
After that the subject is the key. Linking runs in one immediate
transaction, so two concurrent first sign-ins cannot link one Bond twice.

## The issuer's side: `client_pairwise_subject`

While the issuer still runs on the Identity database it grew out of, Core
also gives it the pairwise digest: HMAC of the client id and the private
subject, under the native authentication secret with its own domain. That is
the only issuer-side piece in Core, and it never returns the subject itself.
Rotating the native authentication secret rotates every client's `sub`;
that secret is server-owned and preserved across deploys for this reason.
