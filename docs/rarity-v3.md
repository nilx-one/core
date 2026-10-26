# Five-tier rarity v3 scale (staged, not live)

Canonical target: [0x1 product-direction contract](https://github.com/nilx-one/0x1/blob/master/documents/product-direction/five-tier-rarity-economy.md). Proposed specification review: [0x1 PR #73](https://github.com/nilx-one/0x1/pull/73).

**This module is inert.** It does not change the current artifact roll, public rarity wire, inventory valuation, paid pickup, or rewards. Existing `FindTier`/`Rarity` still describe the **legacy** six-tier catalog.

| Target tier | Code | Target find factor | Target find resale (seeds, ₴€£) | Base find XP |
| ---: | --- | ---: | ---: | ---: |
| 1 | `common` | ×1 | 10 | 10 |
| 2 | `uncommon` | ×4 | 40 | 30 |
| 3 | `rare` | ×16 | 160 | 90 |
| 4 | `special` | ×64 | 640 | 270 |
| 5 | `exclusive` | ×256 | 2,560 | 800 |

`RarityV3` holds the approved target numbers and validates tier/code input independently of legacy `Rarity`. Base XP approximates ×3 between successive tiers but uses published integer rewards (810 → 800). `find_sale_price` derives one sale price from a **shared, positive** Common price (and checks overflow), so names, titles and creator identities cannot set arbitrary sale prices. The Common find resale base is approved at **`FIND_COMMON_BASE_SEEDS = 10`** (10 ₴€£ in in-game `seeds`, not a fiat FX rate), with **×4 per successive tier**: **10 / 40 / 160 / 640 / 2,560 seeds** at tiers 1–5. This is the staged v3 resale contract, **not** a seeds pickup payout. **Implementation gap (not activated by this documentation PR):** `RarityV3::price_multiplier()` and `find_sale_price(common_base_seeds)` still implement the previous ×1/×2/×4/×8/×16 find-price factors; their test fixtures also retain the old table. A separate code/test change must implement ×1/×4/×16/×64/×256 and the approved 10-seeds common anchor before rollout. The Core API presently accepts a caller-supplied common base; do not treat it as a server-enforced economic policy. No existing rolls, claims, inventory or market pricing are changed here.

Subscription term pricing remains a separate contract: **Exclusive = 2 × Special** for the same duration and payment denomination before commercial rounding, **not** ×4 because of find resale. **Market** owns payment quotes, not this item contract. Inactive 0xda-market SKUs and old issued prices are not overwritten.

## Migration gate

Target tiers and old numeric tiers cannot safely share an unversioned protocol:

- legacy `1–3 = common`, `4 = uncommon`, `5 = rare`, `6 = legendary`;
- target `1 = common`, `2 = uncommon`, `3 = rare`, `4 = special`, `5 = exclusive`.

A future PR must bind every persisted, transmitted and rewarded find to its originating catalog/roll version and preserve legacy receipts, XP and inventory; then supply target item pools, reviewed five-tier drop weights, deterministic golden vectors, service entitlement checks, Web preference migration and synchronized rollout. No historical `legendary` item may be automatically treated as paid Special or Exclusive.

Experience Orbs reuse these rarity names but **not** the updated find-resale factors; their already-published registry v1 freezes independent orb coefficients ×1/×2/×4/×8/×16, chances and XP payouts for their own, separate rules ([orb spills](orb-spills.md), v4): an orb's chance is `1 / multiplier` under its landmark's rarity and it pays `orb_experience(registry, rarity)` (registry 1: 10 / 20 / 40 / 80 / 160), frozen with the landmark rarity registry, never `base_find_xp`. Orbs are not gated by a subscription. `landmark_rarity.rs` has a test that currently derives published orb XP from `RarityV3::price_multiplier()`; decouple this test when changing find-sale factors, because historical registry XP **must remain immutable**. An eligible first discovery/description of an Exclusive landmark is intended to award 800 XP via its own authorized, idempotent description claim (exact trigger and verification to be specified before activation). Mere map-pin visibility and orb pickup do not grant that description reward.

`RarityV3` does **not** itself authorize collection or grant paid access. The actual entitlement comes from the identity service, and no find event writes BondChain/Relationship truth.

---

© 2026 aiaiaiai · aiaiaiai.org
