//! Benchmark dashboard for runtime parity and regression tracking.
//!
//! Provides types for aggregating parity metrics across subsystems
//! (ClassDB, physics, render, lifecycle) and detecting performance
//! regressions against baselines.

/// A single parity metric for one subsystem dimension.
#[derive(Debug, Clone)]
pub struct ParityMetric {
    /// Human-readable label (e.g., "ClassDB methods", "Physics traces").
    pub label: String,
    /// Number of items that match the oracle/expected.
    pub matched: usize,
    /// Total number of items compared.
    pub total: usize,
}

impl ParityMetric {
    /// Creates a new parity metric.
    pub fn new(label: &str, matched: usize, total: usize) -> Self {
        Self {
            label: label.to_string(),
            matched,
            total,
        }
    }

    /// Returns the parity percentage (0.0 to 100.0).
    pub fn percentage(&self) -> f64 {
        if self.total == 0 {
            return 100.0;
        }
        self.matched as f64 / self.total as f64 * 100.0
    }

    /// Returns true if parity is 100%.
    pub fn is_full_parity(&self) -> bool {
        self.matched == self.total
    }
}

/// A performance benchmark entry with regression detection.
#[derive(Debug, Clone)]
pub struct BenchmarkEntry {
    /// Benchmark name (e.g., "render_grid_100", "physics_30frames").
    pub name: String,
    /// Current measurement in milliseconds.
    pub current_ms: f64,
    /// Baseline measurement in milliseconds (from previous known-good run).
    pub baseline_ms: f64,
    /// Regression threshold multiplier (e.g., 2.0 means >2x slower = regression).
    pub regression_threshold: f64,
}

impl BenchmarkEntry {
    /// Creates a new benchmark entry.
    pub fn new(name: &str, current_ms: f64, baseline_ms: f64, threshold: f64) -> Self {
        Self {
            name: name.to_string(),
            current_ms,
            baseline_ms,
            regression_threshold: threshold,
        }
    }

    /// Returns the ratio of current to baseline (1.0 = same, >1.0 = slower).
    pub fn ratio(&self) -> f64 {
        if self.baseline_ms <= 0.0 {
            return 1.0;
        }
        self.current_ms / self.baseline_ms
    }

    /// Returns true if performance regressed beyond the threshold.
    pub fn is_regression(&self) -> bool {
        self.ratio() > self.regression_threshold
    }

    /// Returns true if performance improved (current < baseline).
    pub fn is_improvement(&self) -> bool {
        self.current_ms < self.baseline_ms
    }

    /// Returns the delta as a percentage change from baseline.
    /// Positive = slower, negative = faster.
    pub fn delta_pct(&self) -> f64 {
        if self.baseline_ms <= 0.0 {
            return 0.0;
        }
        (self.current_ms - self.baseline_ms) / self.baseline_ms * 100.0
    }
}

/// Aggregated dashboard combining parity metrics and benchmark entries.
#[derive(Debug, Clone)]
pub struct Dashboard {
    /// Dashboard title/version label.
    pub title: String,
    /// Parity metrics across all subsystems.
    pub parity_metrics: Vec<ParityMetric>,
    /// Performance benchmark entries.
    pub benchmarks: Vec<BenchmarkEntry>,
}

impl Dashboard {
    /// Creates a new empty dashboard.
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            parity_metrics: Vec::new(),
            benchmarks: Vec::new(),
        }
    }

    /// Adds a parity metric.
    pub fn add_parity(&mut self, metric: ParityMetric) {
        self.parity_metrics.push(metric);
    }

    /// Adds a benchmark entry.
    pub fn add_benchmark(&mut self, entry: BenchmarkEntry) {
        self.benchmarks.push(entry);
    }

    /// Returns the combined parity across all metrics.
    pub fn combined_parity(&self) -> ParityMetric {
        let matched: usize = self.parity_metrics.iter().map(|m| m.matched).sum();
        let total: usize = self.parity_metrics.iter().map(|m| m.total).sum();
        ParityMetric::new("Combined", matched, total)
    }

    /// Returns the number of regressions detected.
    pub fn regression_count(&self) -> usize {
        self.benchmarks.iter().filter(|b| b.is_regression()).count()
    }

    /// Returns true if no regressions were detected.
    pub fn is_green(&self) -> bool {
        self.regression_count() == 0
    }

    /// Generates a human-readable ASCII dashboard report.
    pub fn render_report(&self) -> String {
        let mut out = String::new();

        // Header
        out.push_str(&format!(
            "================ {} ================\n\n",
            self.title
        ));

        // Parity section
        if !self.parity_metrics.is_empty() {
            out.push_str("PARITY\n");
            out.push_str(&format!(
                "{:<30} {:>8} {:>8} {:>8}\n",
                "Subsystem", "Match", "Total", "Parity"
            ));
            out.push_str(&format!("{:-<30} {:-<8} {:-<8} {:-<8}\n", "", "", "", ""));

            for m in &self.parity_metrics {
                out.push_str(&format!(
                    "{:<30} {:>8} {:>8} {:>7.1}%\n",
                    m.label,
                    m.matched,
                    m.total,
                    m.percentage()
                ));
            }

            let combined = self.combined_parity();
            out.push_str(&format!("{:-<30} {:-<8} {:-<8} {:-<8}\n", "", "", "", ""));
            out.push_str(&format!(
                "{:<30} {:>8} {:>8} {:>7.1}%\n",
                "COMBINED",
                combined.matched,
                combined.total,
                combined.percentage()
            ));
            out.push('\n');
        }

        // Benchmarks section
        if !self.benchmarks.is_empty() {
            out.push_str("BENCHMARKS\n");
            out.push_str(&format!(
                "{:<30} {:>10} {:>10} {:>8} {:>10}\n",
                "Benchmark", "Current", "Baseline", "Delta", "Status"
            ));
            out.push_str(&format!(
                "{:-<30} {:-<10} {:-<10} {:-<8} {:-<10}\n",
                "", "", "", "", ""
            ));

            for b in &self.benchmarks {
                let status = if b.is_regression() {
                    "REGRESS"
                } else if b.is_improvement() {
                    "IMPROVED"
                } else {
                    "OK"
                };
                out.push_str(&format!(
                    "{:<30} {:>8.3}ms {:>8.3}ms {:>+7.1}% {:>10}\n",
                    b.name,
                    b.current_ms,
                    b.baseline_ms,
                    b.delta_pct(),
                    status
                ));
            }

            out.push('\n');
            let reg_count = self.regression_count();
            if reg_count > 0 {
                out.push_str(&format!(
                    "REGRESSIONS DETECTED: {} benchmark(s) exceeded threshold\n",
                    reg_count
                ));
            } else {
                out.push_str("No regressions detected.\n");
            }
        }

        // Overall status
        out.push_str(&format!(
            "\nDashboard status: {}\n",
            if self.is_green() { "GREEN" } else { "RED" }
        ));

        out
    }
}

