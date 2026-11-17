// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Per-entity simulator selection from a host's current view of nearby runtimes.

/// A runtime's eligibility for this entity. Distance is computed from the
/// entity's position in the world, regardless of where the runtime executes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub runtime_id: u64,
    pub distance_mm: u64,
    pub active: bool,
    /// True when this runtime can accept another entity.
    pub has_capacity: bool,
}

/// Direction relative to this entity, inferred from a recent world trajectory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Approach {
    /// The predicted path enters the simulation radius.
    Toward,
    /// Remains near enough to be a standby candidate.
    Stationary,
    /// Unknown, or moving past without entering the radius.
    Passing,
    /// Moving farther from the entity.
    Away,
}

/// Classifies a bounded predicted path using local offsets from the entity,
/// in millimetres. An absent prediction is treated as passing.
#[must_use]
pub fn classify_approach(
    current: [i32; 2],
    predicted: Option<[i32; 2]>,
    active_radius_mm: u64,
) -> Approach {
    let Some(end) = predicted else {
        return Approach::Passing;
    };
    if end == current {
        return Approach::Stationary;
    }
    let [x, y] = current.map(i128::from);
    let [end_x, end_y] = end.map(i128::from);
    let (dx, dy) = (end_x - x, end_y - y);
    let dot = x * dx + y * dy;
    if dot > 0 {
        return Approach::Away;
    }
    if dot == 0 {
        return Approach::Passing;
    }
    let length_squared = (dx * dx + dy * dy).unsigned_abs();
    let radius_squared = u128::from(active_radius_mm).pow(2);
    let intersects = if dot.unsigned_abs() >= length_squared {
        (end_x * end_x + end_y * end_y).unsigned_abs() <= radius_squared
    } else {
        let cross_squared = (x * dy - y * dx).unsigned_abs().pow(2);
        radius_squared.checked_mul(length_squared).is_none_or(|bound| cross_squared <= bound)
    };
    if intersects {
        Approach::Toward
    } else {
        Approach::Passing
    }
}

/// A nearby runtime and its trajectory relative to the entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackedCandidate {
    pub runtime: Candidate,
    pub approach: Approach,
}

/// Assignments for an entity. With neither assignment the entity can sleep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimulationPlan {
    pub simulator: Option<u64>,
    /// May prepare a snapshot for a future transfer. Has no simulation authority.
    pub standby: Option<u64>,
}

/// Use the inner radius for active simulation and the outer radius for standby.
/// The outer limit is exclusive. Within standby, a runtime on course to enter
/// the inner radius outranks a stationary runtime. Departing runtimes are a
/// fallback in case their trajectory changes. Passing paths are skipped.
#[must_use]
pub fn plan_simulation(
    candidates: &[TrackedCandidate],
    current: SimulationPlan,
    active_radius_mm: u64,
    standby_radius_mm: u64,
    switch_margin_mm: u64,
) -> SimulationPlan {
    let simulator = select_simulator_from(
        candidates.iter().map(|tracked| &tracked.runtime),
        current.simulator,
        active_radius_mm,
        switch_margin_mm,
    );
    let eligible_standby = candidates
        .iter()
        .filter(|tracked| {
            let runtime = tracked.runtime;
            runtime.active
                && runtime.has_capacity
                && Some(runtime.runtime_id) != simulator
                && runtime.distance_mm > active_radius_mm
                && runtime.distance_mm < standby_radius_mm
                && tracked.approach != Approach::Passing
        });
    let rank = |approach| match approach {
        Approach::Toward => 0,
        Approach::Stationary => 1,
        Approach::Away => 2,
        Approach::Passing => 3,
    };
    let incumbent = eligible_standby.clone().find(|tracked| Some(tracked.runtime.runtime_id) == current.standby);
    let best = eligible_standby.min_by_key(|tracked| {
        (rank(tracked.approach), tracked.runtime.distance_mm, tracked.runtime.runtime_id)
    });
    let standby = match (incumbent, best) {
        (Some(previous), Some(next))
            if rank(previous.approach) == rank(next.approach)
                && previous.runtime.distance_mm.saturating_sub(next.runtime.distance_mm) <= switch_margin_mm =>
        {
            Some(previous.runtime.runtime_id)
        }
        (_, next) => next.map(|tracked| tracked.runtime.runtime_id),
    };
    SimulationPlan { simulator, standby }
}

