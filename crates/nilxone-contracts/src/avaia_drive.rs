// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The Avaia's drive: what she does next, on her own, while she is at the wheel.
//!
//! A host carries out what the drive says and tells it what happened; the drive
//! decides. One call is one transition: the stored state, one input and the
//! wall clock in, the next state and the commands to carry out back. The same
//! state, input, time and hour always give the same answer.
//!
//! The drive never sees a place. The host's world layer resolves the ground
//! into opaque [`DriveRef`]s with distances in whole metres — a point B, a
//! landmark, a park, a find, a graph node to stroll to — and the drive only
//! ever points at one of the refs it was given. Routing, animation, lines and
//! the notebook are the host's to carry out.
//!
//! Where a choice is the Avaia's to make — an outing, or whether something on
//! the way is worth a detour — the drive asks with [`DriveCommand::Choose`]: a
//! closed menu a model may pick from by index, and the drive's own pick, which
//! stands when no model answers, answers late, or answers with anything that is
//! not on the menu. A model points; it never mints a target.
//!
//! Nothing here is presence evidence, a `BondChain` fact or an interaction. A
//! walk is a body a device draws.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::DecimalU64;

/// Version of the stored drive state. A newer one is refused, never rewritten.
pub const AVAIA_DRIVE_VERSION: u16 = 1;

/// How long an Avaia that just took the wheel looks around before curiosity.
pub const FIRST_LOOK_MS: u64 = 1_500;
/// How long it stands idle before curiosity is asked again.
pub const CURIOSITY_IDLE_MS: u64 = 15_000;
/// How long it stands at a point B its owner set, looking around.
pub const POINT_B_STAND_MS: u64 = 20_000;
/// How long a settled Avaia stands before it strolls about on its own. Each
/// stroll in a row doubles the wait, up to [`STROLL_MAX_IDLE_MS`].
pub const STROLL_IDLE_MS: u64 = 30_000;
/// The longest wait between strolls.
pub const STROLL_MAX_IDLE_MS: u64 = 300_000;
/// How long it looks around where a stroll took it.
pub const STROLL_LOOK_MS: u64 = 8_000;
/// How long it stands settled before it is restless enough to go out.
pub const RESTLESS_MS: u64 = 600_000;
/// No two outings closer together than this.
pub const OUTING_INTERVAL_MS: u64 = 14_400_000;
/// How long it looks around a target it walked out to, unless told otherwise.
pub const VISIT_MS: u64 = 30_000;
/// How long it studies a landmark.
pub const STUDY_MS: u64 = 3_200;
/// How long it looks at something it stepped aside for on the way.
pub const GLANCE_MS: u64 = 6_000;
/// How long picking a find up takes.
pub const PICK_UP_MS: u64 = 2_400;
/// How long a choice waits for a model before the drive's own pick stands.
pub const CHOOSE_MS: u64 = 4_000;
/// How long the drive waits for the world layer to resolve what is around.
pub const RESOLVE_MS: u64 = 30_000;

/// Curiosity goes no further than this to a landmark.
pub const CURIOSITY_REACH_METERS: u32 = 3_000;
/// A stroll goes this far along the paths: a few steps, not an outing.
pub const STROLL_MIN_METERS: u32 = 30;
/// The furthest a single stroll goes.
pub const STROLL_MAX_METERS: u32 = 120;
/// A stroll never takes the Avaia further than this from where it settled.
pub const STROLL_LEASH_METERS: u32 = 200;
/// A wander goes this far along the paths.
pub const WANDER_MIN_METERS: u32 = 150;
/// The furthest a wander goes.
pub const WANDER_MAX_METERS: u32 = 400;
/// An outing never plans further than this.
pub const MAX_OUTING_METERS: u32 = 3_000;
/// A target up to this far is near; in the evening only a near one is taken.
pub const NEAR_METERS: u32 = 1_000;
/// Within this of home it is home.
pub const HOME_RADIUS_METERS: u32 = 50;
/// A landmark or an area further off the way than this does not distract.
pub const GLANCE_OFF_ROUTE_METERS: u32 = 25;
/// A find further off the way than this is not seen at all.
pub const FIND_OFF_ROUTE_METERS: u32 = 15;
/// A target visited this recently is left off the menu, unless the host
/// says the place is dear enough to go back to sooner.
pub const REVISIT_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
/// No walk is interrupted more often than this.
pub const DISTRACTIONS_PER_WALK: u8 = 2;

/// Full energy, in thousandths.
pub const FULL_ENERGY: u16 = 1_000;
/// Below this an Avaia away from home goes home, and does not stroll.
pub const LOW_ENERGY: u16 = 300;
/// Energy spent per kilometre, in thousandths: a full charge is 5 km.
pub const ENERGY_PER_KM: u16 = 200;

/// The most refs a walk remembers having passed.
const SEEN_PER_WALK: usize = 32;
/// The most targets an outing menu offers.
const MENU_TARGETS: usize = 6;
/// The longest ref the drive accepts.
const REF_MAX_CHARS: usize = 128;
/// The longest kind code.
const KIND_MAX_CHARS: usize = 32;

/// Why the drive refused an input. One code, on purpose: a host that sends a
/// malformed state or input has a bug, not a case to branch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriveError;

impl std::fmt::Display for DriveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid avaia drive state or input")
    }
}

impl std::error::Error for DriveError {}

/// An opaque name the host's world layer gave something: a point B, a
/// landmark, a graph node. The drive compares refs and hands them back; it
/// never reads one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DriveRef(String);

impl DriveRef {
    /// A ref of 1 to 128 characters with no control characters and not all
    /// whitespace.
    ///
    /// # Errors
    ///
    /// Returns [`DriveError`] otherwise.
    pub fn new(value: impl Into<String>) -> Result<Self, DriveError> {
        let value = value.into();
        if value.trim().is_empty()
            || value.chars().count() > REF_MAX_CHARS
            || value.chars().any(char::is_control)
        {
            return Err(DriveError);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DriveRef {
    type Error = DriveError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DriveRef> for String {
    fn from(value: DriveRef) -> Self {
        value.0
    }
}

/// What a thing is, as the world layer's closed landmark vocabulary names it
/// (`monument`, `park`, `lake`, `find`, …): lowercase words joined by `_`.
/// The drive passes it to a model's menu and reads nothing else into it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DriveKind(String);

impl DriveKind {
    /// # Errors
    ///
    /// Returns [`DriveError`] for anything but `[a-z][a-z_]*`, up to 32.
    pub fn new(value: impl Into<String>) -> Result<Self, DriveError> {
        let value = value.into();
        let mut bytes = value.bytes();
        let shaped = bytes.next().is_some_and(|first| first.is_ascii_lowercase())
            && bytes.all(|byte| byte.is_ascii_lowercase() || byte == b'_');
        if !shaped || value.len() > KIND_MAX_CHARS {
            return Err(DriveError);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DriveKind {
    type Error = DriveError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DriveKind> for String {
    fn from(value: DriveKind) -> Self {
        value.0
    }
}

/// What a walk is for. A tap is the owner setting a point B; a detour is a
/// step aside on the way, which carries on where it was headed afterwards;
/// the rest are the Avaia's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Tap,
    Curiosity,
    Outing,
    Wander,
    Home,
    Stroll,
    Detour,
}

/// Why the Avaia is standing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stand {
    /// At a point B its owner set.
    PointB,
    /// At an outing's target.
    Visit,
    /// Where a stroll took it.
    Look,
    /// Studying a landmark.
    Study,
    /// Looking at something it stepped aside for.
    Glance,
    /// Picking a find up.
    PickUp,
}

/// What the Avaia is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Activity {
    Idle {
        #[serde(with = "ms")]
        since_ms: u64,
    },
    Walking {
        purpose: Purpose,
        to: DriveRef,
        #[serde(with = "ms")]
        since_ms: u64,
    },
    Standing {
        reason: Stand,
        at: DriveRef,
        #[serde(with = "ms")]
        until_ms: u64,
    },
}

/// What a thing passed on the way is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// A point the archive draws: a monument, an artwork, a fountain.
    Landmark,
    /// A polygon: a park, a lake shore.
    Area,
    /// A find the Avaia may pick up.
    Find,
}

/// Where a detour returns to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resume {
    pub purpose: Purpose,
    pub to: DriveRef,
}

/// What the Avaia stepped aside for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detour {
    pub group: Group,
    /// A landmark its owner walked past and it has not studied: studied, not
    /// only glanced at.
    pub studyable: bool,
}

/// What one walk remembers while it lasts, across its detours.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WalkMemory {
    pub distractions: u8,
    pub seen: Vec<DriveRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<Resume>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detour: Option<Detour>,
    /// How long the outing under way means to stay where it is going.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "opt_ms")]
    pub stay_ms: Option<u64>,
}

/// What the world layer is asked to resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolve {
    Curiosity,
    Stroll,
    Outing,
}

/// What a menu entry does, in the closed vocabulary a model chooses from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Keep walking where it was going.
    CarryOn,
    /// Step aside and look at something on the way.
    Glance,
    /// Step aside and pick a find up.
    PickUp,
    /// Stay where it is.
    Stay,
    /// Go out to a target.
    Go,
    /// Wander a short way along the paths.
    Wander,
    /// Go home.
    Home,
}

/// How the Avaia feels about a place, as the host's record of places says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feeling {
    New,
    Known,
    Fond,
    Loved,
}

