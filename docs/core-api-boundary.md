<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# Core as the engine and the API

Status: owner decision, 2026-11-10. This records the target and the order of work.
It moves no code, changes no contract and activates nothing.

## Decision

Anything a second interface would need lives in Core. A command-line client, a
native iOS app, Web and any later interface are **adapters** of one engine: each
supplies a platform (screen, input, storage, network, sensors) and none of them
owns a rule, a number or a wire shape that another would have to copy.

The test for any piece of code is: *could a Rust or Swift client use this without
reading the Web repository?* If not, it belongs in Core.

## Three layers

| Layer | Owns | Lives in |
| --- | --- | --- |
| **Core** | Domain rules and constants. The **wire contract**: routes, request and response types, error codes, protocol version. A client library over a `Transport` it does not implement. Host **ports** as traits. | `nilx-one/core` |
| **Adapter** | One platform's implementation of the ports and its presentation: UI, map rendering, device sensors, local storage, audio, the transport. | Web, iOS, CLI, … |
| **Server host** | HTTP and process composition: cookies, CSRF, rate limits, request authentication, deployment. It calls Core and defines no rule. | where the owner places it (today `nilx-one/web`) |

`aiaiaiai-org/4x-ais` keeps shared identity, native credentials, providers and the
OIDC issuer ([AIS identity](ais-public-info.md)). Moving those is not part of this work.

## What this means in practice

1. **One wire contract.** A route, its types and its errors are defined once, in
   Core, and used by the server host and by every client. Today the same API is
   written twice outside Core: as Rust types in Web's identity service and as
   hand-written TypeScript parsers.
2. **No rule in an adapter.** Policy numbers (lifetimes, radii, windows, limits)
   and the decisions built on them are Core's. An adapter passes facts in and
   carries commands out.
3. **Ports are traits.** Storage, geolocation, clock, audio, notifications and
   secure storage are described in Core as the capabilities a host provides. An
   adapter implements them and Core never reaches past them.
4. **Parity before deletion.** A rule leaves an adapter only after Core produces
   the same results on fixtures captured from the old implementation. The old code
   is removed in the same change that switches to Core.
5. **Presentation stays out.** What to show is a rule (templates, closed menus,
   policies). How it is drawn or spoken is an adapter's.

## Order of work

Each step is its own pull request, merged before the next starts.

1. This decision.
2. A wire-contract crate for the purchase path: finds attestation, selling,
   crafts, the seeds wallet and the Market visit. The server host switches to its
   types and a shared fixture corpus checks both Rust and TypeScript.
3. A client library over a `Transport` trait. Web supplies `fetch`; the
   hand-written TypeScript client shrinks accordingly.
4. The remaining routes, by area.
5. Domain logic still written in TypeScript, one package at a time: route graph
   (in Core: [walking graph](walk-graph.md)),
   geometry, narration templates and contracts, avatar appearance, state
   placement, identity registration.
6. Policy numbers and decisions inside the server host, starting with spoken
   lines and partner account rules.
7. Host ports as traits.

## Open decisions

- **Local records.** [Landmark notebook](landmark-notebook.md) and
  [presence journal](presence-journal.md) say Web owns persistence and lifecycle
  "until a later normative decision". A second client makes that decision due:
  either Core defines the record shape and its validation, or each client keeps
  its own and they do not synchronise.
- **Server host location.** Keeping HTTP composition in Web matches the current
  ownership table. Moving it to a Core crate would leave Web with no server code.
  The wire contract belongs in Core either way.
- **Wasm cost.** Moving logic behind the Wasm boundary changes bundle size and
  makes calls across it asynchronous. Each move measures both before it lands.

## Out of scope

No new database, secret or deployment variable. No change to what any host runs
today. No move of AIS responsibilities.
