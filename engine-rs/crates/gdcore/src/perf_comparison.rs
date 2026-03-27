//! Performance comparison report: Patina Engine vs upstream Godot.
//!
//! Provides a structured framework for comparing benchmarks between
//! the Patina Rust engine and the upstream Godot C++ engine. Supports
//! multiple subsystem categories, configurable tolerance thresholds,
//! CI-friendly text and JSON output, and summary verdicts.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use gdcore::perf_comparison::{ComparisonReport, SubsystemBenchmark, Measurement};
//!
//! let mut report = ComparisonReport::new("4.6.1", "0.1.0");
//! report.add(SubsystemBenchmark::new(
//!     "scene_tree",
//!     "Node instantiation (1000 nodes)",
//!     Measurement::new(12.5, "ms"),
//!     Measurement::new(14.2, "ms"),
//! ));
//! println!("{}", report.render_text());
//! ```

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

/// A single measurement value with its unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// The measured value.
    pub value: f64,
    /// Unit string (e.g., "ms", "MB", "fps", "ops/s").
    pub unit: String,
}

impl Measurement {
    /// Creates a new measurement.
    pub fn new(value: f64, unit: &str) -> Self {
        Self {
            value,
            unit: unit.to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// SubsystemBenchmark
// ---------------------------------------------------------------------------

/// A single benchmark comparing Godot and Patina for one metric.
#[derive(Debug, Clone)]
pub struct SubsystemBenchmark {
    /// Subsystem category (e.g., "scene_tree", "physics", "render", "resource").
    pub subsystem: String,
    /// Description of what's being measured.
    pub description: String,
    /// Godot (upstream) measurement.
    pub godot: Measurement,
    /// Patina measurement.
    pub patina: Measurement,
    /// Whether higher is better (true for fps/ops, false for ms/MB).
    pub higher_is_better: bool,
}

impl SubsystemBenchmark {
    /// Creates a new benchmark where lower values are better (default).
    pub fn new(
        subsystem: &str,
        description: &str,
        godot: Measurement,
        patina: Measurement,
    ) -> Self {
        Self {
            subsystem: subsystem.to_owned(),
            description: description.to_owned(),
            godot,
            patina,
            higher_is_better: false,
        }
    }

    /// Creates a benchmark where higher values are better (e.g., fps, ops/s).
    pub fn new_higher_better(
        subsystem: &str,
        description: &str,
        godot: Measurement,
        patina: Measurement,
    ) -> Self {
        Self {
            subsystem: subsystem.to_owned(),
            description: description.to_owned(),
            godot,
            patina,
            higher_is_better: true,
        }
    }

    /// Returns the ratio of Patina to Godot values.
    /// For lower-is-better: <1.0 means Patina is faster.
    /// For higher-is-better: >1.0 means Patina is faster.
    pub fn ratio(&self) -> f64 {
        if self.godot.value.abs() < f64::EPSILON {
            return 1.0;
        }
        self.patina.value / self.godot.value
    }

    /// Returns the delta as a percentage.
    /// Positive means Patina is worse, negative means Patina is better
    /// (adjusted for higher_is_better).
    pub fn delta_pct(&self) -> f64 {
        let ratio = self.ratio();
        if self.higher_is_better {
            // Higher is better: if ratio > 1.0, Patina is better → negative delta
            (1.0 - ratio) * 100.0
        } else {
            // Lower is better: if ratio < 1.0, Patina is better → negative delta
            (ratio - 1.0) * 100.0
        }
    }

    /// Returns the verdict for this benchmark given a tolerance threshold.
    /// `tolerance_pct` is how much worse Patina can be before it's "worse"
    /// (e.g., 10.0 means up to 10% worse is still "comparable").
    pub fn verdict(&self, tolerance_pct: f64) -> Verdict {
        let delta = self.delta_pct();
        if delta < -5.0 {
            Verdict::PatinaFaster
        } else if delta <= tolerance_pct {
            Verdict::Comparable
        } else {
            Verdict::GodotFaster
        }
    }
}

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

/// Performance comparison verdict for a single benchmark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Patina is measurably faster.
    PatinaFaster,
    /// Performance is comparable (within tolerance).
    Comparable,
    /// Godot is measurably faster.
    GodotFaster,
}

impl Verdict {
    /// Returns a human-readable label.
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::PatinaFaster => "PATINA FASTER",
            Verdict::Comparable => "COMPARABLE",
            Verdict::GodotFaster => "GODOT FASTER",
        }
    }

