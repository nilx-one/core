<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# Client activation: moving the active client

Status: Core-owned rules, not activated in any host. Wire contract in `nilxone-api::activation`, authority in `nilxone-identity` (`PublicInfoRepository`). Owner decisions of 2026-11-12.

The source contract is 0x1 `documents/15-devices-and-recovery.md` (Single Active Client) and `documents/15-telegram-browser-session-handoff.md`, as amended for those decisions in [nilx-one/0x1#86](https://github.com/nilx-one/0x1/pull/86).

## The rule

A Bond's signed-in clients are product sessions. A session is **active** or **inactive** and nothing else. At most one session of a Bond is active, and only the active one acts for the Bond.

- Signing in makes a session active only when the Bond has no live active session. Otherwise it starts inactive. A password, an email or any other sign-in never takes activity away from another client.
- A signed-in inactive session becomes active in one way: it asks, the active client agrees inside its own session, the active client uploads its data, the asking client fetches all of it and proves that, and only then Core switches.
- Agreeing needs no second factor: no password again and no passkey.
- There is no objection window, no other confirmation route and no recovery here. Recovery exists only through a BondChain (`REC-REQ`).
- Core owns all of it: the sessions, the request, the epoch, the data in transit and the switch. The aiaiaiai Identity Service authenticates a person and holds none of this. A host passes calls through and a client draws the answer.

## Sessions

`PublicInfoRepository::open_session(owner, token_hash, client_label, supersede_token_hash, now, expires_at)` returns whether the session is the active client.

- `client_label` names the device and host to the Bond's other clients, such as `iPhone · Telegram`: at most 80 bytes, no control characters. The host derives it; Core only stores it.
- `supersede_token_hash` is the session the same browser presented when it signed in again. It is closed first, in the same transaction, so signing in again on the active client keeps it active. A session of another Bond is never closed this way.
- `close_session(token_hash, now)` is sign-out. It ends the session's part in an open request: the asking client leaving cancels it, the active client leaving interrupts it. When the active client signs out, the Bond has no active client until the next sign-in.
- `restore_session` restores inactive sessions too. An inactive session identifies its owner and may read.

**The write boundary.** `publish_experience_carry` writes only for an owner restored from a session that is active, checked in the same statement that writes. Anything else is `PublicInfoError::SessionInactive`, which a host answers as `session_inactive` (HTTP 403). The result of a sign-in callback has no session and cannot write. Every later Bond write on the product store follows the same rule.

**The epoch.** `public_profiles.session_epoch` counts every change of a Bond's active client: a session that opens active, the switch, and the active client signing out. It never leaves the service. A request records the epoch it was made under, and the switch commits only while the Bond's epoch is still that one.

A store created before this change gains the columns when it is opened: each Bond's most recently opened session becomes its active one and the others stay signed in, inactive.

## A request

An inactive session asks with `request_activation`. The request binds the asking session, the session that was active at that moment, both labels and the epoch. A step by any other session, of this Bond or another, finds no request.

| Status | Meaning | Open |
| --- | --- | --- |
| `pending` | Waiting for the active client's answer. | yes |
| `approved` | The active client agreed and is uploading. | yes |
| `syncing` | The data is whole on the service; the asking client fetches it and completes. | yes |
| `completed` | The asking client is the active one. | |
| `declined` | The active client refused before agreeing. | |
| `objected` | The active client took its agreement back before the switch. | |
| `cancelled` | The asking client withdrew, or signed out. | |
| `superseded` | Another inactive client of the Bond asked. | |
| `expired` | Nobody answered in time, the asking client did not complete in time, or its session ran out. | |
| `interrupted` | The active client's side broke off: it signed out, its session ran out, the active client changed, or its upload stalled. | |

- A Bond has at most one open request. A request by another inactive session supersedes it.
- The asking session asking again while its own request is open gets that same request. A reloaded page resumes; it does not ask twice.
- When nobody is active, there is nobody to ask: `no_active_client`. The inactive session shows `sign_in_required`, and the next sign-in is active.
- There is no background sweep. Every read and every step first applies what is due on the request: a lapsed deadline, a session that is gone, an epoch that moved on.
- A second approve, decline, cancel or complete answers what the first one did.

## The sync

The active client's data travels through the service in parts.

1. **Upload.** The active client sends its data in order with `upload_activation_sync_part`. Every part is 256 KiB except the last, which carries what remains (`sync_part_len`). The first part fixes the total size. The part that makes the data whole moves the request to `syncing` and fixes its SHA-256 digest. The same part again is a retry and changes nothing; other bytes in its place are refused.
2. **Download.** The asking client fetches the parts in order with `download_activation_sync_part`: the next one, the last one again, or the first one to start over.
3. **Complete.** The asking client sends the digest of everything it fetched (`sync_digest`). Only a matching digest switches. Anything else is `sync_incomplete` and the request stays as it is.

The switch is one write transaction: compare the Bond's epoch with the request's and advance it, deactivate the previous client, activate the asking one, close the request, delete the parts. If any of it does not hold, nobody is activated.

What an interruption means depends on the side, as decided:

- **The asking side** (a reload, a lost connection): the fetch starts again from the first part. If the upload is whole, the same request serves it again, with no new request and no new approval.
- **The active side** (a decline, taking the agreement back, signing out, its session running out, a stalled upload): the request ends and its parts are deleted. The asking client must ask again, even if the upload had been whole.

### What the data is

Core does not read the bytes. It stores them only while the request is open, deletes them with every final status, and never returns them to anyone but the asking session. The store opens with `secure_delete` on, so freed pages are overwritten.

The contents are bounded by this contract, as the minimal option the specification allows:

- **In:** the active client's own transport-eligible state, the state a client keeps only on its device and that may follow a Bond between its devices: interface preferences and play records.
- **Out:** signing keys and any other key material; `bond.chain` copies and every other record that moves only under its own cryptographic lifecycle (the presence journal, the committed award and find journal); cookies, session tokens, credentials and provider material.

The second list is not a detail of this feature. 0x1 `documents/17-protocol-constants-and-open-questions.md` requires a protocol-version change for any change to plaintext boundaries, and signing authority moves only as `documents/15-devices-and-recovery.md` defines. The data is not a recovery archive. Widening it needs that decision first.

## Progress

Every open request, and a completed one, carries `progress`, the same for both clients (`HandoffProgress::project`):

| Step | `done` when |
| --- | --- |
| `confirm` | the active client agreed |
| `sync` | the asking client has been handed every part |
| `switch` | the switch happened |

Each step is `waiting`, `current` or `done`, and exactly one is `current` while the request is open. `percent` is `0..=100`: the upload fills `0..=50`, the delivery `50..=99`, and it is `100` only once the switch happened. A restarted fetch lowers it. `total_bytes` is `null` until the first part arrived; `uploaded_bytes` and `delivered_bytes` are the two counts behind the percentage. `from_label` and `to_label` on the request name the client that hands over and the one that takes over.

A client draws these values. It never derives a step or a percentage from time, animation or its own transfer.

## What a client shows

Every route answers the caller's whole `ActivationState`. `state` is one value per screen (`ClientState::project`):

| `state` | Client | Meaning |
| --- | --- | --- |
| `authenticated_active` | active | Nothing is asked of it. |
| `approval_requested` | active | Another client asks. Show approve and decline. |
| `upload_required` | active | It agreed. Upload the data now. |
| `handoff_syncing` | active | Its data is whole on the service and the other client is fetching it. It may still decline. |
| `active_elsewhere` | inactive | Another client is active. Show the request action. |
| `sign_in_required` | inactive | No client is active. Only signing in again makes one. |
| `activation_pending` | inactive | Its request waits for the active client. Show cancel. |
| `activation_approved` | inactive | The active client agreed and is uploading. |
| `activation_syncing` | inactive | Fetch the data, apply it, complete. |
| `activation_rejected` | inactive | Its last request was declined. |
| `activation_objected` | inactive | The active client agreed, then took it back. |
| `activation_cancelled` | inactive | It withdrew its last request. |
| `activation_superseded` | inactive | A newer request replaced its last one. |
| `activation_expired` | inactive | Its last request ran out of time. |
| `activation_interrupted` | inactive | The active client's side broke off its last request. |

`may_act` is true only for `authenticated_active` and `approval_requested`. The active client stops acting once it agrees, so what it uploads stays the whole truth. No state makes a client active before its sync completed.

## Routes

The host serves these paths (`nilxone_api::activation`) and applies its own session cookie, CSRF protection and rate limits. Each calls one repository method with the session's token digest.

| Method and path | Caller | Repository method |
| --- | --- | --- |
| `GET /api/v1/session/activation` | any session | `activation_state` |
| `POST /api/v1/session/activation/requests` (201) | inactive | `request_activation` |
| `POST …/requests/{id}/cancel` | asking | `cancel_activation` |
| `POST …/requests/{id}/approve` | active | `approve_activation` |
| `POST …/requests/{id}/decline` | active | `decline_activation` |
| `PUT …/requests/{id}/sync?offset=&total_bytes=`, raw bytes | active | `upload_activation_sync_part` |
| `GET …/requests/{id}/sync?offset=`, answers raw bytes | asking | `download_activation_sync_part` |
| `POST …/requests/{id}/complete`, `{"digest": …}` | asking | `complete_activation` |

A refusal is `ActivationError::Refused`; the host answers its `code()` and `http_status()` in the common error body.

| Code | HTTP | When |
| --- | --- | --- |
| `provider_authentication_required` | 401 | No live session. |
| `already_active` | 409 | The active client asked. |
| `no_active_client` | 409 | Nobody is active to ask. |
| `activation_request_limit` | 429 | The Bond opened 8 requests within the hour. |
| `activation_request_not_found` | 404 | No such request for this session in this role. |
| `activation_step_refused` | 409 | The request is not at a status this step applies to. |
| `sync_payload_too_large` | 413 | The declared size is over 16 MiB. |
| `sync_out_of_order` | 409 | The part is not the next one, is of the wrong size, or the size changed. Read the state and continue from its counts. |
| `invalid_sync_digest` | 422 | The digest is not 64 lowercase hex characters. |
| `sync_incomplete` | 409 | The digest is not that of the uploaded data. Fetch again from the first part. |

## Policy numbers

These are Core's drafts and are not confirmed product parameters.

| Constant | Value |
| --- | --- |
| `RESPOND_SECONDS` | 120: the active client's time to answer. |
| `UPLOAD_SECONDS` | 120: its time to upload the next part, after agreeing and after every part. |
| `SYNC_SECONDS` | 600: the asking client's time to fetch and complete, once the upload is whole. |
| `SYNC_MAX_BYTES` | 16 MiB. |
| `SYNC_PART_BYTES` | 256 KiB. |
| `REQUESTS_PER_HOUR` | 8 per Bond. |
| `CLIENT_LABEL_MAX_BYTES` | 80. |

## Bindings

Clients read answers through Core, in the Wasm and UniFFI bindings alike:

- `read_activation_state(ok, body)`: `{"kind":"state",…}` with the state above, `{"kind":"refused","reason":…}`, `{"kind":"authentication_required"}` or `{"kind":"unavailable"}`. A body whose `may_act` or `progress` disagrees with its own state is `unavailable`, never drawn.
- `activation_sync_part(total_bytes, offset)`: the part that starts at `offset`, `{"kind":"part","offset":…,"end":…,"last":…}` or `{"kind":"none"}`.
- `activation_sync_digest(bytes)`: the digest for `complete`.

## The earlier session model

`native_sessions.active` and `session_activation_requests` in the legacy identity store, with their objection-window status, are unchanged: the running host still serves them. The decided model has one session model with two states, and it is this one. The legacy activation converges by retirement, once legacy Bonds reach product sessions through the verified migration ([`ais-legacy-bond-migration.md`](ais-legacy-bond-migration.md), [#115](https://github.com/nilx-one/core/issues/115)); no rule here is back-ported to it. The new tables carry `product_` in their names because a host may open both stores in one file.

## Limits

- No host serves these routes yet and no client draws them.
- Core does not notify the active client. A Telegram message, if a host sends one, is a pointer to open the client and carries no decision.
- The data rests in the product store, unencrypted by Core, for as long as its request is open, at most the deadlines above. WAL frames written before a checkpoint may hold it a little longer.
- The write boundary checks that the session is active. That the active client stops acting during its own handoff is `may_act`, which the client follows; the store does not yet refuse such a write.

## Verification

`cargo test -p nilxone-api activation` pins the wire fixtures, the projection of states, steps and percentage, the part arithmetic and the refusal codes. `cargo test -p nilxone-identity public_info::activation` runs the state machine against SQLite: a whole handoff, a reload, both kinds of decline, cancel, supersede, every deadline, either session ending, strangers and guessed ids, part order and retries, the request limit, a stale epoch, the write boundary, signing in again, and a store from before this change.