// ---------------------------------------------------------------------------
// FrameTimeStats — statistical summary of frame time samples
// ---------------------------------------------------------------------------

/// Statistical summary computed from a window of frame time samples.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameTimeStats {
    /// Minimum frame time in milliseconds.
    pub min_ms: f64,
    /// Maximum frame time in milliseconds.
    pub max_ms: f64,
    /// Mean frame time in milliseconds.
    pub avg_ms: f64,
    /// 99th percentile frame time in milliseconds.
    pub p99_ms: f64,
    /// Number of samples in this window.
    pub sample_count: usize,
}

impl FrameTimeStats {
    /// Computes statistics from a slice of frame time samples (in milliseconds).
    ///
    /// Returns `None` if the slice is empty.
    pub fn from_samples(samples: &[f64]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let n = samples.len();
        let min_ms = samples.iter().copied().fold(f64::INFINITY, f64::min);
        let max_ms = samples.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let sum: f64 = samples.iter().sum();
        let avg_ms = sum / n as f64;

        // p99: sort a copy, take the ceil(0.99 * n)-th element.
        let mut sorted = samples.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p99_idx = ((n as f64 * 0.99).ceil() as usize)
            .saturating_sub(1)
            .min(n - 1);
        let p99_ms = sorted[p99_idx];

        Some(Self {
            min_ms,
            max_ms,
            avg_ms,
            p99_ms,
            sample_count: n,
        })
    }

    /// Returns the implied FPS from the average frame time.
    pub fn avg_fps(&self) -> f64 {
        if self.avg_ms <= 0.0 {
            return 0.0;
        }
        1000.0 / self.avg_ms
    }

    /// Returns a JSON representation of the stats.
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"min_ms\":{:.4},",
                "\"max_ms\":{:.4},",
                "\"avg_ms\":{:.4},",
                "\"p99_ms\":{:.4},",
                "\"avg_fps\":{:.2},",
                "\"sample_count\":{}",
                "}}"
            ),
            self.min_ms,
            self.max_ms,
            self.avg_ms,
            self.p99_ms,
            self.avg_fps(),
            self.sample_count
        )
    }
}

// ---------------------------------------------------------------------------
// PhysicsStepMetrics — physics subsystem metrics
// ---------------------------------------------------------------------------

/// Metrics for the physics subsystem over a measurement window.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsStepMetrics {
    /// Average physics step duration in milliseconds.
    pub step_avg_ms: f64,
    /// Maximum physics step duration in milliseconds.
    pub step_max_ms: f64,
    /// Number of physics steps executed in the window.
    pub step_count: u64,
    /// Number of active physics bodies at the end of the window.
    pub body_count: u32,
    /// Target physics ticks per second (e.g. 60).
    pub target_tps: u32,
}

impl PhysicsStepMetrics {
    /// Creates metrics from a set of step durations and body count.
    pub fn from_step_times(step_times_ms: &[f64], body_count: u32, target_tps: u32) -> Self {
        let step_count = step_times_ms.len() as u64;
        let step_avg_ms = if step_count > 0 {
            step_times_ms.iter().sum::<f64>() / step_count as f64
        } else {
            0.0
        };
        let step_max_ms = step_times_ms.iter().copied().fold(0.0_f64, f64::max);

        Self {
            step_avg_ms,
            step_max_ms,
            step_count,
            body_count,
            target_tps,
        }
    }

    /// Returns the ratio of average step time to the budget (1000 / target_tps).
    /// Values > 1.0 mean physics can't keep up.
    pub fn budget_ratio(&self) -> f64 {
        if self.target_tps == 0 {
            return 0.0;
        }
        let budget_ms = 1000.0 / self.target_tps as f64;
        if budget_ms <= 0.0 {
            return 0.0;
        }
        self.step_avg_ms / budget_ms
    }

    /// Returns a JSON representation.
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"step_avg_ms\":{:.4},",
                "\"step_max_ms\":{:.4},",
                "\"step_count\":{},",
                "\"body_count\":{},",
                "\"target_tps\":{},",
                "\"budget_ratio\":{:.4}",
                "}}"
            ),
            self.step_avg_ms,
            self.step_max_ms,
            self.step_count,
            self.body_count,
            self.target_tps,
            self.budget_ratio()
        )
    }
}

// ---------------------------------------------------------------------------
// RenderMetrics — rendering subsystem metrics
// ---------------------------------------------------------------------------