    /// Returns a short symbol for compact display.
    pub fn symbol(&self) -> &'static str {
        match self {
            Verdict::PatinaFaster => "+",
            Verdict::Comparable => "=",
            Verdict::GodotFaster => "-",
        }
    }
}

// ---------------------------------------------------------------------------
// ComparisonReport
// ---------------------------------------------------------------------------

/// A full performance comparison report between Godot and Patina.
#[derive(Debug, Clone)]
pub struct ComparisonReport {
    /// Godot version being compared against.
    pub godot_version: String,
    /// Patina version being compared.
    pub patina_version: String,
    /// Tolerance percentage: how much worse Patina can be and still be "comparable".
    pub tolerance_pct: f64,
    /// Individual benchmarks.
    pub benchmarks: Vec<SubsystemBenchmark>,
}

impl ComparisonReport {
    /// Creates a new empty comparison report with default 10% tolerance.
    pub fn new(godot_version: &str, patina_version: &str) -> Self {
        Self {
            godot_version: godot_version.to_owned(),
            patina_version: patina_version.to_owned(),
            tolerance_pct: 10.0,
            benchmarks: Vec::new(),
        }
    }

    /// Sets the tolerance threshold.
    pub fn with_tolerance(mut self, pct: f64) -> Self {
        self.tolerance_pct = pct;
        self
    }

    /// Adds a benchmark to the report.
    pub fn add(&mut self, benchmark: SubsystemBenchmark) {
        self.benchmarks.push(benchmark);
    }

    /// Returns the number of benchmarks.
    pub fn benchmark_count(&self) -> usize {
        self.benchmarks.len()
    }

    /// Returns verdicts for all benchmarks.
    pub fn verdicts(&self) -> Vec<(&SubsystemBenchmark, Verdict)> {
        self.benchmarks
            .iter()
            .map(|b| (b, b.verdict(self.tolerance_pct)))
            .collect()
    }

    /// Counts how many benchmarks have each verdict.
    pub fn verdict_counts(&self) -> (usize, usize, usize) {
        let mut faster = 0;
        let mut comparable = 0;
        let mut slower = 0;
        for b in &self.benchmarks {
            match b.verdict(self.tolerance_pct) {
                Verdict::PatinaFaster => faster += 1,
                Verdict::Comparable => comparable += 1,
                Verdict::GodotFaster => slower += 1,
            }
        }
        (faster, comparable, slower)
    }

    /// Returns benchmarks grouped by subsystem.
    pub fn by_subsystem(&self) -> HashMap<&str, Vec<&SubsystemBenchmark>> {
        let mut map: HashMap<&str, Vec<&SubsystemBenchmark>> = HashMap::new();
        for b in &self.benchmarks {
            map.entry(&b.subsystem).or_default().push(b);
        }
        map
    }

    /// Returns all unique subsystem names, sorted.
    pub fn subsystems(&self) -> Vec<&str> {
        let mut subs: Vec<&str> = self
            .benchmarks
            .iter()
            .map(|b| b.subsystem.as_str())
            .collect();
        subs.sort();
        subs.dedup();
        subs
    }

    /// Returns an overall summary verdict.
    pub fn overall_verdict(&self) -> OverallVerdict {
        if self.benchmarks.is_empty() {
            return OverallVerdict::NoData;
        }
        let (faster, comparable, slower) = self.verdict_counts();
        if slower == 0 && faster > 0 {
            OverallVerdict::PatinaWins
        } else if slower == 0 {
            OverallVerdict::Comparable
        } else if slower <= self.benchmarks.len() / 4 {
            OverallVerdict::MostlyComparable
        } else {
            OverallVerdict::NeedsWork
        }
    }

    /// Renders a human-readable text report.
    pub fn render_text(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!(
            "=== Performance Comparison: Godot {} vs Patina {} ===\n",
            self.godot_version, self.patina_version
        ));
        out.push_str(&format!(
            "Tolerance: {:.0}% (within this range = comparable)\n\n",
            self.tolerance_pct
        ));

        if self.benchmarks.is_empty() {
            out.push_str("No benchmarks recorded.\n");
            return out;
        }

