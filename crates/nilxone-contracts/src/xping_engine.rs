// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! xPing's engine: the energy a flight's forcing spends, how it comes back,
//! and the breakdown that forcing on an empty engine causes.
//!
//! Thirty seconds of forcing spend a full engine. It recharges evenly, from
//! empty to full in twelve hours. Forcing on for two seconds more once it is
//! empty breaks the engine: the flight is called off, and the engine stays
//! broken, and empty, for twelve hours, after which it is whole and full. A
//! repair can be paid for instead, in Seeds ₴€£, in proportion to what is
//! missing: fifty for an empty engine, twenty-five for a half-full one. The
//! inventory takes the payment (`pay_xping_repair`); this module says what
//! it costs and what the engine is afterwards.
//!
//! Core keeps no clock and no store. The host hands in the stored state and
//! the time; Core answers the next state to store. The service that stores
//! it decides the time. Energy is in permille, integers only, so every
//! runtime agrees on the same engine.

use serde::{Deserialize, Serialize};

use crate::DecimalU64;

/// Version of the engine rules below; any change to a number raises it.
pub const XPING_ENGINE_VERSION: u32 = 1;
/// A full engine, in permille.
pub const XPING_FULL_ENERGY: u16 = 1000;
/// Forcing that spends a full engine.
pub const XPING_FORCE_FULL_MS: u64 = 30_000;
/// Recharging an empty engine to full.
pub const XPING_RECHARGE_FULL_MS: u64 = 12 * 60 * 60 * 1000;
/// Forcing on an empty engine for this long breaks it.
pub const XPING_BREAK_AFTER_EMPTY_MS: u64 = 2_000;
/// A broken engine mends itself after this long.
pub const XPING_BROKEN_MS: u64 = XPING_RECHARGE_FULL_MS;
/// What a repair of an empty or broken engine costs, in Seeds ₴€£.
pub const XPING_FULL_REPAIR_SEEDS: u64 = 50;

/// The engine as stored: its energy when last written, and since when it is
/// broken, if it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XpingEngine {
    /// Energy in permille at `at_ms`.
    pub energy_permille: u16,
    /// When `energy_permille` was true.
    pub at_ms: DecimalU64,
    /// When the engine broke; absent while it works.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broken_at_ms: Option<DecimalU64>,
}

impl XpingEngine {
    /// A full, working engine at `now_ms`: what a Bond starts with.
    #[must_use]
    pub const fn new(now_ms: u64) -> Self {
        Self {
            energy_permille: XPING_FULL_ENERGY,
            at_ms: DecimalU64::new(now_ms),
            broken_at_ms: None,
        }
    }

    /// When a broken engine is whole again.
    #[must_use]
    pub fn mends_at_ms(&self) -> Option<u64> {
        self.broken_at_ms
            .map(|at| at.get().saturating_add(XPING_BROKEN_MS))
    }

    /// Whether the engine is broken at `now_ms`.
    #[must_use]
    pub fn broken(&self, now_ms: u64) -> bool {
        self.mends_at_ms().is_some_and(|mends| now_ms < mends)
    }

    /// The energy at `now_ms`: empty while broken, then recharging evenly.
    #[must_use]
    pub fn energy_at(&self, now_ms: u64) -> u16 {
        if let Some(mends) = self.mends_at_ms() {
            return if now_ms < mends { 0 } else { XPING_FULL_ENERGY };
        }
        let elapsed = now_ms.saturating_sub(self.at_ms.get());
        let gained = u128::from(elapsed) * u128::from(XPING_FULL_ENERGY)
            / u128::from(XPING_RECHARGE_FULL_MS);
        let energy = u128::from(self.energy_permille).saturating_add(gained);
        u16::try_from(energy.min(u128::from(XPING_FULL_ENERGY))).unwrap_or(XPING_FULL_ENERGY)
    }

    /// When the engine is full again, if it is not.
    #[must_use]
    pub fn full_at_ms(&self, now_ms: u64) -> Option<u64> {
        if let Some(mends) = self.mends_at_ms() {
            return (now_ms < mends).then_some(mends);
        }
        let missing = XPING_FULL_ENERGY - self.energy_at(now_ms);
        (missing > 0).then(|| {
            let ms = (u128::from(missing) * u128::from(XPING_RECHARGE_FULL_MS))
                .div_ceil(u128::from(XPING_FULL_ENERGY));
            now_ms.saturating_add(u64::try_from(ms).unwrap_or(u64::MAX))
        })
    }

