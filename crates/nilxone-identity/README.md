# Shared Identity authority

`nilxone-identity` is the server-side implementation shared by every 0x1
client. It was extracted from `nilx-one/web` at
`29b3653fc3b9cb110242835513cb289e12f65a67`, including its tests and SQL migrations.
Existing source notices are preserved.

This package owns Identity persistence, active-session checks, provider binding,
credentials, location persistence, business membership, progression, xPing
spending, seeds wallets, subscription settlement, purchase visits and receipts.
`market_issuer` is the server adapter to Market's order API. It is shared across
clients, but is not part of the portable semantic kernel.

Clients never link this server package into Wasm or Swift. They consume the
portable contracts/bindings and authenticated server projections. Database
access and issuer secrets stay on the server. Running Core locally does not
grant economic authority.

## Client and payment boundary

- Core owns common validation, entitlement rules and atomic settlement.
- Web owns browser/Telegram/Discord request authentication, HTTP routes, OAuth,
  cookies, host SDKs, presentation, navigation and the process composition root.
- A future native client supplies its own platform/payment adapter. A successful
  UI callback is not a purchase receipt and cannot grant an entitlement.
- Market owns offers, prices, checkout and orders, not the Identity wallet.
- Provider-specific payment verification must produce authenticated evidence
  before the common authority applies any entitlement. This extraction adds no
  new payment method or entitlement-grant API.

## Persistence and rollout

The consuming process continues to open the same `DATABASE_URL` and owns the
same volume. There is no second database, data copy or balance migration.
Migration bytes and ordering are preserved. Legacy provider-specific tables
remain in the repository migration unit so existing databases remain compatible;
Telegram delivery and UI policy stay in the host.

The worker still defaults off. This source extraction does not authorize deploy,
wallet funding, checkout activation or catalog activation. Recovery hardening and
real HTTP E2E remain separate gates; moving code does not claim to fix them.

Host adapters may obtain a partner-specific subject digest, never the underlying
private subject. `market_pairwise_subject` is server-only and must not be exposed
as an unauthenticated browser route.

## Verification

Run `cargo test -p nilxone-identity` and the consuming Identity service tests.
Core workspace CI also verifies the portable bindings remain independent of this
server package. Transport tests remain in Web; repository/issuer tests move here.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
