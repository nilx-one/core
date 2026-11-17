// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Transport-independent interest and update pacing for procedural entities.

use h3o::{CellIndex, Resolution};
use serde::{Deserialize, Deserializer, Serialize, de};

/// Authority and sequence are scoped to one entity in one world.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update<T> {
    #[serde(deserialize_with = "deserialize_id")]
    pub world_id: String,
    #[serde(deserialize_with = "deserialize_id")]
    pub entity_id: String,
    pub epoch: u64,
    pub sequence: u64,
    pub change: Change<T>,
}

fn deserialize_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let id = String::deserialize(deserializer)?;
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_'))
    {
        return Err(de::Error::custom("invalid procedural identifier"));
    }
    Ok(id)
}

impl<T> Update<T> {
    /// Checks the receiver's cursor for this world and entity, after the host
    /// has authenticated the sender and checked the committed epoch.
    #[must_use]
    pub fn applies_to(&self, last_applied: Option<(u64, u64)>) -> bool {
        if last_applied.is_some_and(|cursor| (self.epoch, self.sequence) <= cursor) {
            return false;
        }
        match &self.change {
            Change::Snapshot { .. } => true,
            Change::Delta { base_sequence, .. } => {
                last_applied == Some((self.epoch, *base_sequence))
            }
        }
    }
}

/// A complete state for subscriptions and recovery, or an incremental change
/// against a specific sequence. A recipient missing the base needs a snapshot.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change<T> {
    Snapshot { state: T },
    Delta { base_sequence: u64, state: T },
}

/// Interest queries require resolution-9 world cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidResolution;

/// Checks interest within a host-selected ring. Configure the ring to cover
/// the standby radius when this filter also controls standby delivery.
/// The caller must independently authorize both ends for the same world.
///
/// # Errors
/// Returns an error if either cell is outside the world resolution.
pub fn nearby(
    entity_cell: CellIndex,
    recipient_cell: CellIndex,
    ring: u8,
) -> Result<bool, InvalidResolution> {
    if entity_cell.resolution() != Resolution::Nine
        || recipient_cell.resolution() != Resolution::Nine
    {
        return Err(InvalidResolution);
    }
    Ok(entity_cell
        .grid_disk_safe(u32::from(ring))
        .any(|cell| cell == recipient_cell))
}

/// Paces a single entity's motion stream using the host's monotonic clock.
/// Coalesced motion is emitted as complete state, so it has no delta base.
/// The host assigns a sequence when publishing the returned snapshot.
#[derive(Debug)]
pub struct MotionQueue<T> {
    last_sent_ms: Option<u64>,
    pending: Option<T>,
}

impl<T> Default for MotionQueue<T> {
    fn default() -> Self {
        Self {
            last_sent_ms: None,
            pending: None,
        }
    }
}

impl<T> MotionQueue<T> {
    /// Replaces unsent motion with the latest available state.
    pub fn push(&mut self, motion: T) {
        self.pending = Some(motion);
    }

    /// Records an immediate full-state publication and clears superseded motion.
    pub fn record_snapshot(&mut self, now_ms: u64) {
        self.pending = None;
        self.last_sent_ms = Some(self.last_sent_ms.map_or(now_ms, |last| last.max(now_ms)));
    }

    /// Takes the latest pending motion once the current interval has elapsed.
    /// The host can change the interval as distance or link conditions change.
    pub fn take_due(&mut self, now_ms: u64, interval_ms: u64) -> Option<Change<T>> {
        if self
            .last_sent_ms
            .is_some_and(|last| now_ms < last.saturating_add(interval_ms))
        {
            return None;
        }
        let motion = self.pending.take()?;
        self.last_sent_ms = Some(now_ms);
        Some(Change::Snapshot { state: motion })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_wire_and_requires_the_exact_delta_base() {
        let wire = r#"{"world_id":"world","entity_id":"map-v1:2","epoch":2,"sequence":5,"change":{"kind":"delta","base_sequence":3,"state":9}}"#;
        let update: Update<u8> = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&update).unwrap(), wire);
        assert!(update.applies_to(Some((2, 3))));
        assert!(!update.applies_to(Some((1, 3))));
        assert!(!update.applies_to(Some((2, 4))));
        assert!(!update.applies_to(None));
        let snapshot = Update {
            change: Change::Snapshot { state: 9 },
            ..update
        };
        assert_eq!(
            serde_json::to_string(&snapshot).unwrap(),
            r#"{"world_id":"world","entity_id":"map-v1:2","epoch":2,"sequence":5,"change":{"kind":"snapshot","state":9}}"#
        );
        assert!(snapshot.applies_to(None));
        assert!(!snapshot.applies_to(Some((2, 5))));
        for invalid in [
            wire.replace("map-v1:2", &"a".repeat(129)),
            wire.replace("map-v1:2", "bad id"),
            wire.replace("\"world_id\":", "\"extra\":0,\"world_id\":"),
            wire.replace("\"kind\":", "\"extra\":0,\"kind\":"),
        ] {
            assert!(serde_json::from_str::<Update<u8>>(&invalid).is_err());
        }
    }

    #[test]
    fn limits_fanout_to_the_local_cell_and_its_neighbors() {
        let cell: CellIndex = "8928308280fffff".parse().unwrap();
        let neighbor = cell
            .grid_disk::<Vec<_>>(1)
            .into_iter()
            .find(|candidate| *candidate != cell)
            .unwrap();
        let distant: CellIndex = "89194ad14c3ffff".parse().unwrap();
        assert_eq!(nearby(cell, cell, 0), Ok(true));
        assert_eq!(nearby(cell, neighbor, 1), Ok(true));
        assert_eq!(nearby(cell, distant, 1), Ok(false));
        let farther = cell
            .grid_disk_safe(4)
            .find(|candidate| nearby(cell, *candidate, 1) == Ok(false))
            .unwrap();
        assert_eq!(nearby(cell, farther, 4), Ok(true));
        assert_eq!(
            nearby(cell, cell.parent(Resolution::Eight).unwrap(), 1),
            Err(InvalidResolution)
        );
    }

    #[test]
    fn coalesces_motion_and_clears_it_after_a_full_snapshot() {
        let mut queue = MotionQueue::default();
        queue.push(1);
        assert_eq!(queue.take_due(100, 20), Some(Change::Snapshot { state: 1 }));
        queue.push(2);
        assert_eq!(queue.take_due(110, 20), None);
        queue.push(3);
        assert_eq!(queue.take_due(99, 0), None);
        assert_eq!(queue.take_due(110, 0), Some(Change::Snapshot { state: 3 }));
        queue.push(4);
        queue.record_snapshot(112);
        assert_eq!(queue.take_due(200, 20), None);
    }
}