    /// What a paid repair costs at `now_ms`: in proportion to the missing
    /// energy, rounded up; a broken engine is missing all of it.
    #[must_use]
    pub fn repair_seeds(&self, now_ms: u64) -> u64 {
        repair_seeds_for(XPING_FULL_ENERGY - self.energy_at(now_ms))
    }

    /// The engine as of `now_ms`, written down: what recharged is folded in.
    #[must_use]
    pub fn settled(&self, now_ms: u64) -> Self {
        if self.broken(now_ms) {
            return *self;
        }
        Self {
            energy_permille: self.energy_at(now_ms),
            at_ms: DecimalU64::new(now_ms.max(self.at_ms.get())),
            broken_at_ms: None,
        }
    }

    /// Spends `forced_ms` of forcing at `now_ms`.
    ///
    /// # Errors
    ///
    /// [`XpingEngineError::Broken`] while broken.
    pub fn force(&self, forced_ms: u64, now_ms: u64) -> Result<Self, XpingEngineError> {
        if self.broken(now_ms) {
            return Err(XpingEngineError::Broken);
        }
        let mut next = self.settled(now_ms);
        let spent =
            u128::from(forced_ms) * u128::from(XPING_FULL_ENERGY) / u128::from(XPING_FORCE_FULL_MS);
        next.energy_permille =
            u16::try_from(u128::from(next.energy_permille).saturating_sub(spent)).unwrap_or(0);
        Ok(next)
    }

    /// Breaks the engine at `now_ms`: only an empty one can be broken, by
    /// [`XPING_BREAK_AFTER_EMPTY_MS`] of forcing on empty.
    ///
    /// # Errors
    ///
    /// [`XpingEngineError::Broken`] when already broken, and
    /// [`XpingEngineError::NotEmpty`] when energy is left.
    pub fn break_down(&self, now_ms: u64) -> Result<Self, XpingEngineError> {
        if self.broken(now_ms) {
            return Err(XpingEngineError::Broken);
        }
        if self.energy_at(now_ms) > 0 {
            return Err(XpingEngineError::NotEmpty);
        }
        Ok(Self {
            energy_permille: 0,
            at_ms: DecimalU64::new(now_ms.max(self.at_ms.get())),
            broken_at_ms: Some(DecimalU64::new(now_ms)),
        })
    }

    /// A paid repair at `now_ms`: whole and full. The payment is the
    /// inventory's; this only says the engine afterwards.
    ///
    /// # Errors
    ///
    /// [`XpingEngineError::Full`] when there is nothing to repair.
    pub fn repaired(&self, now_ms: u64) -> Result<Self, XpingEngineError> {
        if self.repair_seeds(now_ms) == 0 {
            return Err(XpingEngineError::Full);
        }
        Ok(Self::new(now_ms.max(self.at_ms.get())))
    }
}

/// What a repair of `missing_permille` costs: fifty Seeds for a whole
/// engine, in proportion, rounded up.
#[must_use]
pub fn repair_seeds_for(missing_permille: u16) -> u64 {
    let missing = u64::from(missing_permille.min(XPING_FULL_ENERGY));
    (missing * XPING_FULL_REPAIR_SEEDS).div_ceil(u64::from(XPING_FULL_ENERGY))
}

/// Stable failure classification for the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XpingEngineError {
    /// The engine is broken.
    Broken,
    /// Energy is left, so it cannot break.
    NotEmpty,
    /// Nothing is missing, so there is nothing to repair.
    Full,
}

impl XpingEngineError {
    /// The wire code for this failure.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Broken => "broken",
            Self::NotEmpty => "not_empty",
            Self::Full => "full",
        }
    }
}

impl core::fmt::Display for XpingEngineError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for XpingEngineError {}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Force { forced_ms: DecimalU64 },
    BreakDown,
    Repaired,
}

/// What the engine is at a moment, for drawing it and its controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct XpingEngineView {
    /// Energy in permille.
    pub energy_permille: u16,
    /// Smoke, in permille: whatever energy is missing.
    pub smoke_permille: u16,
    /// Whether the engine is broken.
    pub broken: bool,
    /// When it is whole and full again, if it is not.
    pub full_at_ms: Option<DecimalU64>,
    /// What a paid repair costs now, in Seeds ₴€£.
    pub repair_seeds: DecimalU64,
}

