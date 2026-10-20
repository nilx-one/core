# Landmark notebook

Status: registered client shape; not owned by 0x1 Core. No Rust implementation.

The landmark notebook is what one device remembers about the landmarks around a Bond: what its owner walked past, what its Avaia studied, and what its Avaia stepped aside for and described. It is device-local and is not protocol evidence. This document exists only so a future Core adoption does not silently invent a different shape. The direction it records is [`nilx-one/ai`](https://github.com/nilx-one/ai)'s `docs/map-walk-and-landmarks.md` (MWL4, MWL5, MWL14, MWL15).

## Canonical client shape

```text
NOTEBOOK_LIMIT = 200          // per list; the oldest entry goes first

LandmarkNotebook {
  noticed: NoticedLandmark[]
  studied: StudiedLandmark[]
  seen:    SeenLandmark[]     // absent in a notebook written before it; read as []
  descriptions: DescribedLandmark[]   // one per landmark, written on the drive's `keep`; absent reads as []
}

NoticedLandmark { landmark: Landmark, noticedAt: u64 }
StudiedLandmark { landmark: Landmark, studiedAt: u64, by: AvaiaAddress }
SeenLandmark    { landmark: Landmark, seenAt:    u64, by: AvaiaAddress }
DescribedLandmark {
  landmarkId:      string                 // a Landmark.id
  text:            string                 // at most 240 characters
  producedBy:      "template" | ModelId   // what worded it
  templateVersion: u32                    // the faculty's template it was worded from
  describedAt:     u64
}

Landmark {
  id:        string            // stable for the same basemap feature
  longitude: f64
  latitude:  f64
  kind:      string            // the basemap's own kind: monument, memorial, artwork, …
  name?:     string
  facts:     { string: string | number | boolean }   // the archive's attributes, verbatim
}
```

Times are Unix epoch milliseconds. A list holds at most one entry per landmark (`noticed`) or per landmark and Avaia (`studied`, `seen`). `noticed` keeps the first time; writing a `studied` or `seen` entry again replaces the earlier one and moves it to the end.

## Semantics

- **Three ways in, one each.** `noticed` is written from the owner's own device observation: an accurate fix close to a landmark the basemap draws. The one exception is a Bond the service grants the administrator capability, whose declared position notices too. Such a notice pays no experience (nilx-one/ai MWL15), and for any other Bond a declared position writes nothing. `studied` is written only when an Avaia's study of a noticed landmark completes. `seen` is written when the [Avaia drive](avaia-drive.md)'s `glance` lands on a landmark: the Avaia stepped aside for it on its walk and said what it is.
- **A description is its facts.** What a `seen` or `studied` entry keeps is the landmark as the archive gives it. The short `description` kept on the drive's `keep` is worded from those facts ([`nilx-one/ai`](https://github.com/nilx-one/ai)'s `docs/landmark-description.md`). A model's retelling is a cache, marked with the model that produced it, which a device may drop and word again from the template.
- **Seen is not studied.** A `seen` entry earns no experience, and it never makes a landmark a curiosity candidate: the drive's `curiosity_options` come from `noticed` only. A landmark both seen and noticed is studyable for being noticed.
- **Local.** Entries stay on the device. They are not synchronized, transmitted to a service, analytics pipeline, error collector or another Bond, and are never training signal without a separate governance contract.
- **Not BondChain, not presence.** No entry says anyone was anywhere. A `seen` entry is where an Avaia's walk went, which is presentation, and `noticed` is a passing fix, not a visit. None of them completes an Interaction or establishes proximity, attendance or any Relationship interpretation.

## Deliberately unresolved

- **Core ownership.** As for the [presence journal](presence-journal.md): a record that is never synced carries no cross-device claim, so there is nothing for the kernel to enforce.
- **Device change.** The notebook does not survive a device change by contract. Export, recovery and transfer are unspecified.
- **A landmark projection.** These are basemap features read locally, not `map.registry` content. When `nilx-one/0x1` versions a landmark projection, whether a notebook entry may refer to one is a change to this note.

## Ownership rule

Until a later normative decision says otherwise, Web/client code owns persistence and lifecycle. Core owns no notebook API, performs no validation of these records, and must not expose them as protocol truth.

---

© 2026 aiaiaiai · aiaiaiai.org
