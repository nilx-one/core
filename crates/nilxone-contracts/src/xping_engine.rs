// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! xPing's engine: the energy a flight's forcing spends, how it comes back,
//! and the breakdown that forcing on an empty engine causes.
//!
//! Thirty seconds of forcing spend a full engine. It recharges evenly, from
//! empty to full in twelve hours. Forcing on for two seconds more once it is
//! empty breaks the engine: Core counts that forcing itself, so a host
//! cannot break a working engine or keep forcing an exhausted one. A broken
//! engine stays broken, and empty, for twelve hours, after which it is whole
//! and full. A repair can be paid for instead, in Seeds ₴€£, in proportion
//! to what is missing at the moment of the repair: fifty for an empty or
//! broken engine, twenty-five for a half-full one. The price is always
//! Core's, from the engine itself ([`crate::Inventory::pay_xping_repair`]).
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
/// Forcing one permille of energy pays for.
const FORCE_MS_PER_PERMILLE: u64 = XPING_FORCE_FULL_MS / XPING_FULL_ENERGY as u64;
/// Recharging an empty engine to full.
pub const XPING_RECHARGE_FULL_MS: u64 = 12 * 60 * 60 * 1000;
/// Forcing on an empty engine for this long breaks it.
pub const XPING_BREAK_AFTER_EMPTY_MS: u64 = 2_000;
/// A broken engine mends itself after this long.
pub const XPING_BROKEN_MS: u64 = XPING_RECHARGE_FULL_MS;
/// What a repair of an empty or broken engine costs, in Seeds ₴€£.
pub const XPING_FULL_REPAIR_SEEDS: u64 = 50;

/// The engine as stored: its energy when last written, the forcing spent on
/// it since it ran empty, and since when it is broken, if it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XpingEngine {
    /// Energy in permille at `at_ms`.
    pub energy_permille: u16,
    /// When `energy_permille` was true.
    pub at_ms: DecimalU64,
    /// Forcing on an empty engine since it last had energy.
    #[serde(default = "no_forcing", skip_serializing_if = "is_zero")]
    pub empty_forced_ms: DecimalU64,
    /// When the engine broke; absent while it works.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broken_at_ms: Option<DecimalU64>,
}