/// How far, as a model reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    Near,
    Far,
}

/// One entry of a pending choice, with what carrying it out needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub action: Action,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<DriveRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<DriveKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<Reach>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feeling: Option<Feeling>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detour: Option<Detour>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "opt_ms")]
    pub stay_ms: Option<u64>,
}

impl Entry {
    const fn bare(action: Action) -> Self {
        Self {
            action,
            to: None,
            kind: None,
            reach: None,
            feeling: None,
            detour: None,
            stay_ms: None,
        }
    }
}

/// What the choice is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// Whether to step aside for something on the way.
    Distraction,
    /// Where to go out to.
    Outing,
}

/// What the drive is waiting on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "for", rename_all = "snake_case", deny_unknown_fields)]
pub enum Pending {
    /// The world layer resolving what is around.
    Resolve {
        what: Resolve,
        #[serde(with = "ms")]
        until_ms: u64,
    },
    /// A model, or nobody, choosing from a menu.
    Choose {
        what: Choice,
        entries: Vec<Entry>,
        default: usize,
        #[serde(with = "ms")]
        until_ms: u64,
    },
}

/// The drive's whole state, stored by the host between calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveState {
    pub version: u16,
    pub activity: Activity,
    /// When it last settled after something it set out to do. Strolling
    /// about does not settle it, so restlessness keeps growing while it
    /// potters.
    #[serde(with = "ms")]
    pub settled_ms: u64,
    /// Where it settled, when that was somewhere the host named: the point
    /// its strolls keep near. `None` means where the body is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<DriveRef>,
    /// Strolls since it last settled; each makes the next wait longer.
    pub strolls: u32,
    /// 0 to [`FULL_ENERGY`].
    pub energy: u16,
    #[serde(with = "opt_ms")]
    pub last_outing_ms: Option<u64>,
    /// Target to when it was last visited.
    pub visited: BTreeMap<DriveRef, DecimalU64>,
    /// Whether anything happened since it took the wheel.
    pub acted: bool,
    /// Whether curiosity was asked during the idle under way.
    pub curiosity_asked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walk: Option<WalkMemory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Pending>,
}

impl DriveState {
    /// A fresh drive at `now_ms`: idle, rested, never out.
    #[must_use]
    pub fn new(now_ms: u64) -> Self {
        Self {
            version: AVAIA_DRIVE_VERSION,
            activity: Activity::Idle { since_ms: now_ms },
            settled_ms: now_ms,
            anchor: None,
            strolls: 0,
            energy: FULL_ENERGY,
            last_outing_ms: None,
            visited: BTreeMap::new(),
            acted: false,
            curiosity_asked: false,
            walk: None,
            pending: None,
        }
    }
}

/// Something on the way the world layer perceived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Passing {
    #[serde(rename = "ref")]
    pub thing: DriveRef,
    pub kind: DriveKind,
    pub group: Group,
    /// How far off the way it lies.
    pub off_route_m: u32,
    #[serde(default)]
    pub studyable: bool,
}

/// A landmark curiosity may go to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curious {
    #[serde(rename = "ref")]
    pub thing: DriveRef,
    /// A place it misses rather than a new one.
    #[serde(default)]
    pub longing: bool,
}

/// A target an outing may go to, with what the host's record of places says
/// about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    #[serde(rename = "ref")]
    pub thing: DriveRef,
    pub kind: DriveKind,
    pub meters: u32,
    /// How much it appeals, 0 to 1000, when the host knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appeal: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feeling: Option<Feeling>,
    /// How long a visit there lasts, when the place says.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "opt_ms")]
    pub stay_ms: Option<u64>,
    /// How long after a visit it may go back, when the place is dearer than
    /// most; [`REVISIT_MS`] otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "opt_ms")]
    pub revisit_ms: Option<u64>,
}

/// Home, as far as it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Home {
    #[serde(rename = "ref")]
    pub thing: DriveRef,
    pub meters: u32,
}

/// What the ground did in the way of a walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Obstacle {
    Building,
    Water,
    Fog,
}

/// What happened, as the host tells the drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DriveInput {
    /// The owner set a point B. Always wins.
    Tap { to: DriveRef },
    /// The walk under way arrived, `meters` after it set off.
    Arrived { meters: u32 },
    /// The host found no way for the walk it was told to make.
    Blocked {
        #[serde(default)]
        by: Option<Obstacle>,
    },
    /// The Avaia left the wheel, or the world was opened again: whatever it
    /// was doing ends, and it settles where it is.
    Stopped {},
    /// Time passed.
    Tick {},
    /// Things on the way, as the walk comes within reach of them.
    Passing { things: Vec<Passing> },
    /// Landmarks curiosity may go to, most wanted first.
    CuriosityOptions { to: Vec<Curious> },
    /// Graph nodes a stroll may go to, in a fixed order.
    StrollOptions { to: Vec<DriveRef> },
    /// What an outing may go to.
    OutingOptions {
        targets: Vec<Target>,
        /// Graph nodes a wander may go to, in a fixed order.
        wander: Vec<DriveRef>,
        #[serde(default)]
        home: Option<Home>,
    },
    /// A model's pick from the menu, or `None` when none answered.
    Chosen {
        #[serde(default)]
        index: Option<usize>,
    },
}

/// A line the Avaia says, in the host's per-study voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Line {
    #[serde(rename = "walk")]
    Walk,
    #[serde(rename = "stroll")]
    Stroll,
    #[serde(rename = "landmark.spotted")]
    LandmarkSpotted,
    #[serde(rename = "landmark.longing")]
    LandmarkLonging,
    #[serde(rename = "blocked.building")]
    BlockedBuilding,
    #[serde(rename = "blocked.water")]
    BlockedWater,
    #[serde(rename = "blocked.fog")]
    BlockedFog,
}

/// One entry of a menu as a model reads it: no refs, no names, no metres.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuOption {
    pub index: usize,
    pub action: Action,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<DriveKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<Reach>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feeling: Option<Feeling>,
}

/// What the host carries out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", deny_unknown_fields)]
pub enum DriveCommand {
    /// Walk to `to`. Only a tap may cross the grass the whole way; the
    /// Avaia's own walks keep to the paths.
    Walk {
        to: DriveRef,
        purpose: Purpose,
        grass: bool,
    },
    /// Stand and look around for `ms`.
    Look {
        #[serde(with = "ms")]
        ms: u64,
    },
    /// Study a landmark: the notebook entry, the experience and its line are
    /// the host's.
    Study { at: DriveRef },
    /// Look at something on the way and say what it is.
    Glance { at: DriveRef },
    /// Pick a find up.
    PickUp { at: DriveRef },
    /// A visit is over: the host records how it felt.
    Visited { at: DriveRef },
    /// Say a line, about `about` when it names a place.
    Say {
        line: Line,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        about: Option<DriveRef>,
    },
    /// Resolve what is around, within the bounds given, and answer with the
    /// matching options input.
    Resolve {
        what: Resolve,
        min_m: u32,
        max_m: u32,
        /// How far from `anchor` a candidate may lie, for a stroll.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leash_m: Option<u32>,
        /// Where the Avaia settled, which a stroll keeps near; absent, where
        /// the body is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<DriveRef>,
        /// How far along the paths a wander may go, for an outing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wander_m: Option<[u32; 2]>,
    },
    /// Choose from the menu by index, and answer with `chosen`. `default` is
    /// the drive's own pick.
    Choose {
        what: Choice,
        /// For a distraction, what the walk it would interrupt is for: a
        /// point B its owner set, or one of its own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heading: Option<Purpose>,
        menu: Vec<MenuOption>,
        default: usize,
    },
    /// Tick no later than `ms`.
    WakeAt {
        #[serde(with = "ms")]
        ms: u64,
    },
}

/// One transition. `hour` is the local hour, 0 to 23, which the evening
/// rules read. The last command is a [`DriveCommand::WakeAt`] whenever
/// anything is due later.
///
/// # Errors
///
/// Returns [`DriveError`] for a state of another version or an hour past 23.
pub fn step(
    state: &DriveState,
    input: DriveInput,
    now_ms: u64,
    hour: u8,
) -> Result<(DriveState, Vec<DriveCommand>), DriveError> {
    if state.version != AVAIA_DRIVE_VERSION || hour > 23 {
        return Err(DriveError);
    }
    let mut next = state.clone();
    let mut out = Vec::new();
    let mut drive = Step {
        state: &mut next,
        out: &mut out,
        now: now_ms,
        evening: is_evening(hour),
    };
    drive.apply(input);
    drive.act_when_due();
    if let Some(at) = next_due(&next, hour) {
        out.push(DriveCommand::WakeAt { ms: at.max(now_ms) });
    }
    Ok((next, out))
}

