// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Transport-independent interest and update pacing for procedural entities.

use h3o::{CellIndex, Resolution};
use serde::{Deserialize, Serialize};

/// Authority and sequence are scoped to one entity in one world.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update<T> {
    pub world_id: String,
    pub entity_id: String,
    pub epoch: u64,
    pub sequence: u64,
    pub change: Change<T>,
}

/// A complete state for subscriptions and recovery, or an incremental change
/// against a specific sequence. A recipient missing the base needs a snapshot.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change<T> {
    Snapshot { state: T },
    Delta { base_sequence: u64, state: T },
}

/// Checks interest using canonical resolution-9 world cells. The caller must
/// independently authorize both ends for the same world.
#[must_use]
pub fn nearby(entity_cell: CellIndex, recipient_cell: CellIndex) -> bool {
    entity_cell.resolution() == Resolution::Nine
        && recipient_cell.resolution() == Resolution::Nine
        && entity_cell.grid_disk::<Vec<_>>(1).contains(&recipient_cell)
}

/// Paces a single entity's motion stream using the host's monotonic clock.
/// Pending motion must be a complete replacement or be based on the last
/// delivered state, so dropping an intermediate update is safe.
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
    pub fn take_due(&mut self, now_ms: u64, interval_ms: u64) -> Option<T> {
        if self
            .last_sent_ms
            .is_some_and(|last| now_ms < last.saturating_add(interval_ms))
        {
            return None;
        }
        let motion = self.pending.take()?;
        self.last_sent_ms = Some(now_ms);
        Some(motion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_fanout_to_the_local_cell_and_its_neighbors() {
        let cell: CellIndex = "8928308280fffff".parse().unwrap();
        let neighbor = cell
            .grid_disk::<Vec<_>>(1)
            .into_iter()
            .find(|candidate| *candidate != cell)
            .unwrap();
        let distant: CellIndex = "89194ad14c3ffff".parse().unwrap();
        assert!(nearby(cell, cell));
        assert!(nearby(cell, neighbor));
        assert!(!nearby(cell, distant));
    }

    #[test]
    fn coalesces_motion_and_clears_it_after_a_full_snapshot() {
        let mut queue = MotionQueue::default();
        queue.push(1);
        assert_eq!(queue.take_due(100, 20), Some(1));
        queue.push(2);
        assert_eq!(queue.take_due(110, 20), None);
        queue.push(3);
        assert_eq!(queue.take_due(110, 10), Some(3));
        queue.push(4);
        queue.record_snapshot(112);
        assert_eq!(queue.take_due(200, 20), None);
    }
}