/// Metrics for the rendering subsystem over a measurement window.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderMetrics {
    /// Average render time per frame in milliseconds.
    pub render_avg_ms: f64,
    /// Maximum render time per frame in milliseconds.
    pub render_max_ms: f64,
    /// Total draw calls in the window.
    pub draw_calls: u64,
    /// Total vertices submitted in the window.
    pub vertices: u64,
    /// Viewport width in pixels.
    pub viewport_width: u32,
    /// Viewport height in pixels.
    pub viewport_height: u32,
    /// Number of frames measured.
    pub frame_count: u64,
}

impl RenderMetrics {
    /// Creates render metrics from per-frame render times and aggregate counts.
    pub fn from_frame_times(
        render_times_ms: &[f64],
        draw_calls: u64,
        vertices: u64,
        viewport_width: u32,
        viewport_height: u32,
    ) -> Self {
        let frame_count = render_times_ms.len() as u64;
        let render_avg_ms = if frame_count > 0 {
            render_times_ms.iter().sum::<f64>() / frame_count as f64
        } else {
            0.0
        };
        let render_max_ms = render_times_ms.iter().copied().fold(0.0_f64, f64::max);

        Self {
            render_avg_ms,
            render_max_ms,
            draw_calls,
            vertices,
            viewport_width,
            viewport_height,
            frame_count,
        }
    }

    /// Returns the average draw calls per frame.
    pub fn avg_draw_calls_per_frame(&self) -> f64 {
        if self.frame_count == 0 {
            return 0.0;
        }
        self.draw_calls as f64 / self.frame_count as f64
    }

    /// Returns the average vertices per frame.
    pub fn avg_vertices_per_frame(&self) -> f64 {
        if self.frame_count == 0 {
            return 0.0;
        }
        self.vertices as f64 / self.frame_count as f64
    }

    /// Returns a JSON representation.
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"render_avg_ms\":{:.4},",
                "\"render_max_ms\":{:.4},",
                "\"draw_calls\":{},",
                "\"vertices\":{},",
                "\"viewport_width\":{},",
                "\"viewport_height\":{},",
                "\"frame_count\":{},",
                "\"avg_draw_calls_per_frame\":{:.2},",
                "\"avg_vertices_per_frame\":{:.2}",
                "}}"
            ),
            self.render_avg_ms,
            self.render_max_ms,
            self.draw_calls,
            self.vertices,
            self.viewport_width,
            self.viewport_height,
            self.frame_count,
            self.avg_draw_calls_per_frame(),
            self.avg_vertices_per_frame()
        )
    }
}

// ---------------------------------------------------------------------------
// RuntimeDashboard — full runtime metrics dashboard
// ---------------------------------------------------------------------------

/// A comprehensive runtime dashboard that aggregates frame time statistics,
/// physics step metrics, render metrics, parity metrics, and benchmarks.
///
/// This is the top-level type for the benchmark dashboard feature. It combines
/// all subsystem metrics into a single queryable, serializable object suitable
/// for CI reporting and in-editor display.
#[derive(Debug, Clone)]
pub struct RuntimeDashboard {
    /// Dashboard title/label.
    pub title: String,
    /// Frame time statistics (may be absent if no frames recorded).
    pub frame_stats: Option<FrameTimeStats>,
    /// Physics step metrics (may be absent if no physics steps recorded).
    pub physics_metrics: Option<PhysicsStepMetrics>,
    /// Render metrics (may be absent if no render frames recorded).
    pub render_metrics: Option<RenderMetrics>,
    /// Parity metrics across subsystems.
    pub parity_metrics: Vec<ParityMetric>,
    /// Performance benchmark entries with regression detection.
    pub benchmarks: Vec<BenchmarkEntry>,
}