        // Group by subsystem.
        let subsystems = self.subsystems();
        for sub in &subsystems {
            out.push_str(&format!("--- {} ---\n", sub));
            out.push_str(&format!(
                "{:<40} {:>10} {:>10} {:>8} {:>15}\n",
                "Benchmark", "Godot", "Patina", "Delta", "Verdict"
            ));

            let by_sub = self.by_subsystem();
            if let Some(entries) = by_sub.get(sub) {
                for b in entries {
                    let v = b.verdict(self.tolerance_pct);
                    out.push_str(&format!(
                        "{:<40} {:>8.2}{} {:>8.2}{} {:>+7.1}% {:>15}\n",
                        truncate(&b.description, 40),
                        b.godot.value,
                        &b.godot.unit,
                        b.patina.value,
                        &b.patina.unit,
                        b.delta_pct(),
                        v.as_str(),
                    ));
                }
            }
            out.push('\n');
        }

        // Summary
        let (faster, comparable, slower) = self.verdict_counts();
        out.push_str("--- Summary ---\n");
        out.push_str(&format!("Total benchmarks: {}\n", self.benchmarks.len()));
        out.push_str(&format!("  Patina faster:  {}\n", faster));
        out.push_str(&format!("  Comparable:     {}\n", comparable));
        out.push_str(&format!("  Godot faster:   {}\n", slower));
        out.push_str(&format!("Overall: {}\n", self.overall_verdict().as_str()));

        out
    }

    /// Renders a JSON report.
    pub fn render_json(&self) -> String {
        let benchmarks_json: Vec<String> = self
            .benchmarks
            .iter()
            .map(|b| {
                let v = b.verdict(self.tolerance_pct);
                format!(
                    concat!(
                        "    {{\n",
                        "      \"subsystem\": \"{}\",\n",
                        "      \"description\": \"{}\",\n",
                        "      \"godot\": {{\"value\": {}, \"unit\": \"{}\"}},\n",
                        "      \"patina\": {{\"value\": {}, \"unit\": \"{}\"}},\n",
                        "      \"delta_pct\": {:.2},\n",
                        "      \"verdict\": \"{}\"\n",
                        "    }}"
                    ),
                    escape_json(&b.subsystem),
                    escape_json(&b.description),
                    b.godot.value,
                    escape_json(&b.godot.unit),
                    b.patina.value,
                    escape_json(&b.patina.unit),
                    b.delta_pct(),
                    v.as_str(),
                )
            })
            .collect();

        let (faster, comparable, slower) = self.verdict_counts();
        format!(
            concat!(
                "{{\n",
                "  \"godot_version\": \"{}\",\n",
                "  \"patina_version\": \"{}\",\n",
                "  \"tolerance_pct\": {},\n",
                "  \"benchmark_count\": {},\n",
                "  \"summary\": {{\n",
                "    \"patina_faster\": {},\n",
                "    \"comparable\": {},\n",
                "    \"godot_faster\": {},\n",
                "    \"overall\": \"{}\"\n",
                "  }},\n",
                "  \"benchmarks\": [\n{}\n  ]\n",
                "}}"
            ),
            escape_json(&self.godot_version),
            escape_json(&self.patina_version),
            self.tolerance_pct,
            self.benchmarks.len(),
            faster,
            comparable,
            slower,
            self.overall_verdict().as_str(),
            benchmarks_json.join(",\n"),
        )
    }
}

// ---------------------------------------------------------------------------
// OverallVerdict
// ---------------------------------------------------------------------------

/// Overall performance comparison verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverallVerdict {
    /// Patina is faster across the board.
    PatinaWins,
    /// Both are comparable in performance.
    Comparable,
    /// Mostly comparable with minor areas where Godot is faster.
    MostlyComparable,
    /// Significant areas where Godot is faster — optimization needed.
    NeedsWork,
    /// No benchmarks available.
    NoData,
}