/// Selects a simulator within range. An existing eligible simulator retains
/// its assignment through the exit margin unless a challenger is closer by
/// more than the margin.
/// Equal-distance challengers are ordered by runtime ID.
///
/// Candidates must represent distinct runtime IDs. Each host must measure
/// liveness and capacity before calling this function.
#[must_use]
pub fn select_simulator(
    candidates: &[Candidate],
    current: Option<u64>,
    range_mm: u64,
    switch_margin_mm: u64,
) -> Option<u64> {
    select_simulator_from(candidates.iter(), current, range_mm, switch_margin_mm)
}

fn select_simulator_from<'a>(
    candidates: impl Iterator<Item = &'a Candidate> + Clone,
    current: Option<u64>,
    range_mm: u64,
    switch_margin_mm: u64,
) -> Option<u64> {
    let eligible = candidates.filter(|candidate| candidate.active);
    let incumbent = eligible
        .clone()
        .find(|candidate| Some(candidate.runtime_id) == current && candidate.distance_mm <= range_mm.saturating_add(switch_margin_mm));
    let challenger = eligible
        .filter(|candidate| candidate.has_capacity && candidate.distance_mm <= range_mm && Some(candidate.runtime_id) != current)
        .min_by_key(|candidate| (candidate.distance_mm, candidate.runtime_id));

    match (incumbent, challenger) {
        (Some(owner), Some(next))
            if owner.distance_mm.saturating_sub(next.distance_mm) > switch_margin_mm =>
        {
            Some(next.runtime_id)
        }
        (Some(owner), _) => Some(owner.runtime_id),
        (None, candidate) => candidate.map(|next| next.runtime_id),
    }
}

/// A committed assignment. Epochs are scoped to a single entity and allocated
/// by the coordination layer, which must agree on assignments before applying them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub epoch: u64,
    pub simulator: Option<u64>,
}

/// A final snapshot from the authenticated outgoing simulator.
pub struct FinalSnapshot<T> {
    pub sender: u64,
    pub epoch: u64,
    pub sequence: u64,
    pub snapshot: T,
}

/// Outcome of applying an assignment from the coordination layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentOutcome {
    Applied,
    Replay,
    Stale,
    Conflict,
    InvalidFinalSnapshot,
}

/// Holds the latest accepted state of one procedural entity across handoffs.
/// The payload can represent animal behaviour or any other simulated state.
#[derive(Debug)]
pub struct EntityState<T> {
    assignment: Assignment,
    sequence: Option<u64>,
    snapshot: T,
}

impl<T> EntityState<T> {
    /// Creates state from the initial committed assignment and snapshot.
    #[must_use]
    pub const fn new(assignment: Assignment, snapshot: T) -> Self {
        Self {
            assignment,
            sequence: None,
            snapshot,
        }
    }

    /// Returns the snapshot a newly assigned simulator should resume from.
    #[must_use]
    pub const fn snapshot(&self) -> &T {
        &self.snapshot
    }

    #[must_use]
    pub const fn assignment(&self) -> Assignment {
        self.assignment
    }

    #[must_use]
    pub const fn sequence(&self) -> Option<u64> {
        self.sequence
    }

