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
    /// Stationary, unknown, or moving past without entering the radius.
    Passing,
    /// Moving farther from the entity.
    Away,
}

/// Classifies a bounded predicted path using local offsets from the entity,
/// in millimetres. An absent or stationary prediction is treated as passing.
#[must_use]
pub fn classify_approach(
    current: [i32; 2],
    predicted: Option<[i32; 2]>,
    active_radius_mm: u32,
) -> Approach {
    let Some(end) = predicted.filter(|end| *end != current) else {
        return Approach::Passing;
    };
    let [x, y] = current.map(f64::from);
    let [end_x, end_y] = end.map(f64::from);
    let (dx, dy) = (end_x - x, end_y - y);
    let dot = x * dx + y * dy;
    if dot > 0.0 {
        return Approach::Away;
    }
    let fraction = (-dot / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    if dot < 0.0 && (x + fraction * dx).hypot(y + fraction * dy) <= f64::from(active_radius_mm) {
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
/// the inner radius outranks a closer runtime that is moving away.
#[must_use]
pub fn plan_simulation(
    candidates: &[TrackedCandidate],
    current: Option<u64>,
    active_radius_mm: u64,
    standby_radius_mm: u64,
    switch_margin_mm: u64,
) -> SimulationPlan {
    let simulator = select_simulator_from(
        candidates.iter().map(|tracked| &tracked.runtime),
        current,
        active_radius_mm,
        switch_margin_mm,
    );
    let standby = candidates
        .iter()
        .filter(|tracked| {
            let runtime = tracked.runtime;
            runtime.active
                && runtime.has_capacity
                && runtime.distance_mm > active_radius_mm
                && runtime.distance_mm < standby_radius_mm
                && tracked.approach != Approach::Passing
        })
        .min_by_key(|tracked| {
            (
                tracked.approach == Approach::Away,
                tracked.runtime.distance_mm,
                tracked.runtime.runtime_id,
            )
        })
        .map(|tracked| tracked.runtime.runtime_id);
    SimulationPlan { simulator, standby }
}

/// Selects a simulator within range. An existing eligible simulator retains
/// its assignment until a challenger is closer by more than the margin.
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
    let eligible =
        candidates.filter(|candidate| candidate.active && candidate.distance_mm <= range_mm);
    let incumbent = eligible
        .clone()
        .find(|candidate| Some(candidate.runtime_id) == current);
    let challenger = eligible
        .filter(|candidate| candidate.has_capacity && Some(candidate.runtime_id) != current)
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
            plan_simulation(candidates, None, 100_000, 1_000_000, 5_000)
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
            select_simulator(&[owner, busy, candidate(2, 101)], Some(1), 100, 10),
            None
        );
    }
}
