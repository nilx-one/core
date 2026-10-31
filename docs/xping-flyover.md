<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# xPing flyover

Status: v0.1 product-behavior contract  
Owner: Web (presentation)

The flyover is an animated view of the world: xPing flies from where the Bond
and its Avaia stand to a business station, along a route drawn on the map.
It is presentation only. Core owns no executable part of it; this document
fixes what starts it, where it goes and what it may never do, so every client
does the same thing.

## What starts it

- In Settings, buying an Exclusive or Special subscription opens that
  subscription's cutscene. The flyover starts when the person presses
  **Agreed** in that cutscene, and at no other time but the one return
  below. No pin, station tap, link or other entry starts it.
- Agreed starts the flight. It is not evidence that a purchase completed.
  What a subscription is, what it costs and what committing one records
  belong to the subscription's own contract, which is not defined yet. The
  flyover never charges, commits, grants or records anything.

## Where it goes

- **A** is where the Bond and its Avaia actually stand, as the world already
  shows them. Without a known position there is no flight.
- **B** is the business station the subscription concerns, by its
  [`StationPubDress`](business-station.md), for example
  `0xda-market-zoloti-vorota`. Where a station stands is persistence's.
- The route from A to B is found once, when the flight begins, on the light
  road graph: the same pedestrian graph an Avaia walks, read from road tiles
  at one zoom. Without a path on it, the route is the straight line from A to
  B. The route is drawn as a dash-dot line for the whole flight and cleared
  afterwards.

## How xPing flies

- xPing keeps to the route at a steady speed, like a runner game. Its height
  follows the built-up ground beneath it, higher over dense blocks, and
  clears the buildings ahead. A building too tall to clear is passed by
  stepping left or right, after which xPing returns to the route.
- **At the wheel**, a person may step xPing left or right: a swipe, the arrow
  keys or `A`/`D`, or the on-screen buttons. The route pulls it back once the
  person lets go. Steering never leaves a narrow corridor around the route:
  it only goes around buildings. Without steering, avoidance is automatic.
- xPing at the wheel offers the Follow camera alone. The camera chases it
  from behind and above; the Bond and its Avaia are in view at the start, and
  the route stays in view throughout.
- The person may leave the flight at any moment. At B, xPing hovers by the
  station, the route is cleared and the camera returns to where it was.
- With reduced motion there is no flight: the route and both ends are shown
  in one still view until the person leaves it.

## Return from Market

At 0xda-market's station xPing parks and the Dock opens Market. A host that
hands the Bond over to Market's own page leaves the tab, so the flight does
not end where it began; it may finish on the way back instead:

- Before it leaves, the tab keeps the route it flew out on. That route is
  presentation only: no token, balance or purchase authority is kept.
- Market sends the tab back to a 0x1 host page naming its visit and an
  order. The address is never payment proof. The host asks Identity, which
  confirms only a Special purchase that Market fulfilled and Identity
  committed for this visit's Bond.
- Only a confirmed purchase flies xPing home: from the station back to where
  the Bond and its Avaia stand, once, when the flyover can fly and no other
  flight is under way. It flies only the way back; it docks nowhere, carries
  no loot and cannot be forced. With reduced motion there is no flight.
- A kept route is flown only if both ends are valid coordinates and it leads
  to one of 0xda-market's stations
  ([`market_return_trip`](../crates/nilxone-contracts/src/market_return.rs));
  any other is dropped.
- An order Identity could not confirm yet keeps the route and stays
  identifiable for a day (`MARKET_RETURN_PENDING_MS`). The tab asks again on
  each of its page loads and, while a page is open, after each of
  `MARKET_RETURN_RECHECK_MS`. After the day the order is let go and the route
  stays. A malformed return is never asked about. The client never confirms a
  purchase itself.

## Boundary

- No body moves. The Bond and its Avaia stay where they stand; the flight
  writes no presence, `BondChain` fact, Interaction, inventory or
  progression.
- xPing stays a system prop. Flying it is not an identity grant and gives it
  no authority.