const fn no_forcing() -> DecimalU64 {
    DecimalU64::new(0)
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if signature
fn is_zero(value: &DecimalU64) -> bool {
    value.get() == 0
}

impl XpingEngine {
    /// A full, working engine at `now_ms`: what a Bond starts with.
    #[must_use]
    pub const fn new(now_ms: u64) -> Self {
        Self {
            energy_permille: XPING_FULL_ENERGY,
            at_ms: DecimalU64::new(now_ms),
            empty_forced_ms: DecimalU64::new(0),
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
        let missing = u64::from(XPING_FULL_ENERGY - self.energy_at(now_ms));
        (missing * XPING_FULL_REPAIR_SEEDS).div_ceil(u64::from(XPING_FULL_ENERGY))
    }

    /// The engine as of `now_ms`, written down: what recharged is folded in,
    /// a mended engine is whole, and forcing counted on empty is forgotten
    /// once any energy came back.
    #[must_use]
    pub fn settled(&self, now_ms: u64) -> Self {
        if self.broken(now_ms) {
            return *self;
        }
        let energy = self.energy_at(now_ms);
        Self {
            energy_permille: energy,
            at_ms: DecimalU64::new(now_ms.max(self.at_ms.get())),
            empty_forced_ms: if energy > 0 || self.broken_at_ms.is_some() {
                DecimalU64::new(0)
            } else {
                self.empty_forced_ms
            },
            broken_at_ms: None,
        }
    }

    /// Spends `forced_ms` of forcing reported at `now_ms`. Forcing past
    /// empty is counted; once [`XPING_BREAK_AFTER_EMPTY_MS`] of it adds up
    /// without any energy coming back, the engine breaks at `now_ms`.
    ///
    /// # Errors
    ///
    /// [`XpingEngineError::Broken`] while broken.
    pub fn force(&self, forced_ms: u64, now_ms: u64) -> Result<Self, XpingEngineError> {
        if self.broken(now_ms) {
            return Err(XpingEngineError::Broken);
        }
        let mut next = self.settled(now_ms);
        let capacity_ms = u64::from(next.energy_permille) * FORCE_MS_PER_PERMILLE;
        if forced_ms <= capacity_ms {
            // What is left is whole permille: a part-spent one is spent.
            let left = (capacity_ms - forced_ms) / FORCE_MS_PER_PERMILLE;
            next.energy_permille = u16::try_from(left).unwrap_or(0);
            return Ok(next);
        }
        next.energy_permille = 0;
        let empty = next
            .empty_forced_ms
            .get()
            .saturating_add(forced_ms - capacity_ms);
        next.empty_forced_ms = DecimalU64::new(empty);
        if empty >= XPING_BREAK_AFTER_EMPTY_MS {
            next.empty_forced_ms = DecimalU64::new(0);
            next.broken_at_ms = Some(DecimalU64::new(now_ms));
        }
        Ok(next)
    }

    /// A repair at `now_ms` for which `paid_seeds` were paid: whole and
    /// full. This is the storing service's half of a repair; the payment is
    /// [`crate::Inventory::pay_xping_repair`], priced from this same engine.
    ///
    /// # Errors
    ///
    /// [`XpingEngineError::Full`] when there is nothing to repair, and
    /// [`XpingEngineError::Underpaid`] when `paid_seeds` is below the price
    /// at `now_ms`. A price only falls as the engine recharges, so a
    /// payment priced earlier still covers a repair recorded later.
    pub fn repaired(&self, paid_seeds: u64, now_ms: u64) -> Result<Self, XpingEngineError> {
        let price = self.repair_seeds(now_ms);
        if price == 0 {
            return Err(XpingEngineError::Full);
        }
        if paid_seeds < price {
            return Err(XpingEngineError::Underpaid);
        }
        Ok(Self::new(now_ms.max(self.at_ms.get())))
    }
}

/// Stable failure classification for the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XpingEngineError {
    /// The engine is broken.
    Broken,
    /// Nothing is missing, so there is nothing to repair.
    Full,
    /// Less was paid than the repair costs.
    Underpaid,
}

impl XpingEngineError {
    /// The wire code for this failure.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Broken => "broken",
            Self::Full => "full",
            Self::Underpaid => "underpaid",
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
    Repaired { paid_seeds: DecimalU64 },
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
    /// Forcing left on empty before it breaks; the whole margin while there
    /// is energy.
    pub empty_margin_ms: DecimalU64,
    /// When it is whole and full again, if it is not.
    pub full_at_ms: Option<DecimalU64>,
    /// What a paid repair costs now, in Seeds ₴€£.
    pub repair_seeds: DecimalU64,
}

/// What `engine` is at `now_ms`.
#[must_use]
pub fn xping_engine_view(engine: &XpingEngine, now_ms: u64) -> XpingEngineView {
    let settled = engine.settled(now_ms);
    let energy = engine.energy_at(now_ms);
    let broken = engine.broken(now_ms);
    XpingEngineView {
        energy_permille: energy,
        smoke_permille: XPING_FULL_ENERGY - energy,
        broken,
        empty_margin_ms: DecimalU64::new(if broken {
            0
        } else {
            XPING_BREAK_AFTER_EMPTY_MS.saturating_sub(settled.empty_forced_ms.get())
        }),
        full_at_ms: engine.full_at_ms(now_ms).map(DecimalU64::new),
        repair_seeds: DecimalU64::new(engine.repair_seeds(now_ms)),
    }
}

/// Reads a stored engine: the empty string is a new, full one.
pub(crate) fn read_engine(state: &str, now_ms: u64) -> Option<XpingEngine> {
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
    match read_engine(state, now_ms) {
        Some(engine) => {
            serde_json::json!({ "ok": true, "view": xping_engine_view(&engine, now_ms) })
                .to_string()
        }
        None => serde_json::json!({ "ok": false, "error": "invalid" }).to_string(),
    }
}

/// Applies one engine command at `now_ms`: `force` (with `forced_ms`) or
/// `repaired` (with `paid_seeds`). Answers `{"ok":true,"state":…,"view":…}`
/// with the next state to store, or `{"ok":false,"error":<code>}`.
#[must_use]
pub fn xping_engine_command_wire(state: &str, command: &str, now_ms: u64) -> String {
    let Some(engine) = read_engine(state, now_ms) else {
        return serde_json::json!({ "ok": false, "error": "invalid" }).to_string();
    };
    let Ok(command) = serde_json::from_str::<Command>(command) else {
        return serde_json::json!({ "ok": false, "error": "invalid" }).to_string();
    };
    let next = match command {
        Command::Force { forced_ms } => engine.force(forced_ms.get(), now_ms),
        Command::Repaired { paid_seeds } => engine.repaired(paid_seeds.get(), now_ms),
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

    fn empty() -> XpingEngine {
        XpingEngine::new(T0).force(30_000, T0).unwrap()
    }

    #[test]
    fn thirty_seconds_of_forcing_spend_a_full_engine() {
        let half = XpingEngine::new(T0).force(15_000, T0).unwrap();
        assert_eq!(half.energy_at(T0), 500);
        assert_eq!(half.force(15_000, T0).unwrap().energy_at(T0), 0);
        // A part-spent permille is spent.
        assert_eq!(
            XpingEngine::new(T0).force(1, T0).unwrap().energy_at(T0),
            999
        );
        assert_eq!(
            XpingEngine::new(T0).force(30, T0).unwrap().energy_at(T0),
            999
        );
        assert_eq!(
            XpingEngine::new(T0).force(31, T0).unwrap().energy_at(T0),
            998
        );
    }

    #[test]
    fn energy_recharges_evenly_over_twelve_hours() {
        assert_eq!(empty().energy_at(T0 + 6 * HOUR), 500);
        assert_eq!(empty().energy_at(T0 + 12 * HOUR), 1000);
        assert_eq!(empty().energy_at(T0 + 30 * HOUR), 1000);
        assert_eq!(empty().full_at_ms(T0), Some(T0 + 12 * HOUR));
        let half = XpingEngine::new(T0).force(15_000, T0).unwrap();
        assert_eq!(half.full_at_ms(T0), Some(T0 + 6 * HOUR));
        assert_eq!(XpingEngine::new(T0).full_at_ms(T0), None);
    }

    #[test]
    fn two_seconds_of_forcing_on_empty_break_the_engine_and_not_before() {
        // In one report: thirty seconds empty the engine, two more break it.
        let at_once = XpingEngine::new(T0).force(31_999, T0).unwrap();
        assert!(!at_once.broken(T0));
        assert_eq!(at_once.empty_forced_ms.get(), 1_999);
        let broken = XpingEngine::new(T0).force(32_000, T0).unwrap();
        assert!(broken.broken(T0));
        // In steps: the forcing on empty adds up.
        let step = empty().force(1_000, T0 + 100).unwrap();
        assert!(!step.broken(T0 + 100));
        assert_eq!(
            xping_engine_view(&step, T0 + 100).empty_margin_ms.get(),
            1_000
        );
        let almost = step.force(999, T0 + 200).unwrap();
        assert!(!almost.broken(T0 + 200));
        let gone = almost.force(1, T0 + 300).unwrap();
        assert!(gone.broken(T0 + 300));
        assert_eq!(gone.broken_at_ms, Some(DecimalU64::new(T0 + 300)));
        assert_eq!(gone.force(1, T0 + 400), Err(XpingEngineError::Broken));
        assert_eq!(xping_engine_view(&gone, T0 + 400).empty_margin_ms.get(), 0);
    }

    #[test]
    fn energy_coming_back_forgets_the_forcing_on_empty() {
        let step = empty().force(1_500, T0).unwrap();
        // One permille recharges in 43.2 s; then the count starts over.
        let later = T0 + 43_200;
        assert_eq!(step.energy_at(later), 1);
        assert_eq!(step.settled(later).empty_forced_ms.get(), 0);
        let after = step.force(30 + 1_500, later).unwrap();
        assert!(!after.broken(later));
        assert_eq!(after.empty_forced_ms.get(), 1_500);
        // Just before the permille comes back, the count holds.
        let early = T0 + 43_199;
        assert!(step.force(500, early).unwrap().broken(early));
    }

    #[test]
    fn a_broken_engine_mends_in_twelve_hours() {
        let broken = XpingEngine::new(T0).force(32_000, T0).unwrap();
        assert_eq!(broken.energy_at(T0 + 6 * HOUR), 0);
        assert_eq!(broken.repair_seeds(T0 + 6 * HOUR), 50);
        let mends = T0 + 12 * HOUR;
        assert_eq!(broken.mends_at_ms(), Some(mends));
        assert_eq!(broken.full_at_ms(T0 + HOUR), Some(mends));
        assert!(!broken.broken(mends));
        assert_eq!(broken.energy_at(mends), 1000);
        // Forcing after the twelve hours starts from a whole, full engine.
        let again = broken.force(15_000, mends).unwrap();
        assert_eq!(again.energy_at(mends), 500);
        assert_eq!(again.broken_at_ms, None);
    }

    #[test]
    fn a_repair_is_priced_from_the_engine_and_never_underpaid() {
        assert_eq!(empty().repair_seeds(T0), 50);
        let half = XpingEngine::new(T0).force(15_000, T0).unwrap();
        assert_eq!(half.repair_seeds(T0), 25);
        // A sliver missing still costs one Seed, rounded up.
        assert_eq!(
            XpingEngine::new(T0).force(30, T0).unwrap().repair_seeds(T0),
            1
        );
        assert_eq!(
            XpingEngine::new(T0).repaired(50, T0),
            Err(XpingEngineError::Full)
        );
        assert_eq!(empty().repaired(1, T0), Err(XpingEngineError::Underpaid));
        assert_eq!(empty().repaired(49, T0), Err(XpingEngineError::Underpaid));
        let repaired = half.repaired(25, T0 + 1).unwrap();
        assert_eq!(repaired.energy_at(T0 + 1), 1000);
        // Priced earlier, recorded later: the price only fell meanwhile.
        assert!(empty().repaired(50, T0 + HOUR).is_ok());
        let broken = XpingEngine::new(T0).force(32_000, T0).unwrap();
        assert_eq!(
            broken.repaired(25, T0 + HOUR),
            Err(XpingEngineError::Underpaid)
        );
        assert!(!broken.repaired(50, T0 + HOUR).unwrap().broken(T0 + HOUR));
    }

    #[test]
    fn the_wire_reads_stores_and_refuses() {
        let view: serde_json::Value =
            serde_json::from_str(&xping_engine_view_wire("", T0)).unwrap();
        assert_eq!(view["ok"], true);
        assert_eq!(view["view"]["energy_permille"], 1000);
        assert_eq!(view["view"]["smoke_permille"], 0);
        assert_eq!(view["view"]["repair_seeds"], "0");
        assert_eq!(view["view"]["empty_margin_ms"], "2000");
        assert_eq!(view["view"]["full_at_ms"], serde_json::Value::Null);

        let forced: serde_json::Value = serde_json::from_str(&xping_engine_command_wire(
            "",
            r#"{"op":"force","forced_ms":"15000"}"#,
            T0,
        ))
        .unwrap();
        assert_eq!(forced["view"]["energy_permille"], 500);
        assert_eq!(forced["view"]["repair_seeds"], "25");
        let state = forced["state"].to_string();
        assert_eq!(
            state,
            format!(r#"{{"at_ms":"{T0}","energy_permille":500}}"#)
        );

        let underpaid: serde_json::Value = serde_json::from_str(&xping_engine_command_wire(
            &state,
            r#"{"op":"repaired","paid_seeds":"24"}"#,
            T0,
        ))
        .unwrap();
        assert_eq!(underpaid["error"], "underpaid");
        for bad in [
            ("{", r#"{"op":"repaired","paid_seeds":"50"}"#),
            (
                r#"{"energy_permille":1001,"at_ms":"0"}"#,
                r#"{"op":"force","forced_ms":"1"}"#,
            ),
            ("", r#"{"op":"force"}"#),
            ("", r#"{"op":"break_down"}"#),
        ] {
            let answer: serde_json::Value =
                serde_json::from_str(&xping_engine_command_wire(bad.0, bad.1, T0)).unwrap();
            assert_eq!(answer["error"], "invalid", "{bad:?}");
        }
    }
}
