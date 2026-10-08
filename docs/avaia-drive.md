# Avaia drive

Status: implementation of a product-behavior contract. It is not protocol truth: where an Avaia walks is presentation, never presence evidence, an interaction or a `BondChain` fact. The model and identity it may grow into stay with [`nilx-one/ai`](https://github.com/nilx-one/ai); this crate owns the deterministic part, so every client does the same thing with the same inputs.

## The boundary

```text
host world layer ──input──▶ Core drive ──commands──▶ host executor
   (tiles, routes,           (decides)                (walks, animates,
    notebook, feelings)          │                     says lines, studies,
                                 └──choose──▶ model ──index──┘ picks finds up)
```

- **Core decides** what the Avaia does next: when it strolls, when it goes out and where to, whether something on the way is worth a detour, when a stand is over. One call, `avaia_drive_step(state, input, now_ms, hour)`, is one transition.
- **Its needs come first.** Hunger, energy and the need to go home and rest are [Avaia life](avaia-life.md)'s. The host passes each life report on as a `life` input. While life asks for home (`return_home`), the drive takes it home and nothing of its own competes. While it rests (`recover`), the drive offers nothing. Only `explore` lets it be curious, stroll, go out and be distracted. The drive keeps no energy of its own: it reads life's.
- **The host carries it out.** Routing, the walking graph, fog, animation, the notebook, experience and every line's wording are the host's. The host also resolves the ground: whenever the drive asks, it answers with opaque refs and whole metres.
- **The drive never sees a place.** No coordinates, names or addresses cross the boundary, only `DriveRef`s the host minted (`b:3`, `poi:12`, `node:881`) and closed kind codes (`monument`, `park`, `lake`, `find`). The drive only points at refs it was given.
- **A model chooses, it does not mint.** At a decision point the drive emits `choose`: a closed menu, and its own pick. The host may put the menu to a local model and answer with an index, or answer `null`. Anything off the menu, a late answer (after 4 s), or no answer leaves the drive's own pick standing. Without a model the Avaia behaves the same way, minus the character a model gives its choices.

## Wire

`avaia_drive_step(state: string, input: string, now_ms: string, hour: u8) -> string`

- `state` is the JSON the previous call returned, or `""` for a fresh drive. The host stores it as it is.
- `now_ms` is the wall clock as a canonical decimal string; `hour` is the local hour, 0–23.
- The answer is `{"ok":true,"state":{…},"commands":[…]}`, or `{"ok":false,"error":"invalid"}` for a malformed state, a state of a newer `version`, a malformed input, or an hour past 23. A refused input leaves the stored state as it was.
- Every `u64` travels as a decimal string. Unknown members are refused.
- The last command is `wake_at` whenever something is due later: the host keeps one timer and sends `tick` no later than then.

### Inputs

| `type` | Members | When |
| --- | --- | --- |
| `tap` | `to` | The owner set a point B. Always wins. |
| `arrived` | — | The walk under way got where it was going. |
| `blocked` | `by`: `building` / `water` / `fog` / absent | The host found no way for the walk it was told to make. |
| `stopped` | — | The Avaia left the wheel, or the world opened again. It settles where it is. |
| `tick` | — | Time passed. |
| `life` | `intent` (`explore` / `return_home` / `recover`), `energy` (decimal string, 0–10000), `home?` | Avaia life's latest report, whenever it changes. `home` names home for the way back. |
| `passing` | `things`: `ref`, `kind`, `group` (`landmark` / `area` / `find`), `off_route_m`, `studyable` | Things within reach of the walk under way. The host may report the same thing again; the drive remembers what it saw. |
| `curiosity_options` | `to`: `ref`, `longing`, `kind?`, `meters?`, `feeling?` | Answers `resolve` `curiosity`: notebook landmarks, most wanted first. `kind`, `meters` and `feeling` are for a model's menu; the drive's own pick does not read them. |
| `stroll_options` | `to`: refs | Answers `resolve` `stroll`: graph nodes in a fixed order. |
| `outing_options` | `targets`: `ref`, `kind`, `meters`, `appeal?`, `feeling?`, `stay_ms?`, `revisit_ms?`; `wander`: refs; `home?`: `ref`, `meters` | Answers `resolve` `outing`. |
| `chosen` | `index`: a menu index or `null` | Answers `choose`. |

### Commands

| `do` | Members | The host… |
| --- | --- | --- |
| `walk` | `to`, `purpose`, `grass` | walks there; only a tap (`grass: true`) may cross the grass the whole way, its own walks keep to the paths. Then sends `arrived` or `blocked`. |
| `look` | `ms` | plays the look-around for `ms`. |
| `study` | `at` | studies the landmark: notebook entry, experience, its line. |
| `glance` | `at` | looks at what it stepped aside for and says what it is: the archive's own name, kind and facts. |
| `pick_up` | `at` | picks the find up. |
| `visited` | `at` | records how the visit felt. |
| `say` | `line`, `about?` | says a line in the study's voice: `walk`, `stroll`, `landmark.spotted`, `landmark.longing`, `blocked.building`, `blocked.water`, `blocked.fog`. |
| `resolve` | `what`, `min_m`, `max_m`, `leash_m?`, `anchor?`, `wander_m?` | answers with the matching `*_options` input. A stroll's candidates lie `min_m`–`max_m` along the paths and within `leash_m` of `anchor` (where it settled; absent, where the body is). An outing's targets lie within `max_m`, and its wander nodes `wander_m` along the paths. |
| `choose` | `what` (`outing` / `distraction` / `curiosity`), `heading?`, `menu`, `default` | puts the menu to a model, or not, and answers `chosen`. |
| `wake_at` | `ms` | ticks no later than `ms`. |

A menu option is `index`, `action` and, where it applies, `kind`, `reach` (`near` up to 1 km, `far`) and `feeling` (`new`, `known`, `fond`, `loved`). The actions are a closed set: `carry_on`, `glance`, `pick_up` for a distraction; `stay`, `go`, `wander`, `home` for an outing; `stay`, `study` for curiosity. `heading` tells a model what the walk a distraction would interrupt is for, such as `tap`: a point B its owner set.

## Behavior

### Back on its own after point B

Arriving at a point B, it stands 20 s looking around, then settles there. It does not freeze until the next outing:

1. **Curiosity**: 15 s after it settles (1.5 s after taking the wheel), it asks for notebook landmarks and puts them to a model: `stay`, then up to six landmarks to `study`, each with its `kind`, `reach` (from `meters`) and `feeling` when the host gives them. A place it longs for reads as at least `fond`. The drive's own pick is the first landmark, the one the host wants most. It walks to the chosen one, saying `landmark.longing` for a fond or loved place and `landmark.spotted` otherwise, then studies it. Staying leaves it idle, and the strolls come next. With nothing in the notebook, no menu is offered.
2. **Strolls**: 30 s after it settles, it asks for stroll nodes 30–120 m away along the paths and within 200 m of where it settled, walks to one, and looks around for 8 s. It says `stroll` on the first stroll after settling only. Each stroll in a row doubles the wait, up to 5 min. In the evening and at night (20:00–07:00) the wait doubles again. A stroll with nowhere to go counts as a stroll.
3. **A stroll is not an outing.** It never touches the interval between outings, and it does not settle the Avaia, so restlessness counts on and the outing comes on time. A tired Avaia (life's energy below 4000) does not stroll, and no stroll is offered when an outing would be due first.

### Outings

The wish to go out is energy's. Rested, it is restless 3 min after it settled; the less energy life reports, the longer that takes, evenly up to 10 min with none left. Restless, and never within 20 min of the last outing, it asks for targets within a there-and-back on what life's energy allows (2.5 km out on full energy, proportionally less, never past 3 km). The menu is `stay`, up to six targets, a wander (one node, picked per 4 h window) and `home` when home is more than 50 m away. A target visited within its `revisit_ms` (7 days unless the host says the place is dearer) stays off the menu. Home is on the menu for a model to take, never the drive's own pick: when its needs ask for home, life says so. The drive's own pick:

- in the evening and at night: only a target within 1 km;
- the target with the highest `appeal` when the host gives one, else the nearest;
- no target: a wander, else stay.

Arriving at a target, it stays `stay_ms` (30 s unless the host says), then reports `visited`. Deciding to stay counts as an outing for the interval.

### Needs

A `life` report of `return_home` takes it home from anything of its own: a stroll, a visit, a detour, a pending choice. A point B its owner set is the exception: the walk and the stand there finish first, then it goes home. Nothing on the way distracts it while its needs lead, and it says `walk` once as it sets off. Arriving home, it waits there for life to say `recover`, then `explore`, and does not set off home again. A way home the host finds blocked is tried again after 30 s and is not an outing. Recovery itself — how long, how much — is life's.

### Distractions on the way

On any walk except a detour or the way home, and only while life says `explore`, a `passing` thing may interrupt it: a find within 15 m of the way, or a landmark or an area within 25 m. A find comes first. The drive asks `carry_on` or `glance`/`pick_up`. Its own pick is to step aside, except that a tired Avaia carries on past a sight but still bends for a find. On a yes it walks to the thing (`landmark.spotted` for a sight), then:

- picks a find up (2.4 s);
- studies a landmark the owner walked past and it has not studied (`studyable`, 3.2 s);
- glances at anything else (6 s).

Then it carries on to where the walk was going, point B included, saying nothing. A walk is interrupted at most twice, the same thing never twice, and arriving while a distraction is being weighed drops it. A tap outranks everything, a pending choice and resolve included.

### Waiting

A `resolve` not answered within 30 s counts as nothing around. A `choose` not answered within 4 s stands as its default. A `blocked` tap says why (`blocked.<by>`) and settles where it is. A blocked detour carries on to where it was going. A blocked stroll counts as a stroll. A blocked outing counts as the outing.

## Determinism

The same state, input, `now_ms` and `hour` give the same answer on every runtime. Energy is life's, 0–10000; distances are in whole metres, times in milliseconds. A pick among refs uses a fixed integer mix (splitmix64) of a seed the state determines, so a host must list candidates in a fixed order.

## What it deliberately does not do

- It does not route, read tiles or know the fog: a candidate the host offers is reachable over open ground, or the host does not offer it.
- It does not word lines, study notes, find rewards or feelings. `appeal`, `feeling`, `stay_ms` and `revisit_ms` are the host's record of places, read and never written here.
- It does not keep needs: hunger, energy and recovery are [Avaia life](avaia-life.md)'s, and the drive only reads them.
- It does not run while the Avaia is away from the wheel: `stopped` ends everything, and nothing walks in the background.

---

© 2026 aiaiaiai · aiaiaiai.org
