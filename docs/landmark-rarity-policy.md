# Landmark rarity: evidence and curation policy

**Scope:** the classification of one verified map-archive landmark into a `RarityV3` tier. This is a **curation and review contract**, not an algorithm that discovers heritage status, an automatic UNESCO import, or a live XP grant. Canonical product authority is [the 0x1 Experience Orb rarity contract](https://github.com/nilx-one/0x1/blob/master/documents/product-direction/rarity-coefficient.md). The corresponding wire/runtime behavior is [orb spills](orb-spills.md).

## The classified subject

A **landmark** is a specifically identified building, monument or deliberately delimited ensemble with **one canonical, verified map-archive feature ID**. Classification belongs to this subject, **not** to its city, country, map-marker colour, tourism popularity, museum use or nearby points.

- Do not infer rarity from an OSM `historic`/`tourism` tag, an icon, a building category, a country or the number of points in a cell.
- A UNESCO **World Heritage property** may contain many buildings, components and a buffer zone. Membership in its boundary does not automatically promote every contained feature.
- A property number or component designation from UNESCO is **not** the stable archive feature ID used as `CellLandmark.id`. Verify the crosswalk; never fabricate it. A principal ensemble and its constituent buildings need distinct curatorial decisions and an overlap policy before creating multiple separately rewarded landmark IDs.

## Tier guidance

These are **evidence thresholds for human-reviewed classification**, not runtime inference or numeric scores.

| Tier | Curatorial evidence required |
| --- | --- |
| **Common** | The deterministic default for any valid but **uncurated** archive landmark. This is not a claim that its heritage value is low. |
| **Uncommon** | Specific documented local significance or a distinctive local cultural/architectural contribution beyond an ordinary map feature. |
| **Rare** | Specific, documented regional or national significance, or an unusually well-evidenced cultural or architectural distinction. |
| **Special** | Verified significance reaching beyond the ordinary national/local threshold: for example, an *individually identified* major UNESCO component or a landmark with comparable documented international significance. A buffer-zone building is not automatically Special. |
| **Exclusive** | Meets the **Special** threshold **and** has explicit evidence for an additional **landmark-specific extraordinary contribution**, such as a demonstrated architectural masterpiece, a technically singular achievement, major cross-regional influence, or a defining role in cultural history. The evidence must identify the same monument/ensemble being classified, not just its city or encompassing UNESCO property. |

**No double counting:** inscription on the UNESCO World Heritage List already requires **Outstanding Universal Value (OUV)**. These are not two independent accolades whose sum elevates an ordinary site to Exclusive. Multiple criteria (i)–(x) describe the **property**, and a criterion applied to the property does not automatically apply to every building inside it. An individually discussed masterpiece/unique contribution may be evidenced in the same official dossier, but must be stated **about the classified subject**. A UNESCO listing is neither mandatory for Exclusive nor by itself sufficient for Exclusive.

**No geography, medium or usage bias:** no quota per city or country; the same threshold applies in Kyiv, Saint Petersburg, Odesa, Paris, Rome and beyond. An architecturally exceptional palace that houses a museum is assessed as a **palace/ensemble**, not marked down for functioning as a museum. Likewise, the name, popularity, collection size or economic value of a museum is insufficient for monument rarity.

## Evidence and decision record

Before adding a non-Common `(archive_feature_id, rarity)` to the registry, a review record MUST document:

1. **Subject:** stable archive feature ID, archive/version source, canonical landmark name, geometry/position, and whether the point is a single monument or a delimited ensemble.
2. **Recognition:** authoritative source URL and exact protection/recognition **scope** (property, serial component, specific landmark, national designation, etc.). Distinguish a protected property from its buffer zone.
3. **Individual significance:** site-specific evidence for the proposed tier and the extraordinary contribution needed for Exclusive; identify the independently meaningful claim rather than counting OUV twice.
4. **Conflict/overlap:** overlapping ensemble and child records, duplicate IDs, ambiguous identities or archive drift; explicitly choose one canonical subject and leave unresolved cases inactive.
5. **Decision:** proposed tier, accepted/rejected/deferred outcome and rationale, product reviewer/date and referenced source revision. Reclassifications require a new version; never mutate a published one.

For an absent or unsupported record, `landmark_rarity(registry, id)` stays **Common** for valid IDs or refuses invalid IDs/versions; do not replace missing evidence with speculative rarity.

### Illustrative decisions and candidates

- **Approved product direction, pending archive-ID mapping:** **Saint-Sophia Cathedral and related monumental ensemble** and **Kyiv-Pechersk Lavra** are separate **Exclusive** targets. They are the principal ensembles of **one** World Heritage property, UNESCO no. 527 (1990), with separately identifiable components. Their detailed individual historical and architectural contribution is described by UNESCO. Do not confer Exclusive on every subordinate church in either ensemble.
- **Cross-city parity examples, not automatically activated:** evaluate the **Louvre palace ensemble** (Paris, property 600) and **Winter Palace / Hermitage architectural ensemble** (Saint Petersburg, property 540) with the **same Special + individually evidenced extraordinary contribution** test. Calling an ensemble a museum must not lower its rarity. The exact qualifying archive feature and classification still require review.
- **Further candidates:** an exceptional palace/park, cathedral, theatre or engineered landmark in any city may qualify, but inclusion in a large UNESCO city property or a list of tourist highlights is **not** enough. Record a site-specific source and review.

Source references for **factual heritage membership**, not automatic classification:
- [UNESCO no. 527 — Kyiv](https://whc.unesco.org/en/list/527/) and [individual geographic components](https://whc.unesco.org/en/list/527/maps/).
- [UNESCO no. 540 — Historic Centre of Saint Petersburg](https://whc.unesco.org/en/list/540/).
- [UNESCO no. 600 — Paris, Banks of the Seine](https://whc.unesco.org/en/list/600/).
- [UNESCO no. 1703 — Historic Centre of Odesa](https://whc.unesco.org/en/list/1703/).
- [UNESCO selection criteria and Outstanding Universal Value](https://whc.unesco.org/en/criteria/).

## Runtime and rollout guard

This document changes **no** Rust enum, registry data, award, claim or rendered marker. Currently `LANDMARK_RARITY_REGISTRY_VERSION = 1` has `landmarks: &[]`, so all valid landmarks are Common at runtime, **even if the product has approved Exclusive targets**.

When reviewed archive IDs become available, append **new immutable registry version(s)** in `crates/nilxone-contracts/src/landmark_rarity.rs`; preserve every previously published version's landmark assignments, XP table, rarity roll and hash domain. Validate stable IDs, canonical subjects, conflicting duplicate positions, replay/golden fixtures and old-ID XP stability. `rated_orb_award(id)` is only a deterministic calculation: Identity still must authenticate the Bond/cell key and archive feature, idempotently commit earned XP, and coordinate host rollout. Neither classified heritage nor touching a location is an Interaction, BondChain fact, Relationship or verified physical presence.

---

© 2026 aiaiaiai · aiaiaiai.org
