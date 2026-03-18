//! Timing, delta tracking, and frame pacing.
//!
//! Provides a `Timer` struct analogous to Godot's `Timer` node.

/// A countdown timer, similar to Godot's `Timer` node.
///
/// Feed it delta-time via `step()` each frame. When the countdown reaches
/// zero, `timeout()` returns `true`. One-shot timers stop automatically;
/// repeating timers restart.
#[derive(Debug, Clone, PartialEq)]
pub struct Timer {
    /// The duration the timer waits before firing, in seconds.
    pub wait_time: f64,
    /// If `true`, the timer stops after the first timeout.
    pub one_shot: bool,
    /// If `true`, the timer starts automatically when created.
    pub autostart: bool,
    /// Remaining time until timeout, in seconds.
    pub time_left: f64,
    /// Whether the timer is currently running.
    running: bool,
    /// Whether a timeout occurred this step.
    timed_out: bool,
}

impl Timer {
    /// Creates a new timer with the given wait time in seconds.
    pub fn new(wait_time: f64) -> Self {
        Self {
            wait_time,
            one_shot: false,
            autostart: false,
            time_left: wait_time,
            running: false,
            timed_out: false,
        }
    }

    /// Builder: sets one-shot mode.
    pub fn with_one_shot(mut self, one_shot: bool) -> Self {
        self.one_shot = one_shot;
        self
    }

    /// Builder: sets autostart.
    pub fn with_autostart(mut self, autostart: bool) -> Self {
        self.autostart = autostart;
        if autostart {
            self.running = true;
        }
        self
    }

    /// Starts (or restarts) the timer.
    pub fn start(&mut self) {
        self.time_left = self.wait_time;
        self.running = true;
        self.timed_out = false;
    }

    /// Stops the timer.
    pub fn stop(&mut self) {
        self.running = false;
        self.timed_out = false;
    }

    /// Returns `true` if the timer is stopped.
    pub fn is_stopped(&self) -> bool {
        !self.running
    }

    /// Advances the timer by `delta` seconds.
    ///
    /// Returns `true` if a timeout occurred during this step.
    pub fn step(&mut self, delta: f64) -> bool {
        self.timed_out = false;

        if !self.running {
            return false;
        }

        self.time_left -= delta;

        if self.time_left <= 0.0 {
            self.timed_out = true;
            if self.one_shot {
                self.running = false;
                self.time_left = 0.0;
            } else {
                // Carry over leftover time for repeating timers.
                self.time_left += self.wait_time;
            }
        }

        self.timed_out
    }

    /// Returns `true` if a timeout occurred on the most recent `step()`.
    pub fn timeout(&self) -> bool {
        self.timed_out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_basic_countdown() {
        let mut timer = Timer::new(1.0);
        timer.start();
        assert!(!timer.step(0.5));
        assert!(!timer.timeout());
        assert!(timer.step(0.6)); // 0.5 + 0.6 = 1.1 > 1.0
        assert!(timer.timeout());
    }

    #[test]
    fn timer_one_shot_stops_after_timeout() {
        let mut timer = Timer::new(1.0).with_one_shot(true);
        timer.start();
        assert!(timer.step(1.5));
        assert!(timer.is_stopped());
        // Further steps do nothing.
        assert!(!timer.step(1.0));
    }

    #[test]
    fn timer_non_one_shot_repeats() {
        let mut timer = Timer::new(1.0);
        timer.start();
        assert!(timer.step(1.1));
        assert!(!timer.is_stopped());
        // Timer should have restarted with carry-over.
        // time_left = -0.1 + 1.0 = 0.9, so another 0.95 should trigger.
        assert!(timer.step(0.95));
    }

    #[test]
    fn timer_stop() {
        let mut timer = Timer::new(1.0);
        timer.start();
        timer.stop();
        assert!(timer.is_stopped());
        assert!(!timer.step(2.0));
    }

    #[test]
    fn timer_autostart() {
        let timer = Timer::new(1.0).with_autostart(true);
        assert!(!timer.is_stopped());
    }
}