impl RuntimeDashboard {
    /// Creates a new empty runtime dashboard.
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            frame_stats: None,
            physics_metrics: None,
            render_metrics: None,
            parity_metrics: Vec::new(),
            benchmarks: Vec::new(),
        }
    }

    /// Sets frame time statistics from raw samples.
    pub fn set_frame_times(&mut self, samples: &[f64]) {
        self.frame_stats = FrameTimeStats::from_samples(samples);
    }

    /// Sets physics step metrics.
    pub fn set_physics_metrics(&mut self, metrics: PhysicsStepMetrics) {
        self.physics_metrics = Some(metrics);
    }

    /// Sets render metrics.
    pub fn set_render_metrics(&mut self, metrics: RenderMetrics) {
        self.render_metrics = Some(metrics);
    }

    /// Adds a parity metric.
    pub fn add_parity(&mut self, metric: ParityMetric) {
        self.parity_metrics.push(metric);
    }

    /// Adds a benchmark entry.
    pub fn add_benchmark(&mut self, entry: BenchmarkEntry) {
        self.benchmarks.push(entry);
    }

    /// Returns the combined parity across all metrics.
    pub fn combined_parity(&self) -> ParityMetric {
        let matched: usize = self.parity_metrics.iter().map(|m| m.matched).sum();
        let total: usize = self.parity_metrics.iter().map(|m| m.total).sum();
        ParityMetric::new("Combined", matched, total)
    }

    /// Returns the number of benchmark regressions.
    pub fn regression_count(&self) -> usize {
        self.benchmarks.iter().filter(|b| b.is_regression()).count()
    }

    /// Returns true if all systems are healthy (no regressions, physics within budget).
    pub fn is_healthy(&self) -> bool {
        let no_regressions = self.regression_count() == 0;
        let physics_ok = self
            .physics_metrics
            .as_ref()
            .map_or(true, |p| p.budget_ratio() <= 1.0);
        no_regressions && physics_ok
    }

    /// Generates a full JSON report of all dashboard metrics.
    pub fn to_json(&self) -> String {
        let mut out = String::from("{");
        out.push_str(&format!("\"title\":\"{}\"", self.title));

        // Frame stats
        if let Some(ref fs) = self.frame_stats {
            out.push_str(",\"frame_stats\":");
            out.push_str(&fs.to_json());
        }

        // Physics metrics
        if let Some(ref pm) = self.physics_metrics {
            out.push_str(",\"physics_metrics\":");
            out.push_str(&pm.to_json());
        }

        // Render metrics
        if let Some(ref rm) = self.render_metrics {
            out.push_str(",\"render_metrics\":");
            out.push_str(&rm.to_json());
        }

        // Parity
        if !self.parity_metrics.is_empty() {
            out.push_str(",\"parity\":[");
            for (i, m) in self.parity_metrics.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!(
                    "{{\"label\":\"{}\",\"matched\":{},\"total\":{},\"pct\":{:.2}}}",
                    m.label,
                    m.matched,
                    m.total,
                    m.percentage()
                ));
            }
            out.push(']');
            let c = self.combined_parity();
            out.push_str(&format!(
                ",\"combined_parity\":{{\"matched\":{},\"total\":{},\"pct\":{:.2}}}",
                c.matched,
                c.total,
                c.percentage()
            ));
        }

        // Benchmarks
        if !self.benchmarks.is_empty() {
            out.push_str(",\"benchmarks\":[");
            for (i, b) in self.benchmarks.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let status = if b.is_regression() {
                    "regression"
                } else if b.is_improvement() {
                    "improvement"
                } else {
                    "ok"
                };
                out.push_str(&format!(
                    concat!(
                        "{{\"name\":\"{}\",",
                        "\"current_ms\":{:.4},",
                        "\"baseline_ms\":{:.4},",
                        "\"ratio\":{:.4},",
                        "\"status\":\"{}\"}}"
                    ),
                    b.name,
                    b.current_ms,
                    b.baseline_ms,
                    b.ratio(),
                    status
                ));
            }
            out.push(']');
        }

        out.push_str(&format!(",\"healthy\":{}", self.is_healthy()));
        out.push_str(&format!(
            ",\"regression_count\":{}",
            self.regression_count()
        ));
        out.push('}');
        out
    }

    /// Generates a human-readable ASCII report covering all sections.
    pub fn render_report(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!(
            "================ {} ================\n\n",
            self.title
        ));

        // Frame time section
        if let Some(ref fs) = self.frame_stats {
            out.push_str("FRAME TIME\n");
            out.push_str(&format!("  Min:    {:>8.3} ms\n", fs.min_ms));
            out.push_str(&format!(
                "  Avg:    {:>8.3} ms ({:.1} FPS)\n",
                fs.avg_ms,
                fs.avg_fps()
            ));
            out.push_str(&format!("  Max:    {:>8.3} ms\n", fs.max_ms));
            out.push_str(&format!("  P99:    {:>8.3} ms\n", fs.p99_ms));
            out.push_str(&format!("  Samples: {}\n\n", fs.sample_count));
        }

        // Physics section
        if let Some(ref pm) = self.physics_metrics {
            out.push_str("PHYSICS\n");
            out.push_str(&format!("  Step avg:  {:>8.3} ms\n", pm.step_avg_ms));
            out.push_str(&format!("  Step max:  {:>8.3} ms\n", pm.step_max_ms));
            out.push_str(&format!("  Steps:     {:>8}\n", pm.step_count));
            out.push_str(&format!("  Bodies:    {:>8}\n", pm.body_count));
            out.push_str(&format!("  Target:    {:>8} TPS\n", pm.target_tps));
            out.push_str(&format!(
                "  Budget:    {:>7.1}%\n\n",
                pm.budget_ratio() * 100.0
            ));
        }

        // Render section
        if let Some(ref rm) = self.render_metrics {
            out.push_str("RENDER\n");
            out.push_str(&format!("  Render avg: {:>7.3} ms\n", rm.render_avg_ms));
            out.push_str(&format!("  Render max: {:>7.3} ms\n", rm.render_max_ms));
            out.push_str(&format!(
                "  Draw calls: {:>7.1}/frame\n",
                rm.avg_draw_calls_per_frame()
            ));
            out.push_str(&format!(
                "  Vertices:   {:>7.0}/frame\n",
                rm.avg_vertices_per_frame()
            ));
            out.push_str(&format!(
                "  Viewport:   {}x{}\n",
                rm.viewport_width, rm.viewport_height
            ));
            out.push_str(&format!("  Frames:     {}\n\n", rm.frame_count));
        }

        // Parity section
        if !self.parity_metrics.is_empty() {
            out.push_str("PARITY\n");
            out.push_str(&format!(
                "  {:<28} {:>6} {:>6} {:>7}\n",
                "Subsystem", "Match", "Total", "Parity"
            ));
            out.push_str(&format!("  {:-<28} {:-<6} {:-<6} {:-<7}\n", "", "", "", ""));
            for m in &self.parity_metrics {
                out.push_str(&format!(
                    "  {:<28} {:>6} {:>6} {:>6.1}%\n",
                    m.label,
                    m.matched,
                    m.total,
                    m.percentage()
                ));
            }
            let combined = self.combined_parity();
            out.push_str(&format!("  {:-<28} {:-<6} {:-<6} {:-<7}\n", "", "", "", ""));
            out.push_str(&format!(
                "  {:<28} {:>6} {:>6} {:>6.1}%\n\n",
                "COMBINED",
                combined.matched,
                combined.total,
                combined.percentage()
            ));
        }

        // Benchmarks section
        if !self.benchmarks.is_empty() {
            out.push_str("BENCHMARKS\n");
            out.push_str(&format!(
                "  {:<26} {:>9} {:>9} {:>7} {:>9}\n",
                "Name", "Current", "Baseline", "Delta", "Status"
            ));
            out.push_str(&format!(
                "  {:-<26} {:-<9} {:-<9} {:-<7} {:-<9}\n",
                "", "", "", "", ""
            ));
            for b in &self.benchmarks {
                let status = if b.is_regression() {
                    "REGRESS"
                } else if b.is_improvement() {
                    "IMPROVED"
                } else {
                    "OK"
                };
                out.push_str(&format!(
                    "  {:<26} {:>7.3}ms {:>7.3}ms {:>+6.1}% {:>9}\n",
                    b.name,
                    b.current_ms,
                    b.baseline_ms,
                    b.delta_pct(),
                    status
                ));
            }
            out.push('\n');
        }

        // Overall status
        out.push_str(&format!(
            "Status: {}\n",
            if self.is_healthy() {
                "HEALTHY"
            } else {
                "UNHEALTHY"
            }
        ));

        out
    }
}