/// When the drive next wants a tick at the local `hour`, or `None` when
/// nothing is due.
#[must_use]
pub fn next_due(state: &DriveState, hour: u8) -> Option<u64> {
    let pending = match &state.pending {
        Some(Pending::Resolve { until_ms, .. } | Pending::Choose { until_ms, .. }) => {
            Some(*until_ms)
        }
        None => None,
    };
    let activity = match &state.activity {
        Activity::Standing { until_ms, .. } => Some(*until_ms),
        Activity::Idle { .. } if state.pending.is_none() => {
            idle_due(state, is_evening(hour)).map(|(_, at)| at)
        }
        _ => None,
    };
    match (pending, activity) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Restlessness, 0 to 1000: 0 while busy, climbing over [`RESTLESS_MS`] since
/// it settled. Strolling about does not calm it.
#[must_use]
pub fn restlessness(state: &DriveState, now_ms: u64) -> u16 {
    if !(matches!(state.activity, Activity::Idle { .. }) || pottering(&state.activity)) {
        return 0;
    }
    let since = now_ms.saturating_sub(state.settled_ms);
    u16::try_from(since.min(RESTLESS_MS) * 1_000 / RESTLESS_MS).unwrap_or(1_000)
}

/// How far out an outing may plan: a there-and-back on what energy is left.
#[must_use]
pub fn outing_budget_meters(state: &DriveState) -> u32 {
    let there_and_back = u32::from(state.energy) * 1_000 / u32::from(ENERGY_PER_KM) / 2;
    there_and_back.min(MAX_OUTING_METERS)
}

/// The idle deadline that comes first, with what it is for. Ties go to
/// curiosity, then the outing, then a stroll.
fn idle_due(state: &DriveState, evening: bool) -> Option<(Resolve, u64)> {
    let Activity::Idle { since_ms } = state.activity else {
        return None;
    };
    let mut due: Option<(Resolve, u64)> = None;
    let mut consider = |what: Resolve, at: u64| {
        if due.is_none_or(|(_, first)| at < first) {
            due = Some((what, at));
        }
    };
    if !state.curiosity_asked {
        let wait = if state.acted {
            CURIOSITY_IDLE_MS
        } else {
            FIRST_LOOK_MS
        };
        consider(Resolve::Curiosity, since_ms + wait);
    }
    let outing = outing_due(state);
    consider(Resolve::Outing, outing);
    if state.energy >= LOW_ENERGY {
        let doubled = STROLL_IDLE_MS.saturating_mul(1 << state.strolls.min(16));
        let wait = doubled.min(STROLL_MAX_IDLE_MS) * if evening { 2 } else { 1 };
        let at = since_ms + wait;
        if at < outing {
            consider(Resolve::Stroll, at);
        }
    }
    due
}

/// The evening and the night: 20:00 to 07:00 local.
fn is_evening(hour: u8) -> bool {
    !(7..20).contains(&hour)
}

/// Restless enough, and the interval since the last outing passed.
fn outing_due(state: &DriveState) -> u64 {
    let restless = state.settled_ms + RESTLESS_MS;
    state
        .last_outing_ms
        .map_or(restless, |last| restless.max(last + OUTING_INTERVAL_MS))
}

/// Whether the Avaia is only strolling about, or looking around after it.
fn pottering(activity: &Activity) -> bool {
    matches!(
        activity,
        Activity::Walking {
            purpose: Purpose::Stroll,
            ..
        } | Activity::Standing {
            reason: Stand::Look,
            ..
        }
    )
}

struct Step<'a> {
    state: &'a mut DriveState,
    out: &'a mut Vec<DriveCommand>,
    now: u64,
    evening: bool,
}

