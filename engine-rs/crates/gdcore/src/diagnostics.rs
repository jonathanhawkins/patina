//! Engine diagnostics and instrumentation utilities.
//!
//! Provides lightweight performance counters and a tracing-based log
//! interface so the engine can report what it is doing without pulling
//! in heavy profiling dependencies.

use std::sync::atomic::{AtomicU64, Ordering};

/// A simple monotonic counter for tracking engine events.
///
/// Counters are lock-free and can be incremented from any thread.
pub struct Counter {
    name: &'static str,
    value: AtomicU64,
}

impl Counter {
    /// Creates a new counter with the given label.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            value: AtomicU64::new(0),
        }
    }

    /// Increments the counter by one.
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the counter by `n`.
    pub fn inc_by(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    /// Returns the current value.
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Returns the label.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Resets the counter to zero and returns the previous value.
    pub fn reset(&self) -> u64 {
        self.value.swap(0, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_basic() {
        let c = Counter::new("test");
        assert_eq!(c.get(), 0);
        c.inc();
        c.inc();
        assert_eq!(c.get(), 2);
    }

    #[test]
    fn counter_reset() {
        let c = Counter::new("test");
        c.inc_by(10);
        let prev = c.reset();
        assert_eq!(prev, 10);
        assert_eq!(c.get(), 0);
    }
}