// ---------------------------------------------------------------------------
// BenchmarkBaseline — serializable baseline entry
// ---------------------------------------------------------------------------

/// A single baseline measurement for serialization/deserialization.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkBaseline {
    /// Benchmark name (must match the entry name).
    pub name: String,
    /// Baseline time in milliseconds.
    pub ms: f64,
    /// Regression threshold multiplier (e.g., 2.0 = fail if >2x slower).
    pub threshold: f64,
}

impl BenchmarkBaseline {
    /// Creates a new baseline.
    pub fn new(name: &str, ms: f64, threshold: f64) -> Self {
        Self {
            name: name.to_string(),
            ms,
            threshold,
        }
    }
}

// ---------------------------------------------------------------------------
// BenchmarkGate — CI gate with regression detection
// ---------------------------------------------------------------------------

/// CI gate result for a single benchmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateVerdict {
    /// Benchmark passed (within threshold).
    Pass,
    /// Benchmark regressed beyond threshold.
    Fail,
    /// No baseline found — skipped (neither pass nor fail).
    Skip,
}

/// Result of running the benchmark gate.
#[derive(Debug, Clone)]
pub struct GateResult {
    /// Name of the benchmark.
    pub name: String,
    /// Current measurement in ms.
    pub current_ms: f64,
    /// Baseline measurement in ms (0.0 if no baseline).
    pub baseline_ms: f64,
    /// Ratio of current/baseline.
    pub ratio: f64,
    /// The threshold that was applied.
    pub threshold: f64,
    /// Pass/Fail/Skip verdict.
    pub verdict: GateVerdict,
}

/// CI benchmark gate that compares current results against baselines
/// and produces a pass/fail decision.
///
/// Usage:
/// 1. Load baselines from a previous known-good run.
/// 2. Submit current measurements.
/// 3. Call `evaluate()` to get pass/fail results.
/// 4. Call `gate_passed()` to determine if CI should proceed.
#[derive(Debug, Clone)]
pub struct BenchmarkGate {
    /// Baselines keyed by benchmark name.
    baselines: Vec<BenchmarkBaseline>,
    /// Default threshold if a benchmark has no explicit baseline threshold.
    default_threshold: f64,
    /// Whether to fail on missing baselines (strict mode).
    strict: bool,
}

impl BenchmarkGate {
    /// Creates a new gate with the given default threshold.
    pub fn new(default_threshold: f64) -> Self {
        Self {
            baselines: Vec::new(),
            default_threshold,
            strict: false,
        }
    }

    /// Enables strict mode: benchmarks without baselines are treated as failures.
    pub fn strict(mut self) -> Self {
        self.strict = true;
        self
    }

    /// Adds a baseline entry.
    pub fn add_baseline(&mut self, baseline: BenchmarkBaseline) {
        self.baselines.push(baseline);
    }

    /// Loads multiple baselines at once.
    pub fn load_baselines(&mut self, baselines: Vec<BenchmarkBaseline>) {
        self.baselines = baselines;
    }

    /// Returns the number of loaded baselines.
    pub fn baseline_count(&self) -> usize {
        self.baselines.len()
    }

    /// Finds the baseline for a given benchmark name.
    pub fn get_baseline(&self, name: &str) -> Option<&BenchmarkBaseline> {
        self.baselines.iter().find(|b| b.name == name)
    }

    /// Evaluates a single benchmark measurement against its baseline.
    pub fn evaluate_one(&self, name: &str, current_ms: f64) -> GateResult {
        match self.get_baseline(name) {
            Some(baseline) => {
                let ratio = if baseline.ms <= 0.0 {
                    1.0
                } else {
                    current_ms / baseline.ms
                };
                let threshold = baseline.threshold;
                let verdict = if ratio > threshold {
                    GateVerdict::Fail
                } else {
                    GateVerdict::Pass
                };
                GateResult {
                    name: name.to_string(),
                    current_ms,
                    baseline_ms: baseline.ms,
                    ratio,
                    threshold,
                    verdict,
                }
            }
            None => {
                let verdict = if self.strict {
                    GateVerdict::Fail
                } else {
                    GateVerdict::Skip
                };
                GateResult {
                    name: name.to_string(),
                    current_ms,
                    baseline_ms: 0.0,
                    ratio: 1.0,
                    threshold: self.default_threshold,
                    verdict,
                }
            }
        }
    }

    /// Evaluates multiple benchmark measurements and returns all results.
    pub fn evaluate(&self, measurements: &[(&str, f64)]) -> Vec<GateResult> {
        measurements
            .iter()
            .map(|(name, ms)| self.evaluate_one(name, *ms))
            .collect()
    }

    /// Returns true if all evaluated benchmarks passed (no failures).
    pub fn gate_passed(results: &[GateResult]) -> bool {
        !results.iter().any(|r| r.verdict == GateVerdict::Fail)
    }