impl Step<'_> {
    fn apply(&mut self, input: DriveInput) {
        match input {
            DriveInput::Tap { to } => {
                self.state.pending = None;
                self.state.walk = Some(WalkMemory::default());
                self.walk(Purpose::Tap, to);
                self.say(Line::Walk, None);
            }
            DriveInput::Arrived { meters } => self.arrived(meters),
            DriveInput::Blocked { by } => self.blocked(by),
            DriveInput::Stopped {} => {
                self.state.pending = None;
                self.state.walk = None;
                self.settle(None);
                self.state.acted = false;
            }
            DriveInput::Tick {} => self.tick(),
            DriveInput::Passing { things } => self.passing(things),
            DriveInput::CuriosityOptions { to } => {
                if self.resolved(Resolve::Curiosity) {
                    self.curious(to);
                }
            }
            DriveInput::StrollOptions { to } => {
                if self.resolved(Resolve::Stroll) {
                    self.stroll(&to);
                }
            }
            DriveInput::OutingOptions {
                targets,
                wander,
                home,
            } => {
                if self.resolved(Resolve::Outing) {
                    self.offer_outing(targets, &wander, home);
                }
            }
            DriveInput::Chosen { index } => self.chosen(index),
        }
    }

    /// Whether the options answer the resolve under way; it ends either way
    /// they match.
    fn resolved(&mut self, what: Resolve) -> bool {
        let matches = matches!(
            self.state.pending,
            Some(Pending::Resolve { what: asked, .. }) if asked == what
        );
        if matches {
            self.state.pending = None;
        }
        matches && matches!(self.state.activity, Activity::Idle { .. })
    }

    fn walk(&mut self, purpose: Purpose, to: DriveRef) {
        self.state.acted = true;
        self.state.activity = Activity::Walking {
            purpose,
            to: to.clone(),
            since_ms: self.now,
        };
        self.out.push(DriveCommand::Walk {
            to,
            purpose,
            grass: purpose == Purpose::Tap,
        });
    }

    fn say(&mut self, line: Line, about: Option<DriveRef>) {
        self.out.push(DriveCommand::Say { line, about });
    }

    fn stand(&mut self, reason: Stand, at: DriveRef, ms: u64) {
        self.state.activity = Activity::Standing {
            reason,
            at,
            until_ms: self.now + ms,
        };
    }

    /// Idle where it is, without settling: pottering about, or a stroll or an
    /// outing that found nothing.
    fn idle(&mut self) {
        self.state.activity = Activity::Idle { since_ms: self.now };
        self.state.curiosity_asked = false;
    }

    /// Idle where it is, settled at `at`: restlessness and strolls start
    /// over, and strolls keep near there.
    fn settle(&mut self, at: Option<DriveRef>) {
        self.idle();
        self.state.settled_ms = self.now;
        self.state.anchor = at;
        self.state.strolls = 0;
    }

    fn arrived(&mut self, meters: u32) {
        let Activity::Walking { purpose, to, .. } = self.state.activity.clone() else {
            return;
        };
        if matches!(
            self.state.pending,
            Some(Pending::Choose {
                what: Choice::Distraction,
                ..
            })
        ) {
            self.state.pending = None;
        }
        let spent = u64::from(meters) * u64::from(ENERGY_PER_KM) / 1_000;
        self.state.energy = self
            .state
            .energy
            .saturating_sub(u16::try_from(spent).unwrap_or(u16::MAX));
        match purpose {
            Purpose::Tap => {
                self.stand(Stand::PointB, to, POINT_B_STAND_MS);
                self.out.push(DriveCommand::Look {
                    ms: POINT_B_STAND_MS,
                });
            }
            Purpose::Outing => {
                let stay = self
                    .state
                    .walk
                    .as_ref()
                    .and_then(|walk| walk.stay_ms)
                    .unwrap_or(VISIT_MS);
                let now = self.now;
                self.state
                    .visited
                    .retain(|_, at| now.saturating_sub(at.get()) < REVISIT_MS);
                self.state.visited.insert(to.clone(), DecimalU64::new(now));
                self.stand(Stand::Visit, to, stay);
                self.out.push(DriveCommand::Look { ms: stay });
            }
            Purpose::Curiosity => {
                self.stand(Stand::Study, to.clone(), STUDY_MS);
                self.out.push(DriveCommand::Study { at: to });
            }
            Purpose::Home => {
                self.state.energy = FULL_ENERGY;
                self.state.walk = None;
                self.settle(Some(to));
            }
            Purpose::Wander => {
                self.state.walk = None;
                self.settle(Some(to));
            }
            Purpose::Stroll => {
                self.stand(Stand::Look, to, STROLL_LOOK_MS);
                self.out.push(DriveCommand::Look { ms: STROLL_LOOK_MS });
            }
            Purpose::Detour => self.arrived_aside(to),
        }
    }

    /// Arrived at what it stepped aside for: it picks it up, studies it or
    /// looks at it, by what it is.
    fn arrived_aside(&mut self, at: DriveRef) {
        let detour = self
            .state
            .walk
            .as_ref()
            .and_then(|walk| walk.detour.clone());
        match detour {
            Some(Detour {
                group: Group::Find, ..
            }) => {
                self.stand(Stand::PickUp, at.clone(), PICK_UP_MS);
                self.out.push(DriveCommand::PickUp { at });
            }
            Some(Detour {
                studyable: true, ..
            }) => {
                self.stand(Stand::Study, at.clone(), STUDY_MS);
                self.out.push(DriveCommand::Study { at });
            }
            _ => {
                self.stand(Stand::Glance, at.clone(), GLANCE_MS);
                self.out.push(DriveCommand::Glance { at });
            }
        }
    }

    /// Back to where a detour was headed, or settled where it is when there is
    /// nowhere to go back to.
    fn resume(&mut self) {
        let resume = self.state.walk.as_mut().and_then(|walk| {
            walk.detour = None;
            walk.resume.take()
        });
        if let Some(Resume { purpose, to }) = resume {
            self.walk(purpose, to);
        } else {
            self.state.walk = None;
            self.settle(None);
        }
    }

    fn blocked(&mut self, by: Option<Obstacle>) {
        let Activity::Walking { purpose, .. } = self.state.activity else {
            return;
        };
        match purpose {
            Purpose::Tap => {
                if let Some(by) = by {
                    self.say(
                        match by {
                            Obstacle::Building => Line::BlockedBuilding,
                            Obstacle::Water => Line::BlockedWater,
                            Obstacle::Fog => Line::BlockedFog,
                        },
                        None,
                    );
                }
                self.state.walk = None;
                self.settle(None);
            }
            Purpose::Detour => self.resume(),
            Purpose::Stroll => {
                self.idle();
                self.state.strolls = self.state.strolls.saturating_add(1);
            }
            Purpose::Outing | Purpose::Wander | Purpose::Home => {
                self.state.walk = None;
                self.state.last_outing_ms = Some(self.now);
                self.idle();
            }
            Purpose::Curiosity => {
                self.state.walk = None;
                self.idle();
                self.state.curiosity_asked = true;
            }
        }
    }

    fn tick(&mut self) {
        match &self.state.pending {
            Some(Pending::Resolve { what, until_ms }) if self.now >= *until_ms => {
                let what = *what;
                self.state.pending = None;
                // The world layer never answered: as if nothing was around.
                match what {
                    Resolve::Curiosity => self.curious(Vec::new()),
                    Resolve::Stroll => self.stroll(&[]),
                    Resolve::Outing => self.offer_outing(Vec::new(), &[], None),
                }
            }
            Some(Pending::Choose { until_ms, .. }) if self.now >= *until_ms => self.chosen(None),
            _ => {}
        }
        if let Activity::Standing {
            reason,
            at,
            until_ms,
        } = self.state.activity.clone()
            && self.now >= until_ms
        {
            self.stand_over(reason, at);
        }
    }

    fn stand_over(&mut self, reason: Stand, at: DriveRef) {
        let aside = self
            .state
            .walk
            .as_ref()
            .is_some_and(|walk| walk.resume.is_some());
        match reason {
            Stand::Visit => {
                self.out.push(DriveCommand::Visited { at: at.clone() });
                self.state.walk = None;
                self.settle(Some(at));
            }
            Stand::Look => self.idle(),
            Stand::Study | Stand::Glance | Stand::PickUp if aside => self.resume(),
            Stand::PointB | Stand::Study | Stand::Glance | Stand::PickUp => {
                self.state.walk = None;
                self.settle(Some(at));
            }
        }
    }

    /// An idle Avaia whose deadline came asks the world layer what is around.
    fn act_when_due(&mut self) {
        if self.state.pending.is_some() {
            return;
        }
        let Some((what, at)) = idle_due(self.state, self.evening) else {
            return;
        };
        if self.now < at {
            return;
        }
        if what == Resolve::Curiosity {
            self.state.curiosity_asked = true;
        }
        self.state.pending = Some(Pending::Resolve {
            what,
            until_ms: self.now + RESOLVE_MS,
        });
        let (min_m, max_m, leash_m, wander_m) = match what {
            Resolve::Curiosity => (0, CURIOSITY_REACH_METERS, None, None),
            Resolve::Stroll => (
                STROLL_MIN_METERS,
                STROLL_MAX_METERS,
                Some(STROLL_LEASH_METERS),
                None,
            ),
            Resolve::Outing => (
                0,
                outing_budget_meters(self.state),
                None,
                Some([WANDER_MIN_METERS, WANDER_MAX_METERS]),
            ),
        };
        let anchor = if what == Resolve::Stroll {
            self.state.anchor.clone()
        } else {
            None
        };
        self.out.push(DriveCommand::Resolve {
            what,
            min_m,
            max_m,
            leash_m,
            anchor,
            wander_m,
        });
    }

    fn curious(&mut self, to: Vec<Curious>) {
        let Some(first) = to.into_iter().next() else {
            return;
        };
        let line = if first.longing {
            Line::LandmarkLonging
        } else {
            Line::LandmarkSpotted
        };
        self.state.walk = Some(WalkMemory::default());
        self.walk(Purpose::Curiosity, first.thing.clone());
        self.say(line, Some(first.thing));
    }

    fn stroll(&mut self, to: &[DriveRef]) {
        let seed = self.state.settled_ms / 1_000 + u64::from(self.state.strolls);
        let first = self.state.strolls == 0;
        self.state.strolls = self.state.strolls.saturating_add(1);
        let Some(there) = pick(to, seed) else {
            self.idle();
            return;
        };
        self.state.walk = Some(WalkMemory::default());
        self.walk(Purpose::Stroll, there.clone());
        if first {
            self.say(Line::Stroll, None);
        }
    }

    fn offer_outing(&mut self, targets: Vec<Target>, wander: &[DriveRef], home: Option<Home>) {
        let mut entries = vec![Entry::bare(Action::Stay)];
        let mut ranks = Vec::new();
        let recent = |target: &Target| {
            self.state.visited.get(&target.thing).is_some_and(|at| {
                self.now.saturating_sub(at.get()) < target.revisit_ms.unwrap_or(REVISIT_MS)
            })
        };
        let targets: Vec<Target> = targets
            .into_iter()
            .filter(|target| !recent(target))
            .take(MENU_TARGETS)
            .collect();
        for target in targets {
            ranks.push((target.meters, target.appeal));
            entries.push(Entry {
                to: Some(target.thing),
                kind: Some(target.kind),
                reach: Some(reach(target.meters)),
                feeling: target.feeling,
                stay_ms: target.stay_ms,
                ..Entry::bare(Action::Go)
            });
        }
        let wander = pick(wander, self.now / OUTING_INTERVAL_MS).map(|there| {
            entries.push(Entry {
                to: Some(there.clone()),
                ..Entry::bare(Action::Wander)
            });
            entries.len() - 1
        });
        let home = home
            .filter(|home| home.meters > HOME_RADIUS_METERS)
            .map(|home| {
                entries.push(Entry {
                    to: Some(home.thing),
                    reach: Some(reach(home.meters)),
                    ..Entry::bare(Action::Home)
                });
                entries.len() - 1
            });
        if entries.len() == 1 {
            self.stayed();
            return;
        }
        let default = self.rule_outing(&ranks, wander, home);
        self.ask(Choice::Outing, entries, default);
    }

    /// The drive's own pick for an outing, as an index on the menu built by
    /// [`Self::offer_outing`] (stay, the targets, a wander, home):
    ///
    /// - tired and away from home: home;
    /// - in the evening and at night (20:00 to 07:00): only a near target;
    /// - the target that appeals most when the host says, else the nearest;
    /// - with no target, a wander, else stay.
    fn rule_outing(
        &self,
        targets: &[(u32, Option<u16>)],
        wander: Option<usize>,
        home: Option<usize>,
    ) -> usize {
        if self.state.energy < LOW_ENERGY
            && let Some(home) = home
        {
            return home;
        }
        targets
            .iter()
            .enumerate()
            .filter(|(_, (meters, _))| !self.evening || *meters <= NEAR_METERS)
            .min_by_key(|(index, (meters, appeal))| {
                (std::cmp::Reverse(appeal.unwrap_or(0)), *meters, *index)
            })
            .map(|(index, _)| index + 1)
            .or(wander)
            .unwrap_or(0)
    }

    fn ask(&mut self, what: Choice, entries: Vec<Entry>, default: usize) {
        let heading = match (&self.state.activity, what) {
            (Activity::Walking { purpose, .. }, Choice::Distraction) => Some(*purpose),
            _ => None,
        };
        let menu = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| MenuOption {
                index,
                action: entry.action,
                kind: entry.kind.clone(),
                reach: entry.reach,
                feeling: entry.feeling,
            })
            .collect();
        self.out.push(DriveCommand::Choose {
            what,
            heading,
            menu,
            default,
        });
        self.state.pending = Some(Pending::Choose {
            what,
            entries,
            default,
            until_ms: self.now + CHOOSE_MS,
        });
    }

    /// The outing was decided as staying: the interval starts over all the
    /// same.
    fn stayed(&mut self) {
        self.state.last_outing_ms = Some(self.now);
        self.idle();
    }

    fn passing(&mut self, things: Vec<Passing>) {
        let Activity::Walking { purpose, .. } = self.state.activity else {
            return;
        };
        let distractible = !matches!(purpose, Purpose::Detour | Purpose::Home);
        let Some(walk) = self.state.walk.as_mut() else {
            return;
        };
        if !distractible
            || self.state.pending.is_some()
            || walk.distractions >= DISTRACTIONS_PER_WALK
        {
            return;
        }
        let near = |thing: &Passing| match thing.group {
            Group::Find => thing.off_route_m <= FIND_OFF_ROUTE_METERS,
            Group::Landmark | Group::Area => thing.off_route_m <= GLANCE_OFF_ROUTE_METERS,
        };
        let fresh: Vec<Passing> = things
            .into_iter()
            .filter(|thing| near(thing) && !walk.seen.contains(&thing.thing))
            .collect();
        // A find is closer to hand than anything to look at.
        let Some(thing) = fresh
            .iter()
            .find(|thing| thing.group == Group::Find)
            .or_else(|| fresh.first())
            .cloned()
        else {
            return;
        };
        if walk.seen.len() >= SEEN_PER_WALK {
            walk.seen.remove(0);
        }
        walk.seen.push(thing.thing.clone());
        let action = if thing.group == Group::Find {
            Action::PickUp
        } else {
            Action::Glance
        };
        // Tired, it still bends for a find but no longer steps aside to look.
        let default = usize::from(action == Action::PickUp || self.state.energy >= LOW_ENERGY);
        let entries = vec![
            Entry::bare(Action::CarryOn),
            Entry {
                to: Some(thing.thing),
                kind: Some(thing.kind),
                reach: Some(Reach::Near),
                detour: Some(Detour {
                    group: thing.group,
                    studyable: thing.studyable && thing.group == Group::Landmark,
                }),
                ..Entry::bare(action)
            },
        ];
        self.ask(Choice::Distraction, entries, default);
    }

    fn chosen(&mut self, index: Option<usize>) {
        let Some(Pending::Choose {
            what,
            mut entries,
            default,
            ..
        }) = self.state.pending.take()
        else {
            return;
        };
        let index = index
            .filter(|index| *index < entries.len())
            .unwrap_or(default);
        let entry = entries.swap_remove(index);
        match what {
            Choice::Distraction => self.step_aside(entry),
            Choice::Outing => self.go_out(entry),
        }
    }

    fn step_aside(&mut self, entry: Entry) {
        let Activity::Walking { purpose, to, .. } = self.state.activity.clone() else {
            return;
        };
        let (Some(there), Some(detour)) = (entry.to, entry.detour) else {
            return;
        };
        let Some(walk) = self.state.walk.as_mut() else {
            return;
        };
        walk.distractions += 1;
        walk.resume = Some(Resume { purpose, to });
        let looks = detour.group != Group::Find;
        walk.detour = Some(detour);
        self.walk(Purpose::Detour, there.clone());
        if looks {
            self.say(Line::LandmarkSpotted, Some(there));
        }
    }

    fn go_out(&mut self, entry: Entry) {
        if !matches!(self.state.activity, Activity::Idle { .. }) {
            return;
        }
        let (purpose, Some(to)) = (
            match entry.action {
                Action::Go => Purpose::Outing,
                Action::Wander => Purpose::Wander,
                Action::Home => Purpose::Home,
                _ => {
                    self.stayed();
                    return;
                }
            },
            entry.to,
        ) else {
            self.stayed();
            return;
        };
        self.state.last_outing_ms = Some(self.now);
        self.state.walk = Some(WalkMemory {
            stay_ms: entry.stay_ms,
            ..WalkMemory::default()
        });
        self.walk(purpose, to.clone());
        if matches!(entry.feeling, Some(Feeling::Fond | Feeling::Loved)) {
            self.say(Line::LandmarkLonging, Some(to));
        } else {
            self.say(Line::Walk, None);
        }
    }
}