    /// Applies a committed assignment and optionally the outgoing simulator's
    /// final snapshot atomically. If the outgoing simulator was lost, callers
    /// can omit the final snapshot and resume from the latest known state.
    pub fn assign(
        &mut self,
        assignment: Assignment,
        final_snapshot: Option<FinalSnapshot<T>>,
    ) -> AssignmentOutcome {
        match assignment.epoch.cmp(&self.assignment.epoch) {
            core::cmp::Ordering::Less => return AssignmentOutcome::Stale,
            core::cmp::Ordering::Equal if assignment == self.assignment => {
                return AssignmentOutcome::Replay;
            }
            core::cmp::Ordering::Equal => return AssignmentOutcome::Conflict,
            core::cmp::Ordering::Greater => {}
        }
        if let Some(final_snapshot) = final_snapshot {
            if self.assignment.simulator != Some(final_snapshot.sender)
                || self.assignment.epoch != final_snapshot.epoch
            {
                return AssignmentOutcome::InvalidFinalSnapshot;
            }
            if self.sequence.is_none_or(|sequence| final_snapshot.sequence > sequence) {
                self.snapshot = final_snapshot.snapshot;
            }
        }
        self.assignment = assignment;
        self.sequence = None;
        AssignmentOutcome::Applied
    }

    /// Accepts a complete snapshot only from the authenticated simulator and
    /// only when its epoch and sequence match the current assignment's stream.
    /// Callers must route snapshots by entity ID before applying them.
    pub fn update(&mut self, sender: u64, epoch: u64, sequence: u64, snapshot: T) -> bool {
        if self.assignment.simulator != Some(sender)
            || self.assignment.epoch != epoch
            || self.sequence.is_some_and(|previous| sequence <= previous)
        {
            return false;
        }
        self.sequence = Some(sequence);
        self.snapshot = snapshot;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(runtime_id: u64, distance_mm: u64) -> Candidate {
        Candidate {
            runtime_id,
            distance_mm,
            active: true,
            has_capacity: true,
        }
    }

    #[test]
    fn trajectory_handles_exact_boundaries_and_stationary_predictions() {
        assert_eq!(classify_approach([10, 0], None, 3), Approach::Passing);
        assert_eq!(classify_approach([10, 0], Some([10, 0]), 3), Approach::Stationary);
        assert_eq!(classify_approach([10, 0], Some([10, 1]), 3), Approach::Passing);
        assert_eq!(classify_approach([10, 0], Some([3, 0]), 3), Approach::Toward);
        assert_eq!(classify_approach([10, 3], Some([-10, 3]), 3), Approach::Toward);
        assert_eq!(classify_approach([10, 3], Some([-10, 3]), 2), Approach::Passing);
        assert_eq!(
            classify_approach([i32::MIN; 2], Some([i32::MAX; 2]), u64::MAX),
            Approach::Toward
        );
    }

    #[test]
    fn retains_assignments_under_small_distance_changes() {
        let current = SimulationPlan {
            simulator: Some(1),
            standby: Some(2),
        };
        let candidates = [
            TrackedCandidate { runtime: candidate(1, 101), approach: Approach::Passing },
            TrackedCandidate { runtime: candidate(2, 150), approach: Approach::Stationary },
            TrackedCandidate { runtime: candidate(3, 145), approach: Approach::Stationary },
            TrackedCandidate { runtime: candidate(4, 120), approach: Approach::Away },
        ];
        assert_eq!(plan_simulation(&candidates, current, 100, 200, 10), current);
        assert_eq!(select_simulator(&[candidate(1, 111)], Some(1), 100, 10), None);
    }

    #[test]
    fn approaching_standby_outranks_a_nearer_departing_runtime() {
        let track = |id, metres: u64, approach| TrackedCandidate {
            runtime: candidate(id, metres * 1_000),
            approach,
        };
        let away = track(
            1,
            750,
            classify_approach([750_000, 0], Some([900_000, 0]), 100_000),
        );
        let toward = track(
            2,
            950,
            classify_approach([950_000, 0], Some([-200_000, 0]), 100_000),
        );
        let passing = track(
            3,
            990,
            classify_approach([990_000, 0], Some([990_000, 400_000]), 100_000),
        );
        let outer_limit = track(4, 1_000, Approach::Toward);
        let nearby = track(5, 30, Approach::Passing);
        let plan = |candidates: &[TrackedCandidate]| {
            plan_simulation(candidates, SimulationPlan { simulator: None, standby: None }, 100_000, 1_000_000, 5_000)
        };
        assert_eq!(
            plan(&[away, toward, passing, outer_limit]),
            SimulationPlan {
                simulator: None,
                standby: Some(2)
            }
        );
        assert_eq!(
            plan(&[nearby, away, toward]),
            SimulationPlan {
                simulator: Some(5),
                standby: Some(2)
            }
        );
        assert_eq!(
            plan(&[passing, outer_limit]),
            SimulationPlan {
                simulator: None,
                standby: None
            }
        );
    }

    #[test]
    fn selects_per_entity_and_breaks_ties_consistently() {
        assert_eq!(
            select_simulator(&[candidate(1, 40), candidate(2, 50)], None, 100, 10),
            Some(1)
        );
        assert_eq!(
            select_simulator(&[candidate(1, 50), candidate(2, 40)], None, 100, 10),
            Some(2)
        );
        assert_eq!(
            select_simulator(&[candidate(2, 40), candidate(1, 40)], None, 100, 10),
            Some(1)
        );
    }

    #[test]
    fn only_hands_off_for_a_clear_improvement_or_an_inactive_owner() {
        let mut owner = candidate(1, 50);
        owner.has_capacity = false;
        assert_eq!(
            select_simulator(&[owner, candidate(2, 40)], Some(1), 100, 10),
            Some(1)
        );
        assert_eq!(
            select_simulator(&[owner, candidate(2, 39)], Some(1), 100, 10),
            Some(2)
        );
        owner.active = false;
        assert_eq!(
            select_simulator(&[owner, candidate(2, 45)], Some(1), 100, 10),
            Some(2)
        );
        let mut busy = candidate(3, 10);
        busy.has_capacity = false;
        assert_eq!(
            select_simulator(&[owner, busy, candidate(2, 111)], Some(1), 100, 10),
            None
        );
    }
}

#[cfg(test)]
mod handoff_tests {
    use super::{Assignment, AssignmentOutcome, EntityState, FinalSnapshot};