/// What `engine` is at `now_ms`.
#[must_use]
pub fn xping_engine_view(engine: &XpingEngine, now_ms: u64) -> XpingEngineView {
    let energy = engine.energy_at(now_ms);
    XpingEngineView {
        energy_permille: energy,
        smoke_permille: XPING_FULL_ENERGY - energy,
        broken: engine.broken(now_ms),
        full_at_ms: engine.full_at_ms(now_ms).map(DecimalU64::new),
        repair_seeds: DecimalU64::new(engine.repair_seeds(now_ms)),
    }
}

fn read(state: &str, now_ms: u64) -> Option<XpingEngine> {
    if state.is_empty() {
        return Some(XpingEngine::new(now_ms));
    }
    let engine = serde_json::from_str::<XpingEngine>(state).ok()?;
    (engine.energy_permille <= XPING_FULL_ENERGY).then_some(engine)
}

/// [`xping_engine_view`] as JSON for Wasm and `UniFFI`. The empty state is
/// a new, full engine. Answers `{"ok":true,"view":…}` or
/// `{"ok":false,"error":"invalid"}`.
#[must_use]
pub fn xping_engine_view_wire(state: &str, now_ms: u64) -> String {
    match read(state, now_ms) {
        Some(engine) => {
            serde_json::json!({ "ok": true, "view": xping_engine_view(&engine, now_ms) })
                .to_string()
        }
        None => serde_json::json!({ "ok": false, "error": "invalid" }).to_string(),
    }
}

