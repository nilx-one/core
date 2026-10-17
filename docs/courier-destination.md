<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# Courier destination

Status: v1 selector contract (`COURIER_DESTINATION_VERSION` 1)  
Owner: Core

Where a courier such as xPing goes: the nearest of the BusinessPoints a host
offers. It is the selector the 0x1
[xPing courier direction](https://github.com/nilx-one/0x1/blob/master/documents/product-direction/xping-courier.md)
names; the [flyover](xping-flyover.md) then flies there.

## What it owns, and what it does not

- It chooses one point from the candidates the host made eligible in its
  context, and answers that point's id.
- It owns no tier, flight time, estimated arrival, distance or route. The
  flyover owns the route and its actual progress.
- Choosing a point never proves a subscription, a payment, an arrival or
  presence, and never makes an unverified point verified.
- A point id is opaque here: its encoding belongs to the
  [BusinessPoint](business-point.md) contract. The selector only requires it
  to be 1–128 Unicode scalars without whitespace or controls, and unique in the
  request.

## `courier_destination(request)`

```json
{
  "origin": { "longitude_e7": "305130000", "latitude_e7": "504490000" },
  "candidates": [
    {
      "point_id": "0xda-market-zoloti-vorota",
      "coordinate": { "longitude_e7": "305131000", "latitude_e7": "504491000" }
    }
  ]
}
```

Answers `{"ok":true,"point_id":"0xda-market-zoloti-vorota"}`.

| Rule | Value |
| --- | --- |
| Distance | equirectangular integer approximation in whole centimetres (as `distance_meters`, unrounded); longitude wraps across the dateline |
| Tie | equally near points choose the smallest `point_id` by code point |
| Order | the answer never depends on the order of `candidates` |
| Candidates | 1 to 256 (`MAX_COURIER_CANDIDATES`), one owner's `UInt8` range |
| Unknown fields | rejected, so a tier or flight time is never silently accepted |

## Failures

| `error` | When |
| --- | --- |
| `invalid` | the request does not parse, has unknown fields or an out-of-range coordinate |
| `no_candidates` | `candidates` is empty |
| `too_many_candidates` | more than 256 candidates |
| `point_id` | a point id is empty, longer than 128 scalars, or holds whitespace or a control |
| `duplicate_point_id` | two candidates share a point id |

A failure never picks a point. A host without this export keeps its own
explicitly labelled preview; it never treats a failure as that fallback.
