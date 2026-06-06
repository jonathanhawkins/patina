//! The Debugger panel's **Profiler tab**.
//!
//! While the game plays, the profiler can be started to capture per-frame
//! function timings. Each captured frame reports, per function, the total time
//! (including callees), the self time (excluding callees), and the call count;
//! the profiler accumulates these across frames into a cost breakdown that the
//! UI can sort by total time, self time, or call count. Capture only happens
//! between `start()` and `stop()`.

/// How the cost breakdown is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostSort {
    /// Highest total time (self + callees) first.
    TotalTime,
    /// Highest self time (excluding callees) first.
    SelfTime,
    /// Most calls first.
    CallCount,
}

/// One function's cost within a single captured frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameCost {
    /// Function name.
    pub name: String,
    /// Total time this frame (ms), including callees.
    pub total_ms: f32,
    /// Self time this frame (ms), excluding callees.
    pub self_ms: f32,
    /// Number of calls this frame.
    pub calls: u32,
}

/// The accumulated cost of one function across all captured frames.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionCost {
    /// Function name.
    pub name: String,
    /// Summed total time (ms) across captured frames.
    pub total_ms: f32,
    /// Summed self time (ms) across captured frames.
    pub self_ms: f32,
    /// Summed call count across captured frames.
    pub calls: u32,
}

/// The Profiler tab: capture state plus the accumulated per-function costs.
#[derive(Debug, Clone, Default)]
pub struct DebuggerProfiler {
    capturing: bool,
    costs: Vec<FunctionCost>,
    frames: u32,
}

impl DebuggerProfiler {
    /// Creates an idle profiler with no captured data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the profiler is currently capturing.
    pub fn is_capturing(&self) -> bool {
        self.capturing
    }

    /// Starts capturing (e.g. when the user starts the profiler during play).
    pub fn start(&mut self) {
        self.capturing = true;
    }

    /// Stops capturing. Accumulated data is retained.
    pub fn stop(&mut self) {
        self.capturing = false;
    }

    /// The number of frames captured so far.
    pub fn frame_count(&self) -> u32 {
        self.frames
    }

    /// Captures one frame's per-function costs, accumulating them into the
    /// breakdown. No-op (returns `false`) unless the profiler is capturing.
    pub fn capture_frame(&mut self, samples: &[FrameCost]) -> bool {
        if !self.capturing {
            return false;
        }
        for sample in samples {
            match self.costs.iter_mut().find(|c| c.name == sample.name) {
                Some(existing) => {
                    existing.total_ms += sample.total_ms;
                    existing.self_ms += sample.self_ms;
                    existing.calls += sample.calls;
                }
                None => self.costs.push(FunctionCost {
                    name: sample.name.clone(),
                    total_ms: sample.total_ms,
                    self_ms: sample.self_ms,
                    calls: sample.calls,
                }),
            }
        }
        self.frames += 1;
        true
    }

    /// The accumulated cost of one function by name, if captured.
    pub fn cost_of(&self, name: &str) -> Option<&FunctionCost> {
        self.costs.iter().find(|c| c.name == name)
    }

    /// The cost breakdown, sorted (descending) by the chosen key. Ties keep a
    /// stable, name-ascending order.
    pub fn breakdown(&self, sort: CostSort) -> Vec<FunctionCost> {
        let mut rows = self.costs.clone();
        rows.sort_by(|a, b| {
            let primary = match sort {
                CostSort::TotalTime => b.total_ms.partial_cmp(&a.total_ms),
                CostSort::SelfTime => b.self_ms.partial_cmp(&a.self_ms),
                CostSort::CallCount => b.calls.cmp(&a.calls).into(),
            }
            .unwrap_or(std::cmp::Ordering::Equal);
            primary.then_with(|| a.name.cmp(&b.name))
        });
        rows
    }

    /// Clears all captured data (capture state is unchanged).
    pub fn clear(&mut self) {
        self.costs.clear();
        self.frames = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fc(name: &str, total: f32, self_ms: f32, calls: u32) -> FrameCost {
        FrameCost {
            name: name.to_string(),
            total_ms: total,
            self_ms,
            calls,
        }
    }

    /// Acceptance (pat-1dzab): starting the profiler during play captures
    /// per-frame function costs, and the breakdown is sortable by total/self
    /// time.
    #[test]
    fn bottom_debugger_profiler_captures_costs() {
        let mut prof = DebuggerProfiler::new();
        assert!(!prof.is_capturing());

        // Before starting, frames are ignored.
        assert!(!prof.capture_frame(&[fc("_process", 5.0, 5.0, 1)]));
        assert_eq!(prof.frame_count(), 0);

        // Start capturing during play.
        prof.start();
        assert!(prof.is_capturing());

        // Frame 1: _physics_process dominates total; draw has high self time.
        assert!(prof.capture_frame(&[
            fc("_physics_process", 8.0, 2.0, 1),
            fc("draw", 4.0, 4.0, 10),
            fc("_process", 1.0, 1.0, 1),
        ]));
        // Frame 2: accumulates onto the same functions.
        assert!(prof.capture_frame(&[
            fc("_physics_process", 6.0, 1.5, 1),
            fc("draw", 5.0, 5.0, 12),
        ]));
        assert_eq!(prof.frame_count(), 2);

        // Costs accumulate across frames.
        let phys = prof.cost_of("_physics_process").unwrap();
        assert_eq!(phys.total_ms, 14.0);
        assert_eq!(phys.self_ms, 3.5);
        assert_eq!(phys.calls, 2);
        let draw = prof.cost_of("draw").unwrap();
        assert_eq!(draw.total_ms, 9.0);
        assert_eq!(draw.self_ms, 9.0);
        assert_eq!(draw.calls, 22);

        // Sort by total time: _physics_process (14) > draw (9) > _process (1).
        let by_total = prof.breakdown(CostSort::TotalTime);
        assert_eq!(
            by_total.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["_physics_process", "draw", "_process"]
        );

        // Sort by self time: draw (9) > _physics_process (3.5) > _process (1).
        let by_self = prof.breakdown(CostSort::SelfTime);
        assert_eq!(
            by_self.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["draw", "_physics_process", "_process"]
        );

        // Sort by call count: draw (22) > _physics_process (2) > _process (1).
        let by_calls = prof.breakdown(CostSort::CallCount);
        assert_eq!(by_calls[0].name, "draw");

        // Stopping ends capture; further frames are ignored but data remains.
        prof.stop();
        assert!(!prof.is_capturing());
        assert!(!prof.capture_frame(&[fc("draw", 1.0, 1.0, 1)]));
        assert_eq!(prof.frame_count(), 2);
        assert_eq!(prof.cost_of("draw").unwrap().total_ms, 9.0);

        // Clearing resets the breakdown.
        prof.clear();
        assert_eq!(prof.frame_count(), 0);
        assert!(prof.breakdown(CostSort::TotalTime).is_empty());
    }
}