impl OverallVerdict {
    /// Returns a human-readable label.
    pub fn as_str(&self) -> &'static str {
        match self {
            OverallVerdict::PatinaWins => "PATINA WINS",
            OverallVerdict::Comparable => "COMPARABLE",
            OverallVerdict::MostlyComparable => "MOSTLY COMPARABLE",
            OverallVerdict::NeedsWork => "NEEDS WORK",
            OverallVerdict::NoData => "NO DATA",
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_basics() {
        let m = Measurement::new(16.5, "ms");
        assert!((m.value - 16.5).abs() < f64::EPSILON);
        assert_eq!(m.unit, "ms");
    }

    #[test]
    fn benchmark_lower_is_better_patina_faster() {
        let b = SubsystemBenchmark::new(
            "scene_tree",
            "Node instantiation",
            Measurement::new(20.0, "ms"),
            Measurement::new(15.0, "ms"),
        );
        assert!((b.ratio() - 0.75).abs() < 0.01);
        assert!(b.delta_pct() < 0.0); // Patina is better
        assert_eq!(b.verdict(10.0), Verdict::PatinaFaster);
    }

    #[test]
    fn benchmark_lower_is_better_godot_faster() {
        let b = SubsystemBenchmark::new(
            "render",
            "Draw calls",
            Measurement::new(10.0, "ms"),
            Measurement::new(15.0, "ms"),
        );
        assert!(b.delta_pct() > 0.0); // Patina is worse
        assert_eq!(b.verdict(10.0), Verdict::GodotFaster); // 50% worse exceeds 10% tolerance
    }

    #[test]
    fn benchmark_lower_is_better_comparable() {
        let b = SubsystemBenchmark::new(
            "physics",
            "Step time",
            Measurement::new(10.0, "ms"),
            Measurement::new(10.5, "ms"),
        );
        assert_eq!(b.verdict(10.0), Verdict::Comparable); // 5% worse within 10% tolerance
    }

    #[test]
    fn benchmark_higher_is_better_patina_faster() {
        let b = SubsystemBenchmark::new_higher_better(
            "render",
            "FPS",
            Measurement::new(60.0, "fps"),
            Measurement::new(75.0, "fps"),
        );
        assert!(b.delta_pct() < 0.0); // Negative delta = Patina better
        assert_eq!(b.verdict(10.0), Verdict::PatinaFaster);
    }

    #[test]
    fn benchmark_higher_is_better_godot_faster() {
        let b = SubsystemBenchmark::new_higher_better(
            "render",
            "FPS",
            Measurement::new(60.0, "fps"),
            Measurement::new(40.0, "fps"),
        );
        assert!(b.delta_pct() > 0.0); // Positive delta = Patina worse
        assert_eq!(b.verdict(10.0), Verdict::GodotFaster);
    }

    #[test]
    fn benchmark_zero_godot_value() {
        let b = SubsystemBenchmark::new(
            "misc",
            "Zero baseline",
            Measurement::new(0.0, "ms"),
            Measurement::new(5.0, "ms"),
        );
        assert!((b.ratio() - 1.0).abs() < f64::EPSILON); // Fallback to 1.0
    }

    #[test]
    fn verdict_labels() {
        assert_eq!(Verdict::PatinaFaster.as_str(), "PATINA FASTER");
        assert_eq!(Verdict::Comparable.as_str(), "COMPARABLE");
        assert_eq!(Verdict::GodotFaster.as_str(), "GODOT FASTER");
    }

    #[test]
    fn verdict_symbols() {
        assert_eq!(Verdict::PatinaFaster.symbol(), "+");
        assert_eq!(Verdict::Comparable.symbol(), "=");
        assert_eq!(Verdict::GodotFaster.symbol(), "-");
    }

    #[test]
    fn empty_report() {
        let report = ComparisonReport::new("4.6.1", "0.1.0");
        assert_eq!(report.benchmark_count(), 0);
        assert_eq!(report.overall_verdict(), OverallVerdict::NoData);
        let text = report.render_text();
        assert!(text.contains("No benchmarks recorded"));
    }

    #[test]
    fn report_all_patina_faster() {
        let mut report = ComparisonReport::new("4.6.1", "0.1.0");
        report.add(SubsystemBenchmark::new(
            "scene",
            "Create nodes",
            Measurement::new(20.0, "ms"),
            Measurement::new(10.0, "ms"),
        ));
        report.add(SubsystemBenchmark::new(
            "physics",
            "Step time",
            Measurement::new(5.0, "ms"),
            Measurement::new(3.0, "ms"),
        ));
        let (faster, comparable, slower) = report.verdict_counts();
        assert_eq!(faster, 2);
        assert_eq!(comparable, 0);
        assert_eq!(slower, 0);
        assert_eq!(report.overall_verdict(), OverallVerdict::PatinaWins);
    }

    #[test]
    fn report_all_comparable() {
        let mut report = ComparisonReport::new("4.6.1", "0.1.0");
        report.add(SubsystemBenchmark::new(
            "scene",
            "Create nodes",
            Measurement::new(20.0, "ms"),
            Measurement::new(20.5, "ms"),
        ));
        report.add(SubsystemBenchmark::new(
            "physics",
            "Step",
            Measurement::new(5.0, "ms"),
            Measurement::new(5.2, "ms"),
        ));
        let (_, comparable, _) = report.verdict_counts();
        assert_eq!(comparable, 2);
        assert_eq!(report.overall_verdict(), OverallVerdict::Comparable);
    }

    #[test]
    fn report_mixed_mostly_comparable() {
        let mut report = ComparisonReport::new("4.6.1", "0.1.0");
        // 3 comparable, 1 godot faster → mostly comparable (1/4 = 25%)
        for _ in 0..3 {
            report.add(SubsystemBenchmark::new(
                "scene",
                "OK test",
                Measurement::new(10.0, "ms"),
                Measurement::new(10.5, "ms"),
            ));
        }
        report.add(SubsystemBenchmark::new(
            "render",
            "Slow test",
            Measurement::new(10.0, "ms"),
            Measurement::new(20.0, "ms"),
        ));
        assert_eq!(report.overall_verdict(), OverallVerdict::MostlyComparable);
    }

    #[test]
    fn report_needs_work() {
        let mut report = ComparisonReport::new("4.6.1", "0.1.0");
        // Half are slower → needs work
        report.add(SubsystemBenchmark::new(
            "scene",
            "A",
            Measurement::new(10.0, "ms"),
            Measurement::new(10.5, "ms"),
        ));
        report.add(SubsystemBenchmark::new(
            "render",
            "B",
            Measurement::new(10.0, "ms"),
            Measurement::new(25.0, "ms"),
        ));
        assert_eq!(report.overall_verdict(), OverallVerdict::NeedsWork);
    }

    #[test]
    fn report_tolerance_changes_verdicts() {
        let mut report = ComparisonReport::new("4.6.1", "0.1.0").with_tolerance(50.0);
        // 50% slower, but tolerance is 50% → comparable
        report.add(SubsystemBenchmark::new(
            "render",
            "Draw",
            Measurement::new(10.0, "ms"),
            Measurement::new(15.0, "ms"),
        ));
        let (_, comparable, _) = report.verdict_counts();
        assert_eq!(comparable, 1);
    }

    #[test]
    fn subsystems_sorted_and_deduped() {
        let mut report = ComparisonReport::new("4.6.1", "0.1.0");
        report.add(SubsystemBenchmark::new(
            "render",
            "A",
            Measurement::new(1.0, "ms"),
            Measurement::new(1.0, "ms"),
        ));
        report.add(SubsystemBenchmark::new(
            "audio",
            "B",
            Measurement::new(1.0, "ms"),
            Measurement::new(1.0, "ms"),
        ));
        report.add(SubsystemBenchmark::new(
            "render",
            "C",
            Measurement::new(1.0, "ms"),
            Measurement::new(1.0, "ms"),
        ));
        assert_eq!(report.subsystems(), vec!["audio", "render"]);
    }

    #[test]
    fn text_report_contains_versions() {
        let report = ComparisonReport::new("4.6.1", "0.1.0");
        let text = report.render_text();
        assert!(text.contains("Godot 4.6.1"));
        assert!(text.contains("Patina 0.1.0"));
    }

    #[test]
    fn text_report_with_benchmarks() {
        let mut report = ComparisonReport::new("4.6.1", "0.1.0");
        report.add(SubsystemBenchmark::new(
            "scene",
            "Create 1000 nodes",
            Measurement::new(20.0, "ms"),
            Measurement::new(15.0, "ms"),
        ));
        let text = report.render_text();
        assert!(text.contains("scene"));
        assert!(text.contains("Create 1000 nodes"));
        assert!(text.contains("PATINA FASTER"));
        assert!(text.contains("Summary"));
    }

    #[test]
    fn json_report_structure() {
        let mut report = ComparisonReport::new("4.6.1", "0.1.0");
        report.add(SubsystemBenchmark::new(
            "scene",
            "Test",
            Measurement::new(10.0, "ms"),
            Measurement::new(12.0, "ms"),
        ));
        let json = report.render_json();
        assert!(json.contains("\"godot_version\": \"4.6.1\""));
        assert!(json.contains("\"patina_version\": \"0.1.0\""));
        assert!(json.contains("\"benchmark_count\": 1"));
        assert!(json.contains("\"subsystem\": \"scene\""));
        assert!(json.contains("\"overall\""));
    }

    #[test]
    fn json_report_escapes_strings() {
        let mut report = ComparisonReport::new("4.6.1", "0.1.0");
        report.add(SubsystemBenchmark::new(
            "scene",
            "Test \"with quotes\"",
            Measurement::new(10.0, "ms"),
            Measurement::new(10.0, "ms"),
        ));
        let json = report.render_json();
        assert!(json.contains("Test \\\"with quotes\\\""));
    }

    #[test]
    fn overall_verdict_labels() {
        assert_eq!(OverallVerdict::PatinaWins.as_str(), "PATINA WINS");
        assert_eq!(OverallVerdict::Comparable.as_str(), "COMPARABLE");
        assert_eq!(OverallVerdict::MostlyComparable.as_str(), "MOSTLY COMPARABLE");
        assert_eq!(OverallVerdict::NeedsWork.as_str(), "NEEDS WORK");
        assert_eq!(OverallVerdict::NoData.as_str(), "NO DATA");
    }

    #[test]
    fn by_subsystem_groups_correctly() {
        let mut report = ComparisonReport::new("4.6.1", "0.1.0");
        report.add(SubsystemBenchmark::new(
            "scene",
            "A",
            Measurement::new(1.0, "ms"),
            Measurement::new(1.0, "ms"),
        ));
        report.add(SubsystemBenchmark::new(
            "render",
            "B",
            Measurement::new(1.0, "ms"),
            Measurement::new(1.0, "ms"),
        ));
        report.add(SubsystemBenchmark::new(
            "scene",
            "C",
            Measurement::new(1.0, "ms"),
            Measurement::new(1.0, "ms"),
        ));
        let groups = report.by_subsystem();
        assert_eq!(groups["scene"].len(), 2);
        assert_eq!(groups["render"].len(), 1);
    }

    #[test]
    fn truncate_helper() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("a very long string indeed", 10), "a very ...");
    }

    #[test]
    fn realistic_comparison_report() {
        let mut report = ComparisonReport::new("4.6.1", "0.1.0");

        // Scene tree benchmarks
        report.add(SubsystemBenchmark::new(
            "scene_tree",
            "Instantiate 1000 nodes",
            Measurement::new(12.5, "ms"),
            Measurement::new(14.2, "ms"),
        ));
        report.add(SubsystemBenchmark::new(
            "scene_tree",
            "Free 1000 nodes",
            Measurement::new(8.3, "ms"),
            Measurement::new(7.1, "ms"),
        ));

        // Physics benchmarks
        report.add(SubsystemBenchmark::new(
            "physics",
            "30-frame simulation (10 bodies)",
            Measurement::new(4.2, "ms"),
            Measurement::new(4.5, "ms"),
        ));

        // Render benchmarks
        report.add(SubsystemBenchmark::new_higher_better(
            "render",
            "2D sprites (100) FPS",
            Measurement::new(120.0, "fps"),
            Measurement::new(115.0, "fps"),
        ));

        // Resource benchmarks
        report.add(SubsystemBenchmark::new(
            "resource",
            "Load .tscn (50 nodes)",
            Measurement::new(3.1, "ms"),
            Measurement::new(2.8, "ms"),
        ));
        report.add(SubsystemBenchmark::new(
            "resource",
            "Save .tscn roundtrip",
            Measurement::new(2.0, "ms"),
            Measurement::new(1.9, "ms"),
        ));

        // Memory benchmarks
        report.add(SubsystemBenchmark::new(
            "memory",
            "Peak RSS (empty project)",
            Measurement::new(45.0, "MB"),
            Measurement::new(32.0, "MB"),
        ));

        assert_eq!(report.benchmark_count(), 7);
        assert_eq!(report.subsystems().len(), 5);

        let text = report.render_text();
        assert!(text.contains("Summary"));
        assert!(text.contains("Total benchmarks: 7"));

        let json = report.render_json();
        assert!(json.contains("\"benchmark_count\": 7"));

        // Most should be comparable or better
        let (faster, comparable, _slower) = report.verdict_counts();
        assert!(faster + comparable >= 5);
    }
}