    #[test]
    fn preserves_final_state_and_fences_old_updates() {
        let assignment = |epoch, simulator| Assignment { epoch, simulator };
        let initial = assignment(1, Some(10));
        let awake = assignment(3, Some(20));
        let mut entity = EntityState::new(initial, "idle");
        assert!(entity.update(10, 1, 4, "walking"));
        let final_snapshot = FinalSnapshot {
            sender: 10,
            epoch: 1,
            sequence: 5,
            snapshot: "running",
        };
        assert_eq!(
            entity.assign(awake, Some(FinalSnapshot { sender: 99, ..final_snapshot })),
            AssignmentOutcome::InvalidFinalSnapshot
        );
        assert_eq!(entity.assignment(), initial);
        assert_eq!(entity.sequence(), Some(4));
        let sleep = assignment(2, None);
        assert_eq!(entity.assign(sleep, Some(final_snapshot)), AssignmentOutcome::Applied);
        assert_eq!(entity.snapshot(), &"running");
        assert!(!entity.update(10, 2, 0, "sleep update"));
        assert_eq!(entity.assign(awake, None), AssignmentOutcome::Applied);
        assert_eq!(entity.assign(awake, None), AssignmentOutcome::Replay);
        assert_eq!(entity.assign(assignment(3, Some(10)), None), AssignmentOutcome::Conflict);
        assert_eq!(entity.assign(sleep, None), AssignmentOutcome::Stale);
        assert!(!entity.update(10, 1, 6, "old owner"));
        assert_eq!(entity.snapshot(), &"running");
        assert!(entity.update(20, 3, 0, "jumping"));
        assert!(!entity.update(20, 3, 0, "duplicate"));
    }
}
