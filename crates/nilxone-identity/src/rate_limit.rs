// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Default)]
pub struct AttemptLimiter {
    entries: Arc<Mutex<HashMap<String, AttemptState>>>,
}

#[derive(Clone, Copy, Debug)]
struct AttemptState {
    failures: u32,
    window_started_at: u64,
    blocked_until: u64,
}

impl AttemptLimiter {
    pub fn consume(
        &self,
        key: impl Into<String>,
        now: u64,
        maximum: u32,
        window_seconds: u64,
    ) -> Result<(), u64> {
        let key = key.into();
        let mut entries = self.entries.lock().expect("attempt limiter lock poisoned");
        let state = entries.entry(key).or_insert(AttemptState {
            failures: 0,
            window_started_at: now,
            blocked_until: 0,
        });
        if now.saturating_sub(state.window_started_at) >= window_seconds {
            *state = AttemptState {
                failures: 0,
                window_started_at: now,
                blocked_until: 0,
            };
        }
        if state.blocked_until > now {
            return Err(state.blocked_until - now);
        }

        state.failures = state.failures.saturating_add(1);
        if state.failures > maximum {
            state.blocked_until = now.saturating_add(window_seconds.min(300));
            return Err(state.blocked_until - now);
        }
        Ok(())
    }

    pub fn check(&self, key: &str, now: u64) -> Result<(), u64> {
        let entries = self.entries.lock().expect("attempt limiter lock poisoned");
        match entries.get(key) {
            Some(state) if state.blocked_until > now => Err(state.blocked_until - now),
            _ => Ok(()),
        }
    }

    pub fn record_authentication_failure(&self, key: impl Into<String>, now: u64) {
        let key = key.into();
        let mut entries = self.entries.lock().expect("attempt limiter lock poisoned");
        let state = entries.entry(key).or_insert(AttemptState {
            failures: 0,
            window_started_at: now,
            blocked_until: 0,
        });
        if now.saturating_sub(state.window_started_at) >= 900 {
            state.failures = 0;
            state.window_started_at = now;
        }
        state.failures = state.failures.saturating_add(1);
        let exponent = state.failures.saturating_sub(1).min(8);
        let delay = 1_u64 << exponent;
        state.blocked_until = now.saturating_add(delay.min(300));
    }

    pub fn clear(&self, key: &str) {
        self.entries
            .lock()
            .expect("attempt limiter lock poisoned")
            .remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::AttemptLimiter;

    #[test]
    fn authentication_backoff_is_observable_and_resettable() {
        let limiter = AttemptLimiter::default();
        limiter.record_authentication_failure("auth:0x0sky", 100);
        assert_eq!(limiter.check("auth:0x0sky", 100), Err(1));
        assert_eq!(limiter.check("auth:0x0sky", 101), Ok(()));
        limiter.clear("auth:0x0sky");
        assert_eq!(limiter.check("auth:0x0sky", 100), Ok(()));
    }

    #[test]
    fn fixed_window_limits_high_volume_public_reads() {
        let limiter = AttemptLimiter::default();
        assert_eq!(limiter.consume("resolve:source", 100, 2, 60), Ok(()));
        assert_eq!(limiter.consume("resolve:source", 100, 2, 60), Ok(()));
        assert_eq!(limiter.consume("resolve:source", 100, 2, 60), Err(60));
        assert_eq!(limiter.consume("resolve:source", 160, 2, 60), Ok(()));
    }
}
