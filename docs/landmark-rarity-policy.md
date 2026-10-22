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

**Conservation and reconstruction neutrality:** restoration after a disaster, replacement of lost fabric, conservation of authentic ruins, or a protective reconstruction **never automatically lowers a historical landmark's tier**. Review (a) independently evidenced historical/architectural influence of the *original landmark* and (b) the documented survival/reconstruction of its fabric **as separate factual fields**. Do not describe a modern reconstruction as wholly original, and do not elevate a generic replica with no verified historical subject solely for resembling an old monument. Apply the same test to Notre-Dame de Paris (restored after the 2019 fire) and Kyiv's Golden Gate (authentic eleventh-century remains protected by a reconstruction pavilion erected in 1982).

## Common/Uncommon coverage: deterministic baseline, not a hand-entered census

The map contains more buildings and small cultural features than can be curated by hand. **Coverage is defined by a rule over verified eligible archive objects**, not by manually listing every residential building. The canonical subject must exist before it receives a gameplay rarity.

| Archive / normalized subject | Initial result | Promotion into Uncommon |
| --- | --- | --- |
| Valid, eligible `pois` monument, memorial, sculpture/artwork or historic landmark, with a canonical archive ID | **Common** if no reviewed registry entry | A named and uniquely identifiable object with independently sourced, **specific local significance** and approved archive-ID mapping |
| Historic or architecturally distinctive **building** explicitly admitted into the validated landmark catalog | **Common** until reviewed | Evidence of documented local architectural, civic or cultural significance for **that exact building**, not just an evocative name or age |
| Ordinary residential, office, commercial, industrial or unnamed `buildings` footprint | **No landmark/XP subject at all**; neutral map building rendering is allowed | Only after a deliberate, verified conversion into an eligible landmark with stable identity and meaningful evidence |
| Multiple labels or archive pieces for the same physical object | One canonical subject; no extra tier or award | Never multiply rarity or orbs by duplicates |

`landmark_rarity(version, valid_id)` already resolves to **Common** for an unlisted ID in published registry v1. That is a **fallback**, not proof that all physical buildings have stable IDs or qualify for `cell_orbs_rated`. Invalid IDs, unsupported registry versions or missing source-feature mappings must not be fabricated into Common or Uncommon.

**Uncommon is evidence-backed, not auto-generated by a weak tag** such as `memorial`, `historic`, a person name, a star rating, local popularity or a one-off sporting event. A curation record must include the exact archive identity, corroborated local significance and deduplication decision. Do not promote a whole street, neighborhood or historic city automatically.

**Kyiv "Start" stadium case:** archival documentation confirms the August 1942 match was held on the then-Zenit pitch, but the narrative of executing players *for winning that match* is a disproven Soviet propaganda legend. See the [Kyiv monument inventory](https://pamyatky.kiev.ua/streets/sholudenka/stadion-zenit-teper-start-1939-na-yakomu-vidbuvsya-t-zv-match-smerti) and [Ukrainian Institute of National Memory's myth analysis](https://old.uinp.gov.ua/news/mif-9-serpnya-1942-roku-match-smerti-zavershivsya-rozstrilom-radyanskoi-komandi). Do **not** assign Rare or Special merely on that story. Treat it as a **candidate for reviewed Uncommon** based on corroborated local sports history, with no active registry assignment unless identity and evidence are verified.

**Archive blocker (not a reason to guess):** current Web normalization can derive device-local landmark IDs from kind/rounded coordinates; Core v4 needs the archive's stable feature ID. The published Kyiv `pois` kinds and joined `landuse`/label features must be inspected and cross-mapped before bulk curation. The generic `buildings` style fields (`height`, `min_height`) alone do **not** establish a portable immutable landmark identity. Refer to [Web's map archive contract](https://github.com/nilx-one/web/blob/master/docs/avaia-osm-landmarks.md). Until identity and source provenance are confirmed, a mass Uncommon publish is **blocked**; it must not quietly select random building fragments.

**Separate visitor-place rarity:** parks, public green areas, beaches, lakes, recreation grounds and other destinations may also be mapped as normalized `LandmarkKind`s, but their **visitor-place rating is a separate classification facet** over that same physical mapped subject. See [Place rarity policy](place-rarity-policy.md). Do not copy this facet into the orb landmark registry or mint extra subjects.

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