/// Applies one engine command at `now_ms`: `force` (with `forced_ms`),
/// `break_down` or `repaired`. Answers `{"ok":true,"state":…,"view":…}`
/// with the next state to store, or `{"ok":false,"error":<code>}`.
#[must_use]
pub fn xping_engine_command_wire(state: &str, command: &str, now_ms: u64) -> String {
    let Some(engine) = read(state, now_ms) else {
        return serde_json::json!({ "ok": false, "error": "invalid" }).to_string();
    };
    let Ok(command) = serde_json::from_str::<Command>(command) else {
        return serde_json::json!({ "ok": false, "error": "invalid" }).to_string();
    };
    let next = match command {
        Command::Force { forced_ms } => engine.force(forced_ms.get(), now_ms),
        Command::BreakDown => engine.break_down(now_ms),
        Command::Repaired => engine.repaired(now_ms),
    };
    match next {
        Ok(next) => serde_json::json!({
            "ok": true,
            "state": next,
            "view": xping_engine_view(&next, now_ms),
        })
        .to_string(),
        Err(error) => serde_json::json!({ "ok": false, "error": error.code() }).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_800_000_000_000;
    const HOUR: u64 = 60 * 60 * 1000;

    #[test]
    fn thirty_seconds_of_forcing_spend_a_full_engine() {
        let engine = XpingEngine::new(T0);
        let half = engine.force(15_000, T0).unwrap();
        assert_eq!(half.energy_at(T0), 500);
        let empty = half.force(15_000, T0).unwrap();
        assert_eq!(empty.energy_at(T0), 0);
        assert_eq!(empty.force(5_000, T0).unwrap().energy_at(T0), 0);
    }

    #[test]
    fn energy_recharges_evenly_over_twelve_hours() {
        let empty = XpingEngine::new(T0).force(30_000, T0).unwrap();
        assert_eq!(empty.energy_at(T0 + 6 * HOUR), 500);
        assert_eq!(empty.energy_at(T0 + 12 * HOUR), 1000);
        assert_eq!(empty.energy_at(T0 + 30 * HOUR), 1000);
        assert_eq!(empty.full_at_ms(T0), Some(T0 + 12 * HOUR));
        let half = XpingEngine::new(T0).force(15_000, T0).unwrap();
        assert_eq!(half.full_at_ms(T0), Some(T0 + 6 * HOUR));
        assert_eq!(XpingEngine::new(T0).full_at_ms(T0), None);
    }

    #[test]
    fn a_repair_costs_fifty_seeds_whole_and_in_proportion() {
        let empty = XpingEngine::new(T0).force(30_000, T0).unwrap();
        assert_eq!(empty.repair_seeds(T0), 50);
        let half = XpingEngine::new(T0).force(15_000, T0).unwrap();
        assert_eq!(half.repair_seeds(T0), 25);
        // A sliver missing still costs one Seed, rounded up.
        let sliver = XpingEngine::new(T0).force(30, T0).unwrap();
        assert_eq!(sliver.repair_seeds(T0), 1);
        assert_eq!(XpingEngine::new(T0).repair_seeds(T0), 0);
        assert_eq!(
            XpingEngine::new(T0).repaired(T0),
            Err(XpingEngineError::Full)
        );
        let repaired = half.repaired(T0 + 1).unwrap();
        assert_eq!(repaired.energy_at(T0 + 1), 1000);
    }

    #[test]
    fn only_an_empty_engine_breaks_and_it_mends_in_twelve_hours() {
        let engine = XpingEngine::new(T0);
        assert_eq!(engine.break_down(T0), Err(XpingEngineError::NotEmpty));
        let broken = engine
            .force(30_000, T0)
            .unwrap()
            .break_down(T0 + 2_000)
            .unwrap();
        assert!(broken.broken(T0 + 2_000));
        assert_eq!(broken.energy_at(T0 + 6 * HOUR), 0);
        assert_eq!(broken.repair_seeds(T0 + 6 * HOUR), 50);
        assert_eq!(broken.force(1, T0 + HOUR), Err(XpingEngineError::Broken));
        assert_eq!(broken.break_down(T0 + HOUR), Err(XpingEngineError::Broken));
        let mends = T0 + 2_000 + 12 * HOUR;
        assert_eq!(broken.mends_at_ms(), Some(mends));
        assert_eq!(broken.full_at_ms(T0 + HOUR), Some(mends));
        assert!(!broken.broken(mends));
        assert_eq!(broken.energy_at(mends), 1000);
        assert_eq!(broken.repaired(mends), Err(XpingEngineError::Full));
        // A paid repair mends it at once, whole and full.
        let paid = broken.repaired(T0 + HOUR).unwrap();
        assert!(!paid.broken(T0 + HOUR));
        assert_eq!(paid.energy_at(T0 + HOUR), 1000);
        // Forcing after the twelve hours starts from a full engine.
        assert_eq!(broken.force(15_000, mends).unwrap().energy_at(mends), 500);
    }

    #[test]
    fn the_wire_reads_stores_and_refuses() {
        let view: serde_json::Value =
            serde_json::from_str(&xping_engine_view_wire("", T0)).unwrap();
        assert_eq!(view["ok"], true);
        assert_eq!(view["view"]["energy_permille"], 1000);
        assert_eq!(view["view"]["smoke_permille"], 0);
        assert_eq!(view["view"]["repair_seeds"], "0");
        assert_eq!(view["view"]["full_at_ms"], serde_json::Value::Null);

        let forced: serde_json::Value = serde_json::from_str(&xping_engine_command_wire(
            "",
            r#"{"op":"force","forced_ms":"15000"}"#,
            T0,
        ))
        .unwrap();
        assert_eq!(forced["ok"], true);
        assert_eq!(forced["view"]["energy_permille"], 500);
        assert_eq!(forced["view"]["smoke_permille"], 500);
        assert_eq!(forced["view"]["repair_seeds"], "25");
        let state = forced["state"].to_string();
        assert_eq!(
            state,
            format!(r#"{{"at_ms":"{T0}","energy_permille":500}}"#)
        );

        let refused: serde_json::Value = serde_json::from_str(&xping_engine_command_wire(
            &state,
            r#"{"op":"break_down"}"#,
            T0,
        ))
        .unwrap();
        assert_eq!(refused["error"], "not_empty");
        for bad in [
            ("{", r#"{"op":"repaired"}"#),
            (
                r#"{"energy_permille":1001,"at_ms":"0"}"#,
                r#"{"op":"repaired"}"#,
            ),
            ("", r#"{"op":"force"}"#),
            ("", r#"{"op":"fly"}"#),
        ] {
            let answer: serde_json::Value =
                serde_json::from_str(&xping_engine_command_wire(bad.0, bad.1, T0)).unwrap();
            assert_eq!(answer["error"], "invalid", "{bad:?}");
        }
    }
}