    /// Returns only the failures from a set of results.
    pub fn failures(results: &[GateResult]) -> Vec<&GateResult> {
        results
            .iter()
            .filter(|r| r.verdict == GateVerdict::Fail)
            .collect()
    }

    /// Generates a CI-friendly summary string.
    pub fn summary(results: &[GateResult]) -> String {
        let mut out = String::new();
        let pass_count = results
            .iter()
            .filter(|r| r.verdict == GateVerdict::Pass)
            .count();
        let fail_count = results
            .iter()
            .filter(|r| r.verdict == GateVerdict::Fail)
            .count();
        let skip_count = results
            .iter()
            .filter(|r| r.verdict == GateVerdict::Skip)
            .count();

        out.push_str(&format!(
            "Benchmark gate: {} passed, {} failed, {} skipped\n",
            pass_count, fail_count, skip_count
        ));

        for r in results {
            let icon = match r.verdict {
                GateVerdict::Pass => "PASS",
                GateVerdict::Fail => "FAIL",
                GateVerdict::Skip => "SKIP",
            };
            if r.baseline_ms > 0.0 {
                out.push_str(&format!(
                    "  [{}] {} — {:.3}ms / {:.3}ms ({:.1}x, threshold {:.1}x)\n",
                    icon, r.name, r.current_ms, r.baseline_ms, r.ratio, r.threshold
                ));
            } else {
                out.push_str(&format!(
                    "  [{}] {} — {:.3}ms (no baseline)\n",
                    icon, r.name, r.current_ms
                ));
            }
        }

        if fail_count > 0 {
            out.push_str("GATE: FAILED\n");
        } else {
            out.push_str("GATE: PASSED\n");
        }
        out
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- ParityMetric -------------------------------------------------------

    #[test]
    fn parity_metric_percentage() {
        let m = ParityMetric::new("test", 75, 100);
        assert!((m.percentage() - 75.0).abs() < 0.001);
    }

    #[test]
    fn parity_metric_full_parity() {
        let m = ParityMetric::new("test", 10, 10);
        assert!(m.is_full_parity());
        assert!((m.percentage() - 100.0).abs() < 0.001);
    }

    #[test]
    fn parity_metric_zero_total() {
        let m = ParityMetric::new("empty", 0, 0);
        assert!((m.percentage() - 100.0).abs() < 0.001);
        assert!(m.is_full_parity());
    }

    // -- BenchmarkEntry -----------------------------------------------------

    #[test]
    fn benchmark_no_regression_when_within_threshold() {
        let b = BenchmarkEntry::new("render", 10.0, 8.0, 2.0);
        assert!(!b.is_regression()); // 10/8 = 1.25x, under 2.0 threshold
        assert!((b.ratio() - 1.25).abs() < 0.001);
    }

    #[test]
    fn benchmark_regression_when_exceeds_threshold() {
        let b = BenchmarkEntry::new("render", 20.0, 8.0, 2.0);
        assert!(b.is_regression()); // 20/8 = 2.5x, above 2.0 threshold
    }

    #[test]
    fn benchmark_improvement_detected() {
        let b = BenchmarkEntry::new("render", 5.0, 10.0, 2.0);
        assert!(b.is_improvement());
        assert!(!b.is_regression());
        assert!((b.delta_pct() - (-50.0)).abs() < 0.001);
    }

    #[test]
    fn benchmark_delta_pct_positive_for_slower() {
        let b = BenchmarkEntry::new("test", 15.0, 10.0, 2.0);
        assert!((b.delta_pct() - 50.0).abs() < 0.001);
    }

    #[test]
    fn benchmark_zero_baseline_safe() {
        let b = BenchmarkEntry::new("test", 5.0, 0.0, 2.0);
        assert!((b.ratio() - 1.0).abs() < 0.001);
        assert!((b.delta_pct() - 0.0).abs() < 0.001);
        assert!(!b.is_regression());
    }

    // -- Dashboard ----------------------------------------------------------

    #[test]
    fn dashboard_combined_parity() {
        let mut d = Dashboard::new("test");
        d.add_parity(ParityMetric::new("methods", 80, 100));
        d.add_parity(ParityMetric::new("properties", 50, 50));
        d.add_parity(ParityMetric::new("signals", 20, 30));

        let combined = d.combined_parity();
        assert_eq!(combined.matched, 150);
        assert_eq!(combined.total, 180);
        assert!((combined.percentage() - 83.333).abs() < 0.01);
    }

    #[test]
    fn dashboard_is_green_when_no_regressions() {
        let mut d = Dashboard::new("green");
        d.add_benchmark(BenchmarkEntry::new("a", 10.0, 10.0, 2.0));
        d.add_benchmark(BenchmarkEntry::new("b", 5.0, 10.0, 2.0));
        assert!(d.is_green());
        assert_eq!(d.regression_count(), 0);
    }

    #[test]
    fn dashboard_is_red_when_regressions_exist() {
        let mut d = Dashboard::new("red");
        d.add_benchmark(BenchmarkEntry::new("a", 10.0, 10.0, 2.0));
        d.add_benchmark(BenchmarkEntry::new("b", 25.0, 10.0, 2.0)); // regression
        assert!(!d.is_green());
        assert_eq!(d.regression_count(), 1);
    }

    #[test]
    fn dashboard_empty_is_green() {
        let d = Dashboard::new("empty");
        assert!(d.is_green());
    }

    #[test]
    fn dashboard_render_report_contains_sections() {
        let mut d = Dashboard::new("Patina Runtime v0.1");
        d.add_parity(ParityMetric::new("ClassDB methods", 392, 511));
        d.add_parity(ParityMetric::new("Lifecycle traces", 71, 71));
        d.add_benchmark(BenchmarkEntry::new("render_grid_100", 11.1, 11.5, 2.0));
        d.add_benchmark(BenchmarkEntry::new("physics_30frames", 0.5, 0.6, 2.0));

        let report = d.render_report();
        assert!(report.contains("Patina Runtime v0.1"));
        assert!(report.contains("PARITY"));
        assert!(report.contains("ClassDB methods"));
        assert!(report.contains("392"));
        assert!(report.contains("511"));
        assert!(report.contains("BENCHMARKS"));
        assert!(report.contains("render_grid_100"));
        assert!(report.contains("GREEN"));
    }

    #[test]
    fn dashboard_render_report_shows_regression() {
        let mut d = Dashboard::new("test");
        d.add_benchmark(BenchmarkEntry::new("slow_test", 30.0, 10.0, 2.0));
        let report = d.render_report();
        assert!(report.contains("REGRESS"));
        assert!(report.contains("RED"));
    }

    #[test]
    fn dashboard_render_report_shows_improvement() {
        let mut d = Dashboard::new("test");
        d.add_benchmark(BenchmarkEntry::new("fast_test", 5.0, 10.0, 2.0));
        let report = d.render_report();
        assert!(report.contains("IMPROVED"));
        assert!(report.contains("GREEN"));
    }

    #[test]
    fn dashboard_combined_parity_empty() {
        let d = Dashboard::new("empty");
        let combined = d.combined_parity();
        assert_eq!(combined.matched, 0);
        assert_eq!(combined.total, 0);
        assert!((combined.percentage() - 100.0).abs() < 0.001);
    }

    #[test]
    fn parity_metric_partial() {
        let m = ParityMetric::new("test", 3, 7);
        assert!(!m.is_full_parity());
        assert!((m.percentage() - 42.857).abs() < 0.01);
    }

    #[test]
    fn benchmark_exact_baseline_match() {
        let b = BenchmarkEntry::new("stable", 10.0, 10.0, 2.0);
        assert!(!b.is_regression());
        assert!(!b.is_improvement());
        assert!((b.ratio() - 1.0).abs() < 0.001);
        assert!((b.delta_pct() - 0.0).abs() < 0.001);
    }

    // -- FrameTimeStats -----------------------------------------------------

    #[test]
    fn frame_time_stats_basic() {
        let samples = vec![16.0, 17.0, 15.0, 16.5, 16.0];
        let stats = FrameTimeStats::from_samples(&samples).unwrap();
        assert!((stats.min_ms - 15.0).abs() < 0.001);
        assert!((stats.max_ms - 17.0).abs() < 0.001);
        assert!((stats.avg_ms - 16.1).abs() < 0.001);
        assert_eq!(stats.sample_count, 5);
    }

    #[test]
    fn frame_time_stats_empty_returns_none() {
        assert!(FrameTimeStats::from_samples(&[]).is_none());
    }

    #[test]
    fn frame_time_stats_single_sample() {
        let stats = FrameTimeStats::from_samples(&[8.0]).unwrap();
        assert!((stats.min_ms - 8.0).abs() < 0.001);
        assert!((stats.max_ms - 8.0).abs() < 0.001);
        assert!((stats.avg_ms - 8.0).abs() < 0.001);
        assert!((stats.p99_ms - 8.0).abs() < 0.001);
    }

    #[test]
    fn frame_time_stats_p99() {
        // 100 samples: 1.0 through 100.0
        let samples: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let stats = FrameTimeStats::from_samples(&samples).unwrap();
        // p99 should be at index ceil(0.99 * 100) - 1 = 98, value = 99.0
        assert!((stats.p99_ms - 99.0).abs() < 0.001);
    }

    #[test]
    fn frame_time_stats_fps() {
        let stats = FrameTimeStats::from_samples(&[16.667]).unwrap();
        assert!((stats.avg_fps() - 59.999).abs() < 0.1);
    }

    #[test]
    fn frame_time_stats_json_parseable() {
        let stats = FrameTimeStats::from_samples(&[10.0, 20.0]).unwrap();
        let json = stats.to_json();
        assert!(json.starts_with('{'));
        assert!(json.ends_with('}'));
        assert!(json.contains("\"min_ms\""));
        assert!(json.contains("\"avg_fps\""));
    }

    // -- PhysicsStepMetrics -------------------------------------------------

    #[test]
    fn physics_metrics_basic() {
        let steps = vec![0.5, 0.6, 0.4, 0.7, 0.5];
        let pm = PhysicsStepMetrics::from_step_times(&steps, 10, 60);
        assert!((pm.step_avg_ms - 0.54).abs() < 0.001);
        assert!((pm.step_max_ms - 0.7).abs() < 0.001);
        assert_eq!(pm.step_count, 5);
        assert_eq!(pm.body_count, 10);
        assert_eq!(pm.target_tps, 60);
    }

    #[test]
    fn physics_metrics_budget_ratio() {
        // At 60 TPS, budget = 16.667ms. step_avg = 8.333ms → ratio = 0.5
        let pm = PhysicsStepMetrics::from_step_times(&[8.333], 5, 60);
        assert!((pm.budget_ratio() - 0.5).abs() < 0.01);
    }

    #[test]
    fn physics_metrics_over_budget() {
        // At 60 TPS, budget = 16.667ms. step_avg = 20ms → ratio > 1.0
        let pm = PhysicsStepMetrics::from_step_times(&[20.0], 5, 60);
        assert!(pm.budget_ratio() > 1.0);
    }

    #[test]
    fn physics_metrics_empty_steps() {
        let pm = PhysicsStepMetrics::from_step_times(&[], 0, 60);
        assert_eq!(pm.step_count, 0);
        assert!((pm.step_avg_ms - 0.0).abs() < 0.001);
    }

    #[test]
    fn physics_metrics_json() {
        let pm = PhysicsStepMetrics::from_step_times(&[1.0], 3, 60);
        let json = pm.to_json();
        assert!(json.contains("\"body_count\":3"));
        assert!(json.contains("\"target_tps\":60"));
    }

    // -- RenderMetrics ------------------------------------------------------

    #[test]
    fn render_metrics_basic() {
        let times = vec![2.0, 3.0, 2.5];
        let rm = RenderMetrics::from_frame_times(&times, 300, 90000, 1920, 1080);
        assert!((rm.render_avg_ms - 2.5).abs() < 0.001);
        assert!((rm.render_max_ms - 3.0).abs() < 0.001);
        assert_eq!(rm.draw_calls, 300);
        assert_eq!(rm.vertices, 90000);
        assert_eq!(rm.frame_count, 3);
    }

    #[test]
    fn render_metrics_per_frame() {
        let rm = RenderMetrics::from_frame_times(&[1.0, 2.0], 100, 6000, 800, 600);
        assert!((rm.avg_draw_calls_per_frame() - 50.0).abs() < 0.001);
        assert!((rm.avg_vertices_per_frame() - 3000.0).abs() < 0.001);
    }

    #[test]
    fn render_metrics_empty() {
        let rm = RenderMetrics::from_frame_times(&[], 0, 0, 800, 600);
        assert_eq!(rm.frame_count, 0);
        assert!((rm.avg_draw_calls_per_frame() - 0.0).abs() < 0.001);
    }

    #[test]
    fn render_metrics_json() {
        let rm = RenderMetrics::from_frame_times(&[5.0], 10, 500, 1920, 1080);
        let json = rm.to_json();
        assert!(json.contains("\"viewport_width\":1920"));
        assert!(json.contains("\"viewport_height\":1080"));
    }

    // -- RuntimeDashboard ---------------------------------------------------

    #[test]
    fn runtime_dashboard_empty_is_healthy() {
        let d = RuntimeDashboard::new("test");
        assert!(d.is_healthy());
    }

    #[test]
    fn runtime_dashboard_with_all_metrics() {
        let mut d = RuntimeDashboard::new("Patina v0.1");
        d.set_frame_times(&[16.0, 16.5, 17.0, 15.5, 16.2]);
        d.set_physics_metrics(PhysicsStepMetrics::from_step_times(
            &[0.5, 0.6, 0.4],
            10,
            60,
        ));
        d.set_render_metrics(RenderMetrics::from_frame_times(
            &[2.0, 2.5, 3.0],
            150,
            45000,
            1920,
            1080,
        ));
        d.add_parity(ParityMetric::new("ClassDB", 400, 500));
        d.add_benchmark(BenchmarkEntry::new("render", 10.0, 10.0, 2.0));

        assert!(d.frame_stats.is_some());
        assert!(d.physics_metrics.is_some());
        assert!(d.render_metrics.is_some());
        assert!(d.is_healthy());
    }

    #[test]
    fn runtime_dashboard_unhealthy_regression() {
        let mut d = RuntimeDashboard::new("test");
        d.add_benchmark(BenchmarkEntry::new("slow", 30.0, 10.0, 2.0));
        assert!(!d.is_healthy());
        assert_eq!(d.regression_count(), 1);
    }

    #[test]
    fn runtime_dashboard_unhealthy_physics() {
        let mut d = RuntimeDashboard::new("test");
        // Physics over budget: step takes 20ms at 60 TPS (budget=16.667ms)
        d.set_physics_metrics(PhysicsStepMetrics::from_step_times(&[20.0], 5, 60));
        assert!(!d.is_healthy());
    }

    #[test]
    fn runtime_dashboard_json_contains_all_sections() {
        let mut d = RuntimeDashboard::new("full");
        d.set_frame_times(&[16.0, 17.0]);
        d.set_physics_metrics(PhysicsStepMetrics::from_step_times(&[0.5], 3, 60));
        d.set_render_metrics(RenderMetrics::from_frame_times(&[2.0], 10, 500, 800, 600));
        d.add_parity(ParityMetric::new("test", 5, 10));
        d.add_benchmark(BenchmarkEntry::new("b", 1.0, 1.0, 2.0));

        let json = d.to_json();
        assert!(json.contains("\"title\":\"full\""));
        assert!(json.contains("\"frame_stats\""));
        assert!(json.contains("\"physics_metrics\""));
        assert!(json.contains("\"render_metrics\""));
        assert!(json.contains("\"parity\""));
        assert!(json.contains("\"benchmarks\""));
        assert!(json.contains("\"healthy\":true"));
    }

    #[test]
    fn runtime_dashboard_report_contains_sections() {
        let mut d = RuntimeDashboard::new("Report Test");
        d.set_frame_times(&[16.0, 17.0]);
        d.set_physics_metrics(PhysicsStepMetrics::from_step_times(&[0.5], 3, 60));
        d.set_render_metrics(RenderMetrics::from_frame_times(&[2.0], 10, 500, 800, 600));
        d.add_parity(ParityMetric::new("ClassDB", 400, 500));
        d.add_benchmark(BenchmarkEntry::new("render", 10.0, 10.0, 2.0));

        let report = d.render_report();
        assert!(report.contains("Report Test"));
        assert!(report.contains("FRAME TIME"));
        assert!(report.contains("PHYSICS"));
        assert!(report.contains("RENDER"));
        assert!(report.contains("PARITY"));
        assert!(report.contains("BENCHMARKS"));
        assert!(report.contains("HEALTHY"));
    }

    #[test]
    fn runtime_dashboard_report_shows_unhealthy() {
        let mut d = RuntimeDashboard::new("bad");
        d.add_benchmark(BenchmarkEntry::new("slow", 30.0, 10.0, 2.0));
        let report = d.render_report();
        assert!(report.contains("UNHEALTHY"));
    }
}
