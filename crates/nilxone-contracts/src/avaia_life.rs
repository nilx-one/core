// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Local runtime state of an owned AI Bond. Never presence evidence or a
//! `BondChain`. Hosts supply elapsed observed time and actual route positions;
//! Core owns needs, activity, recovery and the next intent.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{AvaiaPubDress, DecimalU64, GeoCoordinate, PubDress, distance_meters};

const MAX: u64 = 10_000;
const HOME_RADIUS_M: u32 = 50;
// One percent of full energy for every 50 metres actually walked.
const WALK_ENERGY_PER_METER: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifeActivity {
    AtHome,
    Walking,
    Studying,
    Idle,
    ReturningHome,
    Eating,
    Resting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifeIntent {
    Explore,
    ReturnHome,
    Recover,
}

/// Address references come from the authenticated identity projection. They
/// are not fabricated canonical Bond IDs and do not prove ownership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AvaiaLife {
    pub version: String,
    pub owner: String,
    pub subject: String,
    pub home: GeoCoordinate,
    pub position: GeoCoordinate,
    pub hunger: DecimalU64,
    pub energy: DecimalU64,
    pub activity: LifeActivity,
    pub intent: LifeIntent,
    /// Fractional seconds retained so frequent ticks have the same effect.
    pub remainder_ms: DecimalU64,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Initialize {
        home: GeoCoordinate,
        position: GeoCoordinate,
    },
    Observe {
        elapsed_ms: DecimalU64,
        position: GeoCoordinate,
        motion: Motion,
    },
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Motion {
    Idle,
    Walking,
    Studying,
}

impl AvaiaLife {
    fn valid(&self, owner: &str, subject: &str) -> bool {
        self.version == "1"
            && self.owner == owner
            && self.subject == subject
            && self.energy.get() <= MAX
            && self.hunger.get() <= MAX
            && self.remainder_ms.get() < 1_000
    }

    fn decide(&mut self, motion: Motion) {
        let home = distance_meters(self.position, self.home) <= HOME_RADIUS_M;
        // Hysteresis: finish recovering before leaving again.
        let recovering = self.intent == LifeIntent::Recover
            && (self.hunger.get() > 1_000 || self.energy.get() < 9_000);
        let needs_home = self.hunger.get() >= 7_000 || self.energy.get() <= 3_000;
        if home && (needs_home || recovering) {
            self.intent = LifeIntent::Recover;
            self.activity = if self.hunger.get() > 1_000 {
                LifeActivity::Eating
            } else {
                LifeActivity::Resting
            };
        } else if !home && (needs_home || recovering) {
            self.intent = LifeIntent::ReturnHome;
            self.activity = LifeActivity::ReturningHome;
        } else {
            self.intent = LifeIntent::Explore;
            self.activity = match motion {
                Motion::Walking => LifeActivity::Walking,
                Motion::Studying => LifeActivity::Studying,
                Motion::Idle if home => LifeActivity::AtHome,
                Motion::Idle => LifeActivity::Idle,
            };
        }
    }

    fn observe(&mut self, elapsed: u64, position: GeoCoordinate, motion: Motion) {
        let seconds = (elapsed + self.remainder_ms.get()) / 1_000;
        self.remainder_ms = DecimalU64::new((elapsed + self.remainder_ms.get()) % 1_000);
        let was_home = distance_meters(self.position, self.home) <= HOME_RADIUS_M;
        // No instant refill on arrival. Only time actually spent recovering
        // at home can reduce hunger or replenish energy.
        let recovering = was_home
            && self.intent == LifeIntent::Recover
            && !matches!(motion, Motion::Walking)
            && distance_meters(position, self.home) <= HOME_RADIUS_M;
        let hunger = if recovering {
            self.hunger.get().saturating_sub(seconds * 20)
        } else {
            (self.hunger.get() + seconds).min(MAX)
        };
        let energy = if recovering {
            (self.energy.get() + seconds * 10).min(MAX)
        } else {
            // Charge walked distance, not camera movement or imagined offline
            // time. A zero-duration observation cannot claim travel.
            let cost = if matches!(motion, Motion::Walking) && elapsed > 0 {
                u64::from(distance_meters(self.position, position))
                    .saturating_mul(WALK_ENERGY_PER_METER)
            } else {
                seconds
            };
            self.energy.get().saturating_sub(cost)
        };
        self.hunger = DecimalU64::new(hunger);
        self.energy = DecimalU64::new(energy);
        self.position = position;
        self.decide(motion);
    }
}

/// Applies a host observation to one AI Bond's stored local state. Decimal
/// strings and E7 coordinates reuse the existing Core wire scalars. Elapsed
/// time is active observation time, at most one minute per command; a closed
/// client must not simulate offline trips. Invalid input leaves storage intact.
#[must_use]
pub fn apply_avaia_life(state: &str, owner: &str, subject: &str, command: &str) -> String {
    match apply(state, owner, subject, command) {
        Ok(state) => json!({"ok":true,"state":state}).to_string(),
        Err(code) => json!({"ok":false,"error":code}).to_string(),
    }
}

fn apply(
    state: &str,
    owner: &str,
    subject: &str,
    command: &str,
) -> Result<AvaiaLife, &'static str> {
    let human: PubDress = owner.parse().map_err(|_| "invalid_identity")?;
    let ai: AvaiaPubDress = subject.parse().map_err(|_| "invalid_identity")?;
    if human.discriminator() != ai.owner_discriminator() {
        return Err("invalid_identity");
    }
    let command: Command = serde_json::from_str(command).map_err(|_| "invalid_command")?;
    match command {
        Command::Initialize { home, position } => {
            if !state.is_empty() {
                return Err("already_initialized");
            }
            let mut state = AvaiaLife {
                version: "1".to_owned(),
                owner: owner.to_owned(),
                subject: subject.to_owned(),
                home,
                position,
                hunger: DecimalU64::new(0),
                energy: DecimalU64::new(MAX),
                activity: LifeActivity::AtHome,
                intent: LifeIntent::Explore,
                remainder_ms: DecimalU64::new(0),
            };
            state.decide(Motion::Idle);
            Ok(state)
        }
        Command::Observe {
            elapsed_ms,
            position,
            motion,
        } => {
            let mut state: AvaiaLife = serde_json::from_str(state).map_err(|_| "invalid_state")?;
            if !state.valid(owner, subject) {
                return Err("invalid_state");
            }
            if elapsed_ms.get() > 60_000 {
                return Err("invalid_elapsed");
            }
            state.observe(elapsed_ms.get(), position, motion);
            Ok(state)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn initial() -> AvaiaLife {
        apply("", "0x0sky", "x0skai", r#"{"op":"initialize","home":{"longitude_e7":"305234000","latitude_e7":"504501000"},"position":{"longitude_e7":"305234000","latitude_e7":"504501000"}}"#).unwrap()
    }

    #[test]
    fn hunger_and_fatigue_return_home_then_recover_over_time() {
        let mut s = initial();
        let home = s.home;
        let away = GeoCoordinate::from_degrees(30.54, 50.45).unwrap();
        s.hunger = DecimalU64::new(7_000);
        s.energy = DecimalU64::new(2_000);
        s.observe(0, away, Motion::Walking);
        assert_eq!(s.intent, LifeIntent::ReturnHome);
        s.observe(0, home, Motion::Idle);
        assert_eq!(s.energy.get(), 2_000);
        assert_eq!(s.activity, LifeActivity::Eating);
        for _ in 0..12 {
            s.observe(60_000, home, Motion::Idle);
        }
        assert_eq!(s.intent, LifeIntent::Explore);
        assert_eq!(s.home, home);
    }

    #[test]
    fn position_and_home_are_independent_of_owner_and_cannot_reinitialize() {
        let s = initial();
        let json = serde_json::to_string(&s).unwrap();
        let command = r#"{"op":"observe","elapsed_ms":"0","position":{"longitude_e7":"305400000","latitude_e7":"504501000"},"motion":"walking"}"#;
        let next = apply(&json, "0x0sky", "x0skai", command).unwrap();
        assert_ne!(next.position, s.position);
        assert_eq!(next.home, s.home);
        assert!(apply(&json, "0x0other", "x0skai", command).is_err());
        assert!(apply(&json, "0x0sky", "x0otherai", command).is_err());
        assert!(apply(&json, "0x0sky", "x0skai", r#"{"op":"initialize","home":{"longitude_e7":"0","latitude_e7":"0"},"position":{"longitude_e7":"0","latitude_e7":"0"}}"#).is_err());
    }

    #[test]
    fn walking_energy_is_distance_based_and_stationary_time_is_not_travel() {
        let mut s = initial();
        let at = s.position;
        let fifty_meters = GeoCoordinate::from_degrees(30.5234, 50.45055).unwrap();
        let walked = distance_meters(at, fifty_meters);
        assert!((49..=51).contains(&walked));
        s.observe(40_000, fifty_meters, Motion::Walking);
        assert_eq!(s.energy.get(), MAX - u64::from(walked) * 2);
        let remaining = s.energy.get();
        s.observe(40_000, fifty_meters, Motion::Walking);
        assert_eq!(s.energy.get(), remaining);
        s.observe(10_000, fifty_meters, Motion::Idle);
        assert_eq!(s.energy.get(), remaining - 10);
    }

    #[test]
    fn a_walk_out_of_recovery_spends_distance_energy() {
        let mut s = initial();
        s.intent = LifeIntent::Recover;
        s.energy = DecimalU64::new(5_000);
        let next = GeoCoordinate::from_degrees(30.5234, 50.45055).unwrap();
        let meters = distance_meters(s.position, next);
        s.observe(1_000, next, Motion::Walking);
        assert_eq!(s.energy.get(), 5_000 - u64::from(meters) * 2);
    }

    #[test]
    fn ticks_keep_remainders_and_reject_malformed_state() {
        let mut s = initial();
        for _ in 0..10 {
            s.observe(100, s.position, Motion::Idle);
        }
        assert_eq!(s.hunger.get(), 1);
        assert_eq!(s.energy.get(), 9_999);
        s.energy = DecimalU64::new(MAX + 1);
        let json = serde_json::to_string(&s).unwrap();
        assert!(apply(&json, "0x0sky", "x0skai", r#"{"op":"observe","elapsed_ms":"0","position":{"longitude_e7":"0","latitude_e7":"0"},"motion":"idle"}"#).is_err());
    }
}