fn reach(meters: u32) -> Reach {
    if meters <= NEAR_METERS {
        Reach::Near
    } else {
        Reach::Far
    }
}

/// One of `refs`, picked by `seed`: the same refs and seed pick the same one.
fn pick(refs: &[DriveRef], seed: u64) -> Option<&DriveRef> {
    if refs.is_empty() {
        return None;
    }
    let index = mix(seed) % u64::try_from(refs.len()).unwrap_or(u64::MAX);
    refs.get(usize::try_from(index).unwrap_or(0))
}

/// A fixed integer mix (splitmix64), so every runtime picks alike.
const fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Milliseconds on the wire: a canonical decimal string, as every u64 is.
mod ms {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::DecimalU64;

    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        DecimalU64::new(*value).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        DecimalU64::deserialize(deserializer).map(DecimalU64::get)
    }
}

mod opt_ms {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::DecimalU64;

    #[allow(clippy::ref_option)]
    pub fn serialize<S: Serializer>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
        value.map(DecimalU64::new).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u64>, D::Error> {
        Option::<DecimalU64>::deserialize(deserializer).map(|value| value.map(DecimalU64::get))
    }
}

/// Applies one input to a stored drive at `now_ms` and the local `hour`.
///
/// `state` is the stored JSON, or the empty string for a fresh drive. The
/// answer is `{"ok":true,"state":…,"commands":[…]}` with the next state to
/// store, or `{"ok":false,"error":"invalid"}` for a malformed or newer state,
/// a malformed input, or an hour past 23. A refused input leaves the stored
/// state as it was.
#[must_use]
pub fn avaia_drive_step_wire(state: &str, input: &str, now_ms: u64, hour: u8) -> String {
    let failure = || serde_json::json!({ "ok": false, "error": "invalid" }).to_string();
    let current = if state.is_empty() {
        DriveState::new(now_ms)
    } else {
        match serde_json::from_str::<DriveState>(state) {
            Ok(current) => current,
            Err(_) => return failure(),
        }
    };
    let Ok(input) = serde_json::from_str::<DriveInput>(input) else {
        return failure();
    };
    match step(&current, input, now_ms, hour) {
        Ok((next, commands)) => {
            serde_json::json!({ "ok": true, "state": next, "commands": commands }).to_string()
        }
        Err(DriveError) => failure(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Action, Activity, CHOOSE_MS, CURIOSITY_IDLE_MS, Choice, DriveCommand, DriveInput,
        DriveKind, DriveRef, DriveState, FIRST_LOOK_MS, GLANCE_MS, Group, Home, Line,
        OUTING_INTERVAL_MS, POINT_B_STAND_MS, Passing, Purpose, RESTLESS_MS, Reach, Resolve,
        STROLL_IDLE_MS, STROLL_LEASH_METERS, STROLL_LOOK_MS, STROLL_MAX_METERS, STROLL_MIN_METERS,
        Stand, Target, VISIT_MS, avaia_drive_step_wire, next_due, restlessness, step,
    };

    const T0: u64 = 1_800_000_000_000;
    const NOON: u8 = 13;

    fn r(value: &str) -> DriveRef {
        DriveRef::new(value).expect("ref")
    }

    fn kind(value: &str) -> DriveKind {
        DriveKind::new(value).expect("kind")
    }

    /// A drive that never took the wheel, the way a fresh world opens.
    fn fresh() -> DriveState {
        DriveState::new(T0)
    }

    fn run(state: &DriveState, input: DriveInput, now: u64) -> (DriveState, Vec<DriveCommand>) {
        run_at(state, input, now, NOON)
    }

    fn run_at(
        state: &DriveState,
        input: DriveInput,
        now: u64,
        hour: u8,
    ) -> (DriveState, Vec<DriveCommand>) {
        step(state, input, now, hour).expect("a valid step")
    }

    fn wake(commands: &[DriveCommand]) -> Option<u64> {
        commands.iter().find_map(|command| match command {
            DriveCommand::WakeAt { ms } => Some(*ms),
            _ => None,
        })
    }

    fn without_wake(commands: Vec<DriveCommand>) -> Vec<DriveCommand> {
        commands
            .into_iter()
            .filter(|command| !matches!(command, DriveCommand::WakeAt { .. }))
            .collect()
    }

    fn walk(to: &str, purpose: Purpose) -> DriveCommand {
        DriveCommand::Walk {
            to: r(to),
            purpose,
            grass: purpose == Purpose::Tap,
        }
    }

    fn say(line: Line, about: Option<&str>) -> DriveCommand {
        DriveCommand::Say {
            line,
            about: about.map(r),
        }
    }

    /// Sent to `b`, arrived there and stood the stand out: settled at B.
    fn settled_at_b() -> (DriveState, u64) {
        let (state, _) = run(&fresh(), DriveInput::Tap { to: r("b") }, T0);
        let (state, _) = run(&state, DriveInput::Arrived { meters: 300 }, T0 + 200_000);
        let at = T0 + 200_000 + POINT_B_STAND_MS;
        let (state, _) = run(&state, DriveInput::Tick {}, at);
        (state, at)
    }

    fn passing(thing: &str, group: Group, off_route_m: u32) -> Passing {
        Passing {
            thing: r(thing),
            kind: kind(match group {
                Group::Find => "find",
                Group::Area => "lake",
                Group::Landmark => "monument",
            }),
            group,
            off_route_m,
            studyable: false,
        }
    }

    #[test]
    fn a_tap_walks_to_b_across_the_grass_and_stands_there_looking_around() {
        let (state, commands) = run(&fresh(), DriveInput::Tap { to: r("b") }, T0);
        assert_eq!(
            commands,
            vec![walk("b", Purpose::Tap), say(Line::Walk, None)]
        );

        let (state, commands) = run(&state, DriveInput::Arrived { meters: 300 }, T0 + 1);
        assert_eq!(
            state.activity,
            Activity::Standing {
                reason: Stand::PointB,
                at: r("b"),
                until_ms: T0 + 1 + POINT_B_STAND_MS,
            }
        );
        assert_eq!(
            commands,
            vec![
                DriveCommand::Look {
                    ms: POINT_B_STAND_MS
                },
                DriveCommand::WakeAt {
                    ms: T0 + 1 + POINT_B_STAND_MS
                },
            ]
        );
    }

    #[test]
    fn back_on_its_own_after_b_it_strolls_near_b_and_says_so_once() {
        let (state, settled) = settled_at_b();
        assert_eq!(state.settled_ms, settled);
        assert_eq!(state.anchor, Some(r("b")));

        // Curiosity first: nothing in the notebook to go and see.
        assert_eq!(
            wake(&run(&state, DriveInput::Tick {}, settled).1),
            Some(settled + CURIOSITY_IDLE_MS)
        );
        let (state, commands) = run(&state, DriveInput::Tick {}, settled + CURIOSITY_IDLE_MS);
        assert!(matches!(
            commands[0],
            DriveCommand::Resolve {
                what: Resolve::Curiosity,
                ..
            }
        ));
        let (state, commands) = run(
            &state,
            DriveInput::CuriosityOptions { to: Vec::new() },
            settled + CURIOSITY_IDLE_MS,
        );
        assert_eq!(wake(&commands), Some(settled + STROLL_IDLE_MS));

        // Then a stroll, kept near B.
        let at = settled + STROLL_IDLE_MS;
        let (state, commands) = run(&state, DriveInput::Tick {}, at);
        assert_eq!(
            commands[0],
            DriveCommand::Resolve {
                what: Resolve::Stroll,
                min_m: STROLL_MIN_METERS,
                max_m: STROLL_MAX_METERS,
                leash_m: Some(STROLL_LEASH_METERS),
                anchor: Some(r("b")),
                wander_m: None,
            }
        );
        let (state, commands) = run(
            &state,
            DriveInput::StrollOptions {
                to: vec![r("n1"), r("n2"), r("n3")],
            },
            at,
        );
        let DriveCommand::Walk { to, purpose, grass } = &commands[0] else {
            panic!("a stroll walks: {commands:?}");
        };
        assert_eq!(*purpose, Purpose::Stroll);
        assert!(!grass, "its own walks keep to the paths");
        assert!(["n1", "n2", "n3"].contains(&to.as_str()));
        assert_eq!(commands[1], say(Line::Stroll, None));
        assert_eq!(state.last_outing_ms, None, "a stroll is not an outing");

        let (state, commands) = run(&state, DriveInput::Arrived { meters: 80 }, at + 60_000);
        assert_eq!(commands[0], DriveCommand::Look { ms: STROLL_LOOK_MS });
        let back = at + 60_000 + STROLL_LOOK_MS;
        let (state, _) = run(&state, DriveInput::Tick {}, back);
        assert!(matches!(state.activity, Activity::Idle { since_ms } if since_ms == back));
        // Pottering about is not settling: restless all the same.
        assert_eq!(state.settled_ms, settled);
        assert_eq!(state.strolls, 1);
        assert!(restlessness(&state, back) > 0);

        // The next stroll waits twice as long, and says nothing.
        let (state, _) = run(&state, DriveInput::Tick {}, back + CURIOSITY_IDLE_MS);
        let (state, commands) = run(
            &state,
            DriveInput::CuriosityOptions { to: Vec::new() },
            back + CURIOSITY_IDLE_MS,
        );
        assert_eq!(wake(&commands), Some(back + STROLL_IDLE_MS * 2));
        let (state, _) = run(&state, DriveInput::Tick {}, back + STROLL_IDLE_MS * 2);
        let (_, commands) = run(
            &state,
            DriveInput::StrollOptions { to: vec![r("n1")] },
            back + STROLL_IDLE_MS * 2,
        );
        assert_eq!(without_wake(commands), vec![walk("n1", Purpose::Stroll)]);
    }

    #[test]
    fn a_stroll_with_nowhere_to_go_waits_longer_for_the_next() {
        let (state, settled) = settled_at_b();
        let state = DriveState {
            curiosity_asked: true,
            ..state
        };
        let at = settled + STROLL_IDLE_MS;
        let (state, _) = run(&state, DriveInput::Tick {}, at);
        let (state, commands) = run(&state, DriveInput::StrollOptions { to: Vec::new() }, at);
        assert_eq!(state.strolls, 1);
        assert!(matches!(state.activity, Activity::Idle { .. }));
        // Curiosity is asked again after a while, then the longer stroll wait.
        assert_eq!(wake(&commands), Some(at + CURIOSITY_IDLE_MS));
    }

    #[test]
    fn strolls_less_at_night_without_waking_the_host_early() {
        let (state, settled) = settled_at_b();
        let state = DriveState {
            curiosity_asked: true,
            ..state
        };
        assert_eq!(next_due(&state, 23), Some(settled + STROLL_IDLE_MS * 2));
        let (state, commands) = run_at(&state, DriveInput::Tick {}, settled + STROLL_IDLE_MS, 23);
        assert_eq!(
            without_wake(commands.clone()),
            Vec::new(),
            "not due yet at night"
        );
        assert_eq!(wake(&commands), Some(settled + STROLL_IDLE_MS * 2));
        assert!(state.pending.is_none());
    }

    #[test]
    fn a_tired_avaia_does_not_stroll() {
        let (state, settled) = settled_at_b();
        let state = DriveState {
            energy: 100,
            curiosity_asked: true,
            ..state
        };
        assert_eq!(next_due(&state, NOON), Some(settled + RESTLESS_MS));
    }

    #[test]
    fn restless_on_time_while_it_potters_and_out_before_another_stroll() {
        let (state, settled) = settled_at_b();
        let state = DriveState {
            strolls: 4,
            curiosity_asked: true,
            ..state
        };
        // The next stroll would come after the outing is due: the outing wins.
        let late = DriveState {
            activity: Activity::Idle {
                since_ms: settled + RESTLESS_MS - 10_000,
            },
            ..state
        };
        assert_eq!(next_due(&late, NOON), Some(settled + RESTLESS_MS));
        let (_, commands) = run(&late, DriveInput::Tick {}, settled + RESTLESS_MS);
        assert_eq!(
            commands[0],
            DriveCommand::Resolve {
                what: Resolve::Outing,
                min_m: 0,
                max_m: 2_350,
                leash_m: None,
                anchor: None,
                wander_m: Some([150, 400]),
            }
        );
    }

    fn target(thing: &str, kind_code: &str, meters: u32) -> Target {
        Target {
            thing: r(thing),
            kind: kind(kind_code),
            meters,
            appeal: None,
            feeling: None,
            stay_ms: None,
            revisit_ms: None,
        }
    }

    /// An idle drive that was just asked to resolve an outing at `at`.
    fn asked_out(state: DriveState, at: u64) -> DriveState {
        let state = DriveState {
            activity: Activity::Idle { since_ms: at },
            settled_ms: at - RESTLESS_MS,
            curiosity_asked: true,
            ..state
        };
        let (state, commands) = run(&state, DriveInput::Tick {}, at);
        assert!(matches!(
            commands[0],
            DriveCommand::Resolve {
                what: Resolve::Outing,
                ..
            }
        ));
        state
    }

    fn offer(
        state: &DriveState,
        targets: Vec<Target>,
        home: Option<u32>,
        at: u64,
        hour: u8,
    ) -> (DriveState, Vec<DriveCommand>) {
        run_at(
            state,
            DriveInput::OutingOptions {
                targets,
                wander: vec![r("w")],
                home: home.map(|meters| Home {
                    thing: r("home"),
                    meters,
                }),
            },
            at,
            hour,
        )
    }

    fn default_of(commands: &[DriveCommand]) -> (Vec<Action>, usize) {
        let Some(DriveCommand::Choose { menu, default, .. }) = commands.first() else {
            panic!("a choice: {commands:?}");
        };
        (menu.iter().map(|option| option.action).collect(), *default)
    }

    #[test]
    fn an_outing_is_a_menu_for_a_model_with_the_drives_own_pick() {
        let at = T0 + 1_000_000;
        let state = asked_out(fresh(), at);
        let (state, commands) = offer(
            &state,
            vec![
                target("park", "park", 1_500),
                target("museum", "museum", 800),
            ],
            Some(900),
            at,
            NOON,
        );
        let Some(DriveCommand::Choose {
            what,
            heading,
            menu,
            default,
        }) = commands.first()
        else {
            panic!("a choice: {commands:?}");
        };
        assert_eq!(*what, Choice::Outing);
        assert_eq!(*heading, None);
        // What a model reads: no refs, no names, no metres.
        let json = serde_json::to_string(menu).expect("menu");
        assert_eq!(
            json,
            r#"[{"index":0,"action":"stay"},{"index":1,"action":"go","kind":"park","reach":"far"},{"index":2,"action":"go","kind":"museum","reach":"near"},{"index":3,"action":"wander"},{"index":4,"action":"home","reach":"near"}]"#
        );
        assert_eq!(*default, 2, "the nearest target");
        assert_eq!(wake(&commands), Some(at + CHOOSE_MS));

        // No model answered: the drive's pick stands.
        let (state, commands) = run(&state, DriveInput::Chosen { index: None }, at + 10);
        assert_eq!(
            without_wake(commands),
            vec![walk("museum", Purpose::Outing), say(Line::Walk, None)]
        );
        assert_eq!(state.last_outing_ms, Some(at + 10));

        let (state, commands) = run(&state, DriveInput::Arrived { meters: 800 }, at + 600_000);
        assert_eq!(commands[0], DriveCommand::Look { ms: VISIT_MS });
        let over = at + 600_000 + VISIT_MS;
        let (state, commands) = run(&state, DriveInput::Tick {}, over);
        assert_eq!(commands[0], DriveCommand::Visited { at: r("museum") });
        assert_eq!(state.anchor, Some(r("museum")));
        assert_eq!(state.settled_ms, over);
        assert_eq!(
            next_due(&state, NOON).map(|due| due - over),
            Some(CURIOSITY_IDLE_MS)
        );
    }

    #[test]
    fn a_models_pick_is_carried_out_and_anything_off_the_menu_is_not() {
        let at = T0 + 1_000_000;
        let state = asked_out(fresh(), at);
        let targets = vec![
            target("park", "park", 1_500),
            target("museum", "museum", 800),
        ];
        let (offered, _) = offer(&state, targets, None, at, NOON);

        let (_, commands) = run(&offered, DriveInput::Chosen { index: Some(1) }, at);
        assert_eq!(commands[0], walk("park", Purpose::Outing));
        let (_, commands) = run(&offered, DriveInput::Chosen { index: Some(9) }, at);
        assert_eq!(commands[0], walk("museum", Purpose::Outing));
        let (stayed, commands) = run(&offered, DriveInput::Chosen { index: Some(0) }, at);
        assert_eq!(without_wake(commands), Vec::new());
        assert_eq!(stayed.last_outing_ms, Some(at));
        // Never on the wheel before: it looks around first, then is curious.
        assert_eq!(next_due(&stayed, NOON), Some(at + FIRST_LOOK_MS));

        // A model that never answers is not waited on for ever.
        let (_, commands) = run(&offered, DriveInput::Tick {}, at + CHOOSE_MS);
        assert_eq!(commands[0], walk("museum", Purpose::Outing));
    }

    #[test]
    fn the_rule_keeps_near_at_night_goes_home_tired_and_follows_appeal() {
        let at = T0 + 1_000_000;
        let targets = || {
            vec![
                target("far", "park", 1_500),
                target("museum", "museum", 800),
            ]
        };
        let state = asked_out(fresh(), at);

        let (_, night) = offer(&state, vec![target("far", "park", 1_500)], None, at, 22);
        assert_eq!(
            default_of(&night),
            (vec![Action::Stay, Action::Go, Action::Wander], 2)
        );

        let tired = asked_out(
            DriveState {
                energy: 200,
                ..fresh()
            },
            at,
        );
        let (_, commands) = offer(&tired, targets(), Some(900), at, NOON);
        assert_eq!(default_of(&commands).1, 4);
        // Tired but already home: no home entry at all.
        let (_, commands) = offer(&tired, targets(), Some(20), at, NOON);
        assert_eq!(
            default_of(&commands).0,
            vec![Action::Stay, Action::Go, Action::Go, Action::Wander]
        );

        let mut wanted = targets();
        wanted[0].appeal = Some(800);
        wanted[1].appeal = Some(300);
        let (_, commands) = offer(&state, wanted, None, at, NOON);
        assert_eq!(default_of(&commands).1, 1);
    }

    #[test]
    fn a_target_visited_lately_stays_off_the_menu_unless_it_is_dear() {
        let at = T0 + 1_000_000;
        let mut visited = fresh();
        visited
            .visited
            .insert(r("museum"), crate::DecimalU64::new(at - 2 * 24 * 3_600_000));
        let state = asked_out(visited, at);
        let (_, commands) = offer(
            &state,
            vec![target("museum", "museum", 800)],
            None,
            at,
            NOON,
        );
        assert_eq!(default_of(&commands).0, vec![Action::Stay, Action::Wander]);

        let mut loved = target("museum", "museum", 800);
        loved.revisit_ms = Some(24 * 3_600_000);
        let (_, commands) = offer(&state, vec![loved], None, at, NOON);
        assert_eq!(
            default_of(&commands).0,
            vec![Action::Stay, Action::Go, Action::Wander]
        );
    }

    #[test]
    fn an_outing_to_a_place_it_misses_says_so() {
        let at = T0 + 1_000_000;
        let state = asked_out(fresh(), at);
        let mut dear = target("lake", "lake", 600);
        dear.feeling = Some(super::Feeling::Loved);
        let (offered, _) = offer(&state, vec![dear], None, at, NOON);
        let (_, commands) = run(&offered, DriveInput::Chosen { index: Some(1) }, at);
        assert_eq!(commands[1], say(Line::LandmarkLonging, Some("lake")));
    }

    #[test]
    fn nothing_to_offer_is_staying_without_asking() {
        let at = T0 + 1_000_000;
        let state = asked_out(fresh(), at);
        let (state, commands) = run(
            &state,
            DriveInput::OutingOptions {
                targets: Vec::new(),
                wander: Vec::new(),
                home: None,
            },
            at,
        );
        assert_eq!(without_wake(commands), Vec::new());
        assert_eq!(state.last_outing_ms, Some(at));
        assert_eq!(
            next_due(&state, NOON).map(|due| due >= at + OUTING_INTERVAL_MS),
            Some(false),
            "curiosity and strolls go on meanwhile"
        );
    }

    /// On the way to B, 100 m out.
    fn walking_to_b() -> DriveState {
        run(&fresh(), DriveInput::Tap { to: r("b") }, T0).0
    }

    #[test]
    fn on_the_way_to_b_it_steps_aside_to_look_and_carries_on_to_b() {
        let state = walking_to_b();
        let (state, commands) = run(
            &state,
            DriveInput::Passing {
                things: vec![passing("statue", Group::Landmark, 12)],
            },
            T0 + 60_000,
        );
        let Some(DriveCommand::Choose {
            what,
            heading,
            menu,
            default,
        }) = commands.first()
        else {
            panic!("a choice: {commands:?}");
        };
        assert_eq!(*what, Choice::Distraction);
        assert_eq!(*heading, Some(Purpose::Tap));
        assert_eq!(
            serde_json::to_string(menu).expect("menu"),
            r#"[{"index":0,"action":"carry_on"},{"index":1,"action":"glance","kind":"monument","reach":"near"}]"#
        );
        assert_eq!(*default, 1);

        let (state, commands) = run(&state, DriveInput::Chosen { index: None }, T0 + 61_000);
        assert_eq!(
            without_wake(commands),
            vec![
                walk("statue", Purpose::Detour),
                say(Line::LandmarkSpotted, Some("statue")),
            ]
        );
        let (state, commands) = run(&state, DriveInput::Arrived { meters: 12 }, T0 + 70_000);
        assert_eq!(commands[0], DriveCommand::Glance { at: r("statue") });
        assert_eq!(wake(&commands), Some(T0 + 70_000 + GLANCE_MS));

        let (state, commands) = run(&state, DriveInput::Tick {}, T0 + 70_000 + GLANCE_MS);
        assert_eq!(
            without_wake(commands),
            vec![walk("b", Purpose::Tap)],
            "back on the way to B, saying nothing"
        );
        let (state, _) = run(&state, DriveInput::Arrived { meters: 250 }, T0 + 300_000);
        assert!(matches!(
            state.activity,
            Activity::Standing {
                reason: Stand::PointB,
                ..
            }
        ));
    }

    #[test]
    fn a_find_on_the_way_is_picked_up_and_a_noticed_landmark_studied() {
        let state = walking_to_b();
        let (state, _) = run(
            &state,
            DriveInput::Passing {
                things: vec![
                    passing("statue", Group::Landmark, 10),
                    passing("art:1", Group::Find, 8),
                ],
            },
            T0 + 1,
        );
        let (state, commands) = run(&state, DriveInput::Chosen { index: Some(1) }, T0 + 2);
        assert_eq!(without_wake(commands), vec![walk("art:1", Purpose::Detour)]);
        let (state, commands) = run(&state, DriveInput::Arrived { meters: 8 }, T0 + 3);
        assert_eq!(commands[0], DriveCommand::PickUp { at: r("art:1") });
        let (state, _) = run(&state, DriveInput::Tick {}, T0 + 3 + super::PICK_UP_MS);

        let mut noticed = passing("statue", Group::Landmark, 10);
        noticed.studyable = true;
        let (state, _) = run(
            &state,
            DriveInput::Passing {
                things: vec![noticed],
            },
            T0 + 10_000,
        );
        let (state, _) = run(&state, DriveInput::Chosen { index: None }, T0 + 10_001);
        let (_, commands) = run(&state, DriveInput::Arrived { meters: 10 }, T0 + 10_002);
        assert_eq!(commands[0], DriveCommand::Study { at: r("statue") });
    }

    #[test]
    fn a_model_may_keep_it_on_its_way() {
        let state = walking_to_b();
        let (state, _) = run(
            &state,
            DriveInput::Passing {
                things: vec![passing("lake", Group::Area, 20)],
            },
            T0 + 1,
        );
        let (state, commands) = run(&state, DriveInput::Chosen { index: Some(0) }, T0 + 2);
        assert_eq!(without_wake(commands), Vec::new());
        assert!(matches!(
            state.activity,
            Activity::Walking {
                purpose: Purpose::Tap,
                ..
            }
        ));
        // Seen once is enough: passing it again asks nothing.
        let (_, commands) = run(
            &state,
            DriveInput::Passing {
                things: vec![passing("lake", Group::Area, 5)],
            },
            T0 + 3,
        );
        assert_eq!(without_wake(commands), Vec::new());
    }

    #[test]
    fn distractions_are_few_near_and_never_on_the_way_home() {
        let mut state = walking_to_b();
        for (index, thing) in ["a", "b2", "c"].iter().enumerate() {
            let at = T0 + 1_000 * u64::try_from(index).expect("small");
            let (next, commands) = run(
                &state,
                DriveInput::Passing {
                    things: vec![passing(thing, Group::Landmark, 5)],
                },
                at,
            );
            if index < 2 {
                let (next, _) = run(&next, DriveInput::Chosen { index: None }, at);
                let (next, _) = run(&next, DriveInput::Arrived { meters: 5 }, at);
                state = run(&next, DriveInput::Tick {}, at + GLANCE_MS).0;
            } else {
                assert_eq!(without_wake(commands), Vec::new(), "two per walk");
                state = next;
            }
        }

        let (_, commands) = run(
            &walking_to_b(),
            DriveInput::Passing {
                things: vec![
                    passing("far", Group::Landmark, 40),
                    passing("art", Group::Find, 20),
                ],
            },
            T0,
        );
        assert_eq!(without_wake(commands), Vec::new(), "too far off the way");

        let home = DriveState {
            activity: Activity::Walking {
                purpose: Purpose::Home,
                to: r("home"),
                since_ms: T0,
            },
            walk: Some(super::WalkMemory::default()),
            ..fresh()
        };
        let (_, commands) = run(
            &home,
            DriveInput::Passing {
                things: vec![passing("statue", Group::Landmark, 5)],
            },
            T0,
        );
        assert_eq!(without_wake(commands), Vec::new(), "tired, going home");
    }

    #[test]
    fn tired_it_carries_on_past_a_sight_but_still_bends_for_a_find() {
        let state = DriveState {
            energy: 100,
            ..walking_to_b()
        };
        let (_, commands) = run(
            &state,
            DriveInput::Passing {
                things: vec![passing("statue", Group::Landmark, 5)],
            },
            T0,
        );
        assert_eq!(default_of(&commands).1, 0);
        let (_, commands) = run(
            &state,
            DriveInput::Passing {
                things: vec![passing("art", Group::Find, 5)],
            },
            T0,
        );
        assert_eq!(default_of(&commands).1, 1);
    }

    #[test]
    fn arriving_while_a_distraction_is_weighed_drops_it() {
        let state = walking_to_b();
        let (state, _) = run(
            &state,
            DriveInput::Passing {
                things: vec![passing("statue", Group::Landmark, 5)],
            },
            T0,
        );
        let (state, _) = run(&state, DriveInput::Arrived { meters: 100 }, T0 + 1);
        assert!(state.pending.is_none());
        let (_, commands) = run(&state, DriveInput::Chosen { index: Some(1) }, T0 + 2);
        assert_eq!(without_wake(commands).len(), 0);
    }

    #[test]
    fn a_tap_with_no_way_there_says_why_and_settles() {
        let state = walking_to_b();
        let (state, commands) = run(
            &state,
            DriveInput::Blocked {
                by: Some(super::Obstacle::Building),
            },
            T0 + 1,
        );
        assert_eq!(commands[0], say(Line::BlockedBuilding, None));
        assert!(matches!(state.activity, Activity::Idle { .. }));
        assert_eq!(state.settled_ms, T0 + 1);
    }

    #[test]
    fn a_tap_outranks_everything() {
        let (state, settled) = settled_at_b();
        let at = settled + STROLL_IDLE_MS;
        let (state, _) = run(
            &DriveState {
                curiosity_asked: true,
                ..state
            },
            DriveInput::Tick {},
            at,
        );
        assert!(state.pending.is_some());
        let (state, commands) = run(&state, DriveInput::Tap { to: r("c") }, at + 1);
        assert_eq!(commands[0], walk("c", Purpose::Tap));
        assert!(state.pending.is_none());
        // The stroll's answer, arriving late, is not taken.
        let (state, commands) = run(
            &state,
            DriveInput::StrollOptions { to: vec![r("n")] },
            at + 2,
        );
        assert_eq!(without_wake(commands), Vec::new());
        assert!(matches!(
            state.activity,
            Activity::Walking {
                purpose: Purpose::Tap,
                ..
            }
        ));
    }

    #[test]
    fn taking_the_wheel_looks_around_first_then_curiosity_goes() {
        let (state, commands) = run(&fresh(), DriveInput::Stopped {}, T0);
        assert_eq!(wake(&commands), Some(T0 + FIRST_LOOK_MS));
        let (state, _) = run(&state, DriveInput::Tick {}, T0 + FIRST_LOOK_MS);
        let (state, commands) = run(
            &state,
            DriveInput::CuriosityOptions {
                to: vec![super::Curious {
                    thing: r("statue"),
                    longing: false,
                }],
            },
            T0 + FIRST_LOOK_MS,
        );
        assert_eq!(
            without_wake(commands),
            vec![
                walk("statue", Purpose::Curiosity),
                say(Line::LandmarkSpotted, Some("statue")),
            ]
        );
        let (_, commands) = run(&state, DriveInput::Arrived { meters: 40 }, T0 + 60_000);
        assert_eq!(commands[0], DriveCommand::Study { at: r("statue") });
    }

    #[test]
    fn a_world_layer_that_never_answers_is_not_waited_on() {
        let (state, settled) = settled_at_b();
        let (state, _) = run(&state, DriveInput::Tick {}, settled + CURIOSITY_IDLE_MS);
        assert_eq!(
            next_due(&state, NOON),
            Some(settled + CURIOSITY_IDLE_MS + super::RESOLVE_MS)
        );
        let (state, _) = run(
            &state,
            DriveInput::Tick {},
            settled + CURIOSITY_IDLE_MS + super::RESOLVE_MS,
        );
        assert!(
            state.pending.is_none()
                || matches!(
                    state.pending,
                    Some(super::Pending::Resolve {
                        what: Resolve::Stroll,
                        ..
                    })
                )
        );
    }

    #[test]
    fn the_same_inputs_give_the_same_drive() {
        let play = || {
            let (state, _) = settled_at_b();
            let at = state.settled_ms + STROLL_IDLE_MS;
            let state = DriveState {
                curiosity_asked: true,
                ..state
            };
            let (state, _) = run(&state, DriveInput::Tick {}, at);
            run(
                &state,
                DriveInput::StrollOptions {
                    to: (0..10).map(|n| r(&format!("n{n}"))).collect(),
                },
                at,
            )
        };
        assert_eq!(play(), play());
    }

    #[test]
    fn the_wire_round_trips_and_refuses_what_it_cannot_read() {
        let answer: serde_json::Value = serde_json::from_str(&avaia_drive_step_wire(
            "",
            r#"{"type":"tap","to":"b:1"}"#,
            T0,
            NOON,
        ))
        .expect("json");
        assert_eq!(answer["ok"], true);
        assert_eq!(answer["state"]["settled_ms"], T0.to_string());
        assert_eq!(
            answer["commands"][0],
            serde_json::json!({"do":"walk","to":"b:1","purpose":"tap","grass":true})
        );
        let stored = answer["state"].to_string();
        let again: serde_json::Value = serde_json::from_str(&avaia_drive_step_wire(
            &stored,
            r#"{"type":"arrived","meters":120}"#,
            T0 + 90_000,
            NOON,
        ))
        .expect("json");
        assert_eq!(again["state"]["activity"]["kind"], "standing");
        assert_eq!(again["state"]["energy"], 976);

        let invalid = r#"{"error":"invalid","ok":false}"#;
        for (state, input, hour) in [
            ("", r#"{"type":"tap","to":""}"#, NOON),
            ("", r#"{"type":"fly"}"#, NOON),
            ("", r#"{"type":"tick","extra":1}"#, NOON),
            ("", r#"{"type":"tick"}"#, 24),
            ("{", r#"{"type":"tick"}"#, NOON),
        ] {
            assert_eq!(avaia_drive_step_wire(state, input, T0, hour), invalid);
        }
        let newer = stored.replace("\"version\":1", "\"version\":2");
        assert_eq!(
            avaia_drive_step_wire(&newer, r#"{"type":"tick"}"#, T0, NOON),
            invalid
        );
    }

    #[test]
    fn options_read_from_the_wire_and_a_pending_choice_survives_storage() {
        let at = T0 + 1_000_000;
        let state = asked_out(fresh(), at);
        let stored = serde_json::to_string(&state).expect("state");
        let answer: serde_json::Value = serde_json::from_str(&avaia_drive_step_wire(
            &stored,
            r#"{"type":"outing_options","targets":[{"ref":"poi:1","kind":"museum","meters":800},{"ref":"poi:2","kind":"park","meters":400,"appeal":900,"feeling":"fond","stay_ms":"90000","revisit_ms":"86400000"}],"wander":["node:7"]}"#,
            at,
            NOON,
        ))
        .expect("json");
        assert_eq!(answer["ok"], true, "{answer}");
        assert_eq!(answer["commands"][0]["default"], 2);
        let pending = answer["state"].to_string();
        let restored: DriveState = serde_json::from_str(&pending).expect("stored state reads back");
        assert_eq!(
            serde_json::to_value(&restored).expect("state"),
            answer["state"]
        );

        let chosen: serde_json::Value = serde_json::from_str(&avaia_drive_step_wire(
            &pending,
            r#"{"type":"chosen","index":2}"#,
            at + 1,
            NOON,
        ))
        .expect("json");
        assert_eq!(
            chosen["commands"][1],
            serde_json::json!({"do":"say","line":"landmark.longing","about":"poi:2"})
        );
        assert_eq!(chosen["state"]["walk"]["stay_ms"], "90000");
    }

    #[test]
    fn refs_and_kinds_are_shaped_not_read() {
        assert!(DriveRef::new("  ").is_err());
        assert!(DriveRef::new("a\nb").is_err());
        assert!(DriveRef::new("x".repeat(129)).is_err());
        assert!(DriveRef::new("старе місто").is_ok());
        assert!(DriveKind::new("major_monument").is_ok());
        assert!(DriveKind::new("Monument").is_err());
        assert!(DriveKind::new("_x").is_err());
        assert_eq!(Reach::Near, super::reach(1_000));
    }
}
