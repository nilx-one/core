# Five-tier rarity v3 scale (staged, not live)

Canonical target: [0x1 product-direction contract](https://github.com/nilx-one/0x1/blob/master/documents/product-direction/five-tier-rarity-economy.md). Proposed specification review: [0x1 PR #73](https://github.com/nilx-one/0x1/pull/73).

**This module is inert.** It does not change the current artifact roll, public rarity wire, inventory valuation, paid pickup, or rewards. Existing `FindTier`/`Rarity` still describe the **legacy** six-tier catalog.

| Target tier | Code | Price factor | Base find XP |
| ---: | --- | ---: | ---: |
| 1 | `common` | ×1 | 10 |
| 2 | `uncommon` | ×2 | 30 |
| 3 | `rare` | ×4 | 90 |
| 4 | `special` | ×8 | 270 |
| 5 | `exclusive` | ×16 | 800 |

`RarityV3` holds the approved target numbers and validates tier/code input independently of legacy `Rarity`. Base XP approximates ×3 between successive tiers but uses published integer rewards (810 → 800). `find_sale_price` derives one sale price from a **shared, positive** Common price (and checks overflow), so names, titles and creator identities cannot set arbitrary sale prices. The Common Seeds base is not yet fixed, so no new market price is emitted automatically.

The multiplier ×2 for prices must also guide future Special/Exclusive subscription prices, with a separate term discount and denomination-aware commercial rounding. **Market** owns payment quotes, not this item contract. Inactive 0xda-market SKUs and old issued prices are not overwritten.

## Migration gate

Target tiers and old numeric tiers cannot safely share an unversioned protocol:

- legacy `1–3 = common`, `4 = uncommon`, `5 = rare`, `6 = legendary`;
- target `1 = common`, `2 = uncommon`, `3 = rare`, `4 = special`, `5 = exclusive`.

A future PR must bind every persisted, transmitted and rewarded find to its originating catalog/roll version and preserve legacy receipts, XP and inventory; then supply target item pools, reviewed five-tier drop weights, deterministic golden vectors, service entitlement checks, Web preference migration and synchronized rollout. No historical `legendary` item may be automatically treated as paid Special or Exclusive.

Experience Orbs reuse these names and multipliers for their own, separate rules ([orb spills](orb-spills.md), v4): an orb's chance is `1 / multiplier` under its landmark's rarity and it pays `orb_experience` (10 / 20 / 40 / 80 / 160), never `base_find_xp`. Orbs are not gated by a subscription.

`RarityV3` does **not** itself authorize collection or grant paid access. The actual entitlement comes from the identity service, and no find event writes BondChain/Relationship truth.

---

© 2026 aiaiaiai · aiaiaiai.org
