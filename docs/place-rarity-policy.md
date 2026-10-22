# Visitor-place rarity (separate curation facet)

**Status:** proposed content policy; **not** an activated Core API, wire field, XP table or new domain entity. Canonical product direction: [0x1 visitor-place rarity](https://github.com/nilx-one/0x1/blob/master/documents/product-direction/place-rarity.md). Historic buildings and monuments use the independent [landmark rarity policy](landmark-rarity-policy.md).

## Reuse one geographic subject

A park, garden, public square, recreation area, riverside, public beach, lake, scenic viewpoint or similarly visitable public feature may be identified in the existing archive/normalized `LandmarkRecord`. Some normalized `LandmarkKind`s already include parks, lakes and viewpoints. **Do not create a competing `Place`, `Destination` or second `BusinessPoint` record** for the same real object. A `BusinessPoint` retains its own business owner, verification and addressable ID, and is not a public park or heritage certification.

The proposal is an **independent, typed curation facet** named `visitor_place_rarity` attached only after one canonical geographic ID is verified. It may reuse the five display tiers Common, Uncommon, Rare, Special and Exclusive, **but it is NOT `landmark_rarity(registry,id)`**, not `RarityV3::base_find_xp`, and does not change the rated orb's rarity ceiling.

When a park contains a monument, park and monument are **distinct factual subjects** with different archive identities (if verified), not two aliases for the same object. When one historic garden or ensemble is genuinely a single geographic subject relevant to both aspects, the same canonical record may carry two explicitly labelled **independent facets**; do not duplicate its geometry, destination or award.

## Visitor-place thresholds

These are editorial tiers, not stars, crowd counts, sentiment, entry prices or accessibility ratings.

| Tier | Evidence for the *specific place* |
| --- | --- |
| **Common** | Valid accessible public recreation/visitation feature without reviewed extra significance; default **only after** map source and subject eligibility are verified |
| **Uncommon** | Documented distinctive neighborhood/local importance as an intentionally designed, maintained or culturally meaningful visitor place |
| **Rare** | Verifiable regionally or nationally distinctive landscape/recreational/historical significance |
| **Special** | Exceptional **national or international** environmental, landscape, urban-design or visitation heritage, supported by evidence **specific to that place** |
| **Exclusive** | Special plus an individually proven **globally extraordinary** landscape, ecological, artistic or cultural contribution; never an automatic side effect of being famous or in a protected zone |

National nature protection, UNESCO listings, media popularity and tourism counts may be **evidence**, not automatic rarity assignments. A negative access or conservation restriction must be respected: **a protected/inaccessible location must not encourage unauthorized entry**. Neither walking, viewing a map nor collecting an orb proves presence.

## Candidate source and mapping gates

- **Named parks / reserves / beaches:** current Web maps joined `landuse` area `kind` with matching `pois` name and source feature ID, but deployment-specific joins must be inspected before publication. A valid public area is not automatically an individually rateable heritage monument.
- **Lakes / water:** matched `water` polygons and labels; treat shore access differently from the lake polygon itself. No walk-to-water targets.
- **Recreation grounds / public plazas / gardens / viewpoints:** enable only if archive-backed geometry, stable source ID, normalized kind, legitimate public access and a meaningful place boundary are confirmed. The current archive may not retain the needed fields. Never guess from a nearby label.
- **Overlaps / nesting:** park subdivisions, park + garden, lakes inside parks and an adjacent memorial require canonical scope and deduplication. Prevent stacking rewards for the same physical discovery.
- **Ordinary buildings and private businesses:** not public visitor-place rarity by default. Commercial ratings or BusinessPoint verification are completely different contracts.

An editor record requires the same minimum proof as a landmark: source archive revision + stable feature ID, canonical subject + geometry/boundary, accessibility/protection caveats, supported tier rationale, evidence URL, decision timestamp and review provenance. No runtime projection from a normalized display-only coordinate hash is accepted until a stable archive-ID crosswalk exists.

### Kyiv candidate audit, **not activated tiers**

The city's [nature reserve registry](https://pzf.land.kiev.ua/pzf-obl-26.php) documents **Mariinskyi Park, Volodymyrska Hill and Holosiivskyi Park named for M. Rylskyi** as protected parks. These are strong **curation candidates** whose actual visitor-place tier depends on the place-specific evidence and verified archive ID. Local neighborhood parks and gardens can start Common once eligible, with Uncommon reserved for documented local distinctiveness. Do not assign these named parks a live rarity merely from a protected designation.

## Explicit future rollout

A separate approved versioned Core contract must define how this facet is represented, if/when it is read over Wasm/UniFFI, and what it changes in Web presentation or exploration. **Until such a version exists, it is a curation policy only.** `LANDMARK_RARITY_REGISTRY_VERSION = 1` and `cell_orbs_rated` remain unchanged; no place-based orb, XP, BondChain or presence award is authorized by this document.

---

© 2026 aiaiaiai · aiaiaiai.org
