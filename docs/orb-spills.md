# Orb spills

When a fog cell opens, each find in it spills a short trail of **orbs** that
leads to the find. Core owns every rule about them; a host only draws what
Core answers, reports where its Bond and Avaia are, and asks its claiming
service who was first.

| Rule | Value |
| --- | --- |
| Orbs per find | 5 to 30, `orb_count(artifact_id)`, FNV-1a of the id |
| Clumps | 3 to 6 orbs each, evenly from the trail's start to 88% of the way to the find |
| Scatter | up to 3 m from the clump's middle on each axis |
| Shortest trail | 30 m: a start closer to the find is moved back along the same line |
| Lifetime | 30 minutes from the spill (`ORB_LIFETIME_MS`) |
| Reach | strictly closer than 15 m (`ORB_PICKUP_METERS`), by `distance_meters` |
| Pays | 10 experience (`ORB_EXPERIENCE`) to whoever picks it up, Avaia or Bond |
| Landing | orb `i` lands `70 × (i + 1)` ms after the host first drew the spill; the find's ring after the last |

Everything is integer arithmetic over E7 coordinates, so every runtime lays
the same orb in the same place.

## `orb_world(world, now_ms)`

Core keeps no state. The host hands in what it knows, as JSON (decimal
strings for times and coordinates):

```json
{
  "spills": [
    {
      "artifact_id": "art:seg:312346:298243:e2908:1:0",
      "from": { "longitude_e7": "304461000", "latitude_e7": "504650000" },
      "to": { "longitude_e7": "304469000", "latitude_e7": "504655000" },
      "appeared_at": "1000",
      "expires_at": "1801000",
      "count": 20,
      "taken": [1]
    }
  ],
  "picked": ["orb:art:seg:312346:298243:e2908:1:0:2"],
  "bond": null,
  "avaia": { "longitude_e7": "304469000", "latitude_e7": "504655000" }
}
```

- `from` is where the trail starts (the middle of the cell the find lies in),
  `to` where the find lies; `count` and `taken` are the claiming service's
  answer; `picked` what this device already picked up.
- `bond` is the Bond's own observation, given only while it may pick up; a
  declared point is never given. `avaia` is the Avaia's body while it walks.

Core answers `{"ok":true,"view":{"orbs":[…],"bond_reach":[…],"avaia_reach":[…],"next_expiry":"…"}}`:
each drawn orb (`kind` `orb`, or `goal` for the find) with its point and
`lands_at`, the orb ids each of them reaches now, nearest first, and when the
soonest live spill is gone. A spill that is gone, an orb taken or picked, and
a spill whose `count` is not its find's are not drawn.

## Claims

Who picked an orb up first is not Core's to decide: the host's service claims
it, first come first served, and answers a later pick-up as taken ("crap!").
An orb is a pick-up intent and presentation, never a `BondChain` interaction,
a Relationship, or proof of presence.
