# Business membership

A membership relates one human Bond to one Business Bond. It is not a global
`BondAccessRole`, subscription, provider affiliation, completed Interaction or
BondChain. Membership creation records an administrative fact only.

Wire roles: `member`, `special_member`, `maintainer`, `administrator`, `co_owner`,
`owner`. Special member is independent of the Special subscription. Administrator
is scoped to this business and never grants system administration. No ordinal
comparison of these roles grants permissions. Every operation must define its
own permission contract before exposing it.

## Creation

An authenticated active human requests a fresh business address. Identity must
atomically allocate the address, mark the business, and persist the requesting
human's membership. The creator is taken from authentication, never a request
body. `creator_role` accepts Owner (default) or Maintainer only. Maintainer means
exactly that role, without fabricating an owner; ownership assignment/transfer
requires a separately authorized lifecycle. A taken address cannot be claimed,
including an existing business. Failed creation leaves no partial identity.

Identity owns persistence and authorization; Core owns role values. Web/iOS show
the server result. Memberships follow identity renames through foreign keys.
Listing one's memberships does not expose another person's memberships.

## Existing businesses

An operator may bootstrap an existing business's first membership after checking
both exact identities. This is a separate, explicit operation; normal creation
never claims it. For the requested 0xda-market assignment, verify `0x0sky` and
`0xda-market` against the operator's trusted account records before assigning
Owner. No name-based startup migration, GitHub affiliation inference, global
admin fallback or public claiming endpoint is permitted. Repeated bootstrap must
not overwrite existing membership or transfer ownership.

## Delivery slices

1. Web map: move Motherland preview arrival clear of monument terraces.
2. Core: closed six-role values and restricted creator role (this change).
3. Identity: schema, atomic creation, authenticated own-membership listing,
   explicit first-member operator bootstrap, rollback/authorization tests.
4. Web: organizations list and create form, Owner/Maintainer choice, error and
   pending states; use the normal Identity transport in every host.

Invitations, delegated role changes, removal, ownership transfer and business
point mutation are separate follow-ups. Their permission matrix must be defined
before routes are enabled; no permissions are inferred from role names here.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
