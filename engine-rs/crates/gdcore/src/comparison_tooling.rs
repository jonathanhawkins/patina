//! Unified render and physics comparison tooling.
//!
//! Provides batch comparison infrastructure that aggregates render (2D/3D),
//! physics trace, and scene tree comparison results across multiple fixtures
//! into a structured, JSON-serializable report.
//!
//! # Usage
//!
//! ```rust,ignore
//! use gdcore::comparison_tooling::{BatchComparisonReport, FixtureResult, SubsystemScore};
//!
//! let mut report = BatchComparisonReport::new("phase6-3d-parity");
//!
//! report.add_fixture(FixtureResult::new("minimal_3d")
//!     .with_render(SubsystemScore::new(95, 100))
//!     .with_physics(SubsystemScore::new(48, 50))
//!     .with_scene_tree(SubsystemScore::new(12, 12)));
//!
//! assert!(report.overall_parity() > 0.95);
//! let json = report.to_json();
//! ```

/// Score for a single subsystem comparison (render, physics, or scene tree).
#[derive(Debug, Clone, PartialEq)]
pub struct SubsystemScore {
    /// Number of checks that matched within tolerance.
    pub matching: u64,
    /// Total number of checks performed.
    pub total: u64,
    /// Maximum observed deviation (unitless, subsystem-specific).
    pub max_diff: f64,
    /// Average observed deviation.
    pub avg_diff: f64,
    /// Human-readable details about mismatches, if any.
    pub notes: Vec<String>,
}

impl SubsystemScore {
    /// Creates a new score with match counts only.
    pub fn new(matching: u64, total: u64) -> Self {
        Self {
            matching,
            total,
            max_diff: 0.0,
            avg_diff: 0.0,
            notes: Vec::new(),
        }
    }

    /// Creates a score with full diff statistics.
    pub fn with_diffs(matching: u64, total: u64, max_diff: f64, avg_diff: f64) -> Self {
        Self {
            matching,
            total,
            max_diff,
            avg_diff,
            notes: Vec::new(),
        }
    }

    /// Adds a note describing a mismatch or observation.
    pub fn add_note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    /// Returns the match ratio (0.0 to 1.0). Returns 1.0 for zero-total.
    pub fn match_ratio(&self) -> f64 {
        if self.total == 0 {
            return 1.0;
        }
        self.matching as f64 / self.total as f64
    }

    /// Returns `true` if all checks passed.
    pub fn is_perfect(&self) -> bool {
        self.matching == self.total
    }

    /// Serializes to a JSON string.
    pub fn to_json(&self) -> String {
        let notes_json: Vec<String> = self
            .notes
            .iter()
            .map(|n| format!("\"{}\"", n.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect();

        format!(
            concat!(
                "{{",
                "\"matching\":{},",
                "\"total\":{},",
                "\"match_ratio\":{:.6},",
                "\"max_diff\":{:.6},",
                "\"avg_diff\":{:.6},",
                "\"notes\":[{}]",
                "}}"
            ),
            self.matching,
            self.total,
            self.match_ratio(),
            self.max_diff,
            self.avg_diff,
            notes_json.join(","),
        )
    }
}

/// Comparison result for a single fixture, covering multiple subsystems.
#[derive(Debug, Clone)]
pub struct FixtureResult {
    /// Fixture name (e.g. "minimal_3d", "hierarchy_3d").
    pub name: String,
    /// Render comparison score, if applicable.
    pub render: Option<SubsystemScore>,
    /// Physics trace comparison score, if applicable.
    pub physics: Option<SubsystemScore>,
    /// Scene tree comparison score, if applicable.
    pub scene_tree: Option<SubsystemScore>,
}

impl FixtureResult {
    /// Creates a new fixture result with no subsystem scores.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            render: None,
            physics: None,
            scene_tree: None,
        }
    }

    /// Sets the render comparison score.
    pub fn with_render(mut self, score: SubsystemScore) -> Self {
        self.render = Some(score);
        self
    }

    /// Sets the physics comparison score.
    pub fn with_physics(mut self, score: SubsystemScore) -> Self {
        self.physics = Some(score);
        self
    }

    /// Sets the scene tree comparison score.
    pub fn with_scene_tree(mut self, score: SubsystemScore) -> Self {
        self.scene_tree = Some(score);
        self
    }

    /// Aggregate parity across all present subsystems (0.0 to 1.0).
    pub fn overall_parity(&self) -> f64 {
        let mut total = 0u64;
        let mut matching = 0u64;

        for score in [&self.render, &self.physics, &self.scene_tree].into_iter().flatten() {
            total += score.total;
            matching += score.matching;
        }

        if total == 0 {
            1.0
        } else {
            matching as f64 / total as f64
        }
    }

    /// Returns `true` if all present subsystems are at perfect parity.
    pub fn is_perfect(&self) -> bool {
        [&self.render, &self.physics, &self.scene_tree]
            .into_iter()
            .flatten()
            .all(|s| s.is_perfect())
    }

    /// Serializes to a JSON string.
    pub fn to_json(&self) -> String {
        let render_json = match &self.render {
            Some(s) => s.to_json(),
            None => "null".to_string(),
        };
        let physics_json = match &self.physics {
            Some(s) => s.to_json(),
            None => "null".to_string(),
        };
        let scene_tree_json = match &self.scene_tree {
            Some(s) => s.to_json(),
            None => "null".to_string(),
        };

        format!(
            concat!(
                "{{",
                "\"name\":\"{}\",",
                "\"overall_parity\":{:.6},",
                "\"is_perfect\":{},",
                "\"render\":{},",
                "\"physics\":{},",
                "\"scene_tree\":{}",
                "}}"
            ),
            self.name,
            self.overall_parity(),
            self.is_perfect(),
            render_json,
            physics_json,
            scene_tree_json,
        )
    }

    /// Generates a human-readable summary line.
    pub fn summary_line(&self) -> String {
        let mut parts = Vec::new();
        if let Some(r) = &self.render {
            parts.push(format!("render={:.1}%", r.match_ratio() * 100.0));
        }
        if let Some(p) = &self.physics {
            parts.push(format!("physics={:.1}%", p.match_ratio() * 100.0));
        }
        if let Some(s) = &self.scene_tree {
            parts.push(format!("scene={:.1}%", s.match_ratio() * 100.0));
        }
        format!(
            "{}: {:.1}% overall [{}]",
            self.name,
            self.overall_parity() * 100.0,
            parts.join(", "),
        )
    }
}

/// Aggregated comparison report across multiple fixtures and subsystems.
#[derive(Debug, Clone)]
pub struct BatchComparisonReport {
    /// Report identifier (e.g. "phase6-3d-parity").
    pub report_id: String,
    /// Per-fixture results.
    pub fixtures: Vec<FixtureResult>,
    /// Minimum parity threshold for the report to pass (0.0 to 1.0).
    pub pass_threshold: f64,
}

impl BatchComparisonReport {
    /// Creates a new empty report with the default 95% pass threshold.
    pub fn new(report_id: impl Into<String>) -> Self {
        Self {
            report_id: report_id.into(),
            fixtures: Vec::new(),
            pass_threshold: 0.95,
        }
    }

    /// Sets the minimum pass threshold.
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.pass_threshold = threshold;
        self
    }

    /// Adds a fixture result to the report.
    pub fn add_fixture(&mut self, fixture: FixtureResult) {
        self.fixtures.push(fixture);
    }

    /// Returns the overall parity across all fixtures and subsystems.
    pub fn overall_parity(&self) -> f64 {
        let mut total = 0u64;
        let mut matching = 0u64;

        for f in &self.fixtures {
            for score in [&f.render, &f.physics, &f.scene_tree].into_iter().flatten() {
                total += score.total;
                matching += score.matching;
            }
        }

        if total == 0 {
            1.0
        } else {
            matching as f64 / total as f64
        }
    }

    /// Returns `true` if overall parity meets the pass threshold.
    pub fn passes(&self) -> bool {
        self.overall_parity() >= self.pass_threshold
    }

    /// Returns per-subsystem aggregate parity across all fixtures.
    pub fn subsystem_summary(&self) -> SubsystemSummary {
        let mut render_m = 0u64;
        let mut render_t = 0u64;
        let mut physics_m = 0u64;
        let mut physics_t = 0u64;
        let mut scene_m = 0u64;
        let mut scene_t = 0u64;

        for f in &self.fixtures {
            if let Some(r) = &f.render {
                render_m += r.matching;
                render_t += r.total;
            }
            if let Some(p) = &f.physics {
                physics_m += p.matching;
                physics_t += p.total;
            }
            if let Some(s) = &f.scene_tree {
                scene_m += s.matching;
                scene_t += s.total;
            }
        }

        SubsystemSummary {
            render_parity: if render_t == 0 {
                1.0
            } else {
                render_m as f64 / render_t as f64
            },
            render_total: render_t,
            physics_parity: if physics_t == 0 {
                1.0
            } else {
                physics_m as f64 / physics_t as f64
            },
            physics_total: physics_t,
            scene_tree_parity: if scene_t == 0 {
                1.0
            } else {
                scene_m as f64 / scene_t as f64
            },
            scene_tree_total: scene_t,
        }
    }

    /// Generates a human-readable report.
    pub fn to_text_report(&self) -> String {
        let mut report = String::new();
        report.push_str(&format!(
            "=== Batch Comparison Report: {} ===\n",
            self.report_id
        ));
        report.push_str(&format!(
            "Overall parity: {:.1}% (threshold: {:.1}%) — {}\n",
            self.overall_parity() * 100.0,
            self.pass_threshold * 100.0,
            if self.passes() { "PASS" } else { "FAIL" },
        ));

        let summary = self.subsystem_summary();
        report.push_str(&format!(
            "Subsystems: render={:.1}% ({} checks), physics={:.1}% ({} checks), scene={:.1}% ({} checks)\n",
            summary.render_parity * 100.0,
            summary.render_total,
            summary.physics_parity * 100.0,
            summary.physics_total,
            summary.scene_tree_parity * 100.0,
            summary.scene_tree_total,
        ));
        report.push_str(&format!("Fixtures: {}\n\n", self.fixtures.len()));

        for f in &self.fixtures {
            report.push_str(&format!("  {}\n", f.summary_line()));
        }

        report
    }

    /// Serializes the full report to JSON.
    pub fn to_json(&self) -> String {
        let fixtures_json: Vec<String> = self.fixtures.iter().map(|f| f.to_json()).collect();
        let summary = self.subsystem_summary();

        format!(
            concat!(
                "{{",
                "\"report_id\":\"{}\",",
                "\"overall_parity\":{:.6},",
                "\"pass_threshold\":{:.6},",
                "\"passes\":{},",
                "\"fixture_count\":{},",
                "\"subsystem_summary\":{{",
                "\"render_parity\":{:.6},",
                "\"render_total\":{},",
                "\"physics_parity\":{:.6},",
                "\"physics_total\":{},",
                "\"scene_tree_parity\":{:.6},",
                "\"scene_tree_total\":{}",
                "}},",
                "\"fixtures\":[{}]",
                "}}"
            ),
            self.report_id,
            self.overall_parity(),
            self.pass_threshold,
            self.passes(),
            self.fixtures.len(),
            summary.render_parity,
            summary.render_total,
            summary.physics_parity,
            summary.physics_total,
            summary.scene_tree_parity,
            summary.scene_tree_total,
            fixtures_json.join(","),
        )
    }
}

/// Aggregate per-subsystem parity metrics.
#[derive(Debug, Clone)]
pub struct SubsystemSummary {
    /// Render parity across all fixtures (0.0 to 1.0).
    pub render_parity: f64,
    /// Total render checks.
    pub render_total: u64,
    /// Physics parity across all fixtures (0.0 to 1.0).
    pub physics_parity: f64,
    /// Total physics checks.
    pub physics_total: u64,
    /// Scene tree parity across all fixtures (0.0 to 1.0).
    pub scene_tree_parity: f64,
    /// Total scene tree checks.
    pub scene_tree_total: u64,
}

// ===========================================================================
// Conversion helpers — bridge from existing comparison types
// ===========================================================================

use crate::compare3d::{PhysicsTraceCompareResult, PhysicsTraceEntry3D, RenderCompareResult3D};
use crate::math::Vector3;

impl From<&PhysicsTraceCompareResult> for SubsystemScore {
    fn from(result: &PhysicsTraceCompareResult) -> Self {
        let mut score = Self::with_diffs(
            result.matching_entries as u64,
            result.total_entries as u64,
            result.max_position_diff as f64,
            result.avg_position_diff as f64,
        );
        for m in &result.mismatches {
            score.add_note(format!(
                "[{}] frame {}: pos_diff={:.6}",
                m.name, m.frame, m.position_diff
            ));
        }
        score
    }
}

impl From<&RenderCompareResult3D> for SubsystemScore {
    fn from(result: &RenderCompareResult3D) -> Self {
        Self::with_diffs(
            result.matching_pixels,
            result.total_pixels,
            result.max_diff,
            result.avg_diff,
        )
    }
}

// ===========================================================================
// Golden I/O — load golden traces and scene data from JSON fixtures
// ===========================================================================

/// Loads a physics golden trace from a JSON array of entries.
///
/// Expected format: `[{"name":"Ball","frame":0,"px":0.0,"py":5.0,"pz":0.0,"vx":0.0,"vy":0.0,"vz":0.0}, ...]`
///
/// # Errors
///
/// Returns `Err` if the JSON is malformed or entries are missing required fields.
pub fn load_physics_trace_json(json_str: &str) -> Result<Vec<PhysicsTraceEntry3D>, String> {
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(json_str).map_err(|e| format!("invalid JSON: {e}"))?;

    entries
        .iter()
        .map(|entry| {
            let name = entry["name"]
                .as_str()
                .ok_or("missing 'name' field")?
                .to_string();
            let frame = entry["frame"].as_u64().ok_or("missing 'frame' field")?;
            let px = entry["px"].as_f64().ok_or("missing 'px'")? as f32;
            let py = entry["py"].as_f64().ok_or("missing 'py'")? as f32;
            let pz = entry["pz"].as_f64().ok_or("missing 'pz'")? as f32;
            let vx = entry["vx"].as_f64().ok_or("missing 'vx'")? as f32;
            let vy = entry["vy"].as_f64().ok_or("missing 'vy'")? as f32;
            let vz = entry["vz"].as_f64().ok_or("missing 'vz'")? as f32;

            Ok(PhysicsTraceEntry3D::new(
                &name,
                frame,
                Vector3::new(px, py, pz),
                Vector3::new(vx, vy, vz),
                0.0,
            ))
        })
        .collect()
}

/// Loads a physics golden trace from a file path.
///
/// # Errors
///
/// Returns `Err` if the file cannot be read or the JSON is malformed.
pub fn load_physics_trace_file(path: &std::path::Path) -> Result<Vec<PhysicsTraceEntry3D>, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    load_physics_trace_json(&contents)
}

/// Serializes a physics trace to JSON for golden file output.
pub fn save_physics_trace_json(trace: &[PhysicsTraceEntry3D]) -> String {
    let entries: Vec<String> = trace
        .iter()
        .map(|e| {
            format!(
                "  {{\"name\": \"{}\", \"frame\": {}, \"px\": {:.3}, \"py\": {:.3}, \"pz\": {:.3}, \"vx\": {:.1}, \"vy\": {:.1}, \"vz\": {:.1}}}",
                e.name, e.frame,
                e.position.x, e.position.y, e.position.z,
                e.velocity.x, e.velocity.y, e.velocity.z,
            )
        })
        .collect();
    format!("[\n{}\n]\n", entries.join(",\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsystem_score_perfect() {
        let score = SubsystemScore::new(100, 100);
        assert!(score.is_perfect());
        assert!((score.match_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn subsystem_score_partial() {
        let score = SubsystemScore::new(75, 100);
        assert!(!score.is_perfect());
        assert!((score.match_ratio() - 0.75).abs() < 0.001);
    }

    #[test]
    fn subsystem_score_empty() {
        let score = SubsystemScore::new(0, 0);
        assert!(score.is_perfect());
        assert!((score.match_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn subsystem_score_with_diffs() {
        let score = SubsystemScore::with_diffs(90, 100, 0.05, 0.002);
        assert!((score.max_diff - 0.05).abs() < f64::EPSILON);
        assert!((score.avg_diff - 0.002).abs() < f64::EPSILON);
    }

    #[test]
    fn subsystem_score_notes() {
        let mut score = SubsystemScore::new(9, 10);
        score.add_note("pixel (3,4) off by 0.02");
        assert_eq!(score.notes.len(), 1);
        let json = score.to_json();
        assert!(json.contains("pixel (3,4) off by 0.02"));
    }

    #[test]
    fn fixture_result_render_only() {
        let f = FixtureResult::new("minimal_3d")
            .with_render(SubsystemScore::new(95, 100));
        assert!((f.overall_parity() - 0.95).abs() < 0.001);
        assert!(!f.is_perfect());
    }

    #[test]
    fn fixture_result_all_subsystems() {
        let f = FixtureResult::new("test")
            .with_render(SubsystemScore::new(90, 100))
            .with_physics(SubsystemScore::new(45, 50))
            .with_scene_tree(SubsystemScore::new(50, 50));
        // (90+45+50) / (100+50+50) = 185/200 = 0.925
        assert!((f.overall_parity() - 0.925).abs() < 0.001);
    }

    #[test]
    fn fixture_result_perfect() {
        let f = FixtureResult::new("test")
            .with_render(SubsystemScore::new(100, 100))
            .with_physics(SubsystemScore::new(50, 50));
        assert!(f.is_perfect());
    }

    #[test]
    fn fixture_result_no_subsystems() {
        let f = FixtureResult::new("empty");
        assert!((f.overall_parity() - 1.0).abs() < f64::EPSILON);
        assert!(f.is_perfect());
    }

    #[test]
    fn fixture_summary_line() {
        let f = FixtureResult::new("hierarchy_3d")
            .with_render(SubsystemScore::new(90, 100))
            .with_physics(SubsystemScore::new(48, 50));
        let line = f.summary_line();
        assert!(line.contains("hierarchy_3d"));
        assert!(line.contains("render=90.0%"));
        assert!(line.contains("physics=96.0%"));
    }

    #[test]
    fn batch_report_overall_parity() {
        let mut report = BatchComparisonReport::new("test-report");
        report.add_fixture(
            FixtureResult::new("a")
                .with_render(SubsystemScore::new(90, 100))
                .with_physics(SubsystemScore::new(50, 50)),
        );
        report.add_fixture(
            FixtureResult::new("b")
                .with_render(SubsystemScore::new(80, 100))
                .with_scene_tree(SubsystemScore::new(25, 25)),
        );
        // (90+50+80+25) / (100+50+100+25) = 245/275 ≈ 0.89090...
        assert!((report.overall_parity() - 245.0 / 275.0).abs() < 0.001);
    }

    #[test]
    fn batch_report_passes_with_threshold() {
        let mut report = BatchComparisonReport::new("high-parity")
            .with_threshold(0.90);
        report.add_fixture(
            FixtureResult::new("a")
                .with_render(SubsystemScore::new(95, 100)),
        );
        assert!(report.passes());

        let mut report_low = BatchComparisonReport::new("low-parity")
            .with_threshold(0.90);
        report_low.add_fixture(
            FixtureResult::new("a")
                .with_render(SubsystemScore::new(80, 100)),
        );
        assert!(!report_low.passes());
    }

    #[test]
    fn batch_report_empty_passes() {
        let report = BatchComparisonReport::new("empty");
        assert!(report.passes());
        assert!((report.overall_parity() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn subsystem_summary_aggregates() {
        let mut report = BatchComparisonReport::new("test");
        report.add_fixture(
            FixtureResult::new("a")
                .with_render(SubsystemScore::new(90, 100))
                .with_physics(SubsystemScore::new(50, 50)),
        );
        report.add_fixture(
            FixtureResult::new("b")
                .with_render(SubsystemScore::new(80, 100))
                .with_physics(SubsystemScore::new(40, 50)),
        );
        let s = report.subsystem_summary();
        // render: (90+80)/(100+100) = 0.85
        assert!((s.render_parity - 0.85).abs() < 0.001);
        assert_eq!(s.render_total, 200);
        // physics: (50+40)/(50+50) = 0.90
        assert!((s.physics_parity - 0.90).abs() < 0.001);
        assert_eq!(s.physics_total, 100);
        // scene: no fixtures have it
        assert!((s.scene_tree_parity - 1.0).abs() < f64::EPSILON);
        assert_eq!(s.scene_tree_total, 0);
    }

    #[test]
    fn to_json_roundtrip_structure() {
        let mut report = BatchComparisonReport::new("json-test");
        report.add_fixture(
            FixtureResult::new("fixture_a")
                .with_render(SubsystemScore::new(95, 100))
                .with_physics(SubsystemScore::with_diffs(48, 50, 0.01, 0.002)),
        );
        let json = report.to_json();
        assert!(json.contains("\"report_id\":\"json-test\""));
        assert!(json.contains("\"fixture_count\":1"));
        assert!(json.contains("\"fixture_a\""));
        assert!(json.contains("\"passes\":true"));
    }

    #[test]
    fn to_text_report_contains_sections() {
        let mut report = BatchComparisonReport::new("text-test")
            .with_threshold(0.90);
        report.add_fixture(
            FixtureResult::new("minimal_3d")
                .with_render(SubsystemScore::new(95, 100))
                .with_physics(SubsystemScore::new(50, 50)),
        );
        let text = report.to_text_report();
        assert!(text.contains("=== Batch Comparison Report: text-test ==="));
        assert!(text.contains("PASS"));
        assert!(text.contains("minimal_3d"));
        assert!(text.contains("render=95.0%"));
    }

    #[test]
    fn to_text_report_shows_fail() {
        let mut report = BatchComparisonReport::new("fail-test")
            .with_threshold(0.99);
        report.add_fixture(
            FixtureResult::new("bad")
                .with_render(SubsystemScore::new(50, 100)),
        );
        let text = report.to_text_report();
        assert!(text.contains("FAIL"));
    }

    #[test]
    fn from_physics_trace_compare_result() {
        use crate::compare3d::{PhysicsTraceCompareResult, TraceMismatch};
        use crate::math::Vector3;

        let result = PhysicsTraceCompareResult {
            total_entries: 10,
            matching_entries: 8,
            max_position_diff: 0.5,
            max_velocity_diff: 0.1,
            avg_position_diff: 0.05,
            mismatches: vec![TraceMismatch {
                name: "Ball".to_string(),
                frame: 5,
                position_diff: 0.5,
                velocity_diff: 0.1,
                expected_position: Vector3::ZERO,
                actual_position: Vector3::new(0.5, 0.0, 0.0),
            }],
        };

        let score = SubsystemScore::from(&result);
        assert_eq!(score.matching, 8);
        assert_eq!(score.total, 10);
        assert!((score.max_diff - 0.5).abs() < f64::EPSILON);
        assert_eq!(score.notes.len(), 1);
        assert!(score.notes[0].contains("Ball"));
    }

    #[test]
    fn from_render_compare_result_3d() {
        let result = RenderCompareResult3D {
            matching_pixels: 90,
            total_pixels: 100,
            max_diff: 0.05,
            avg_diff: 0.002,
            width: 10,
            height: 10,
        };

        let score = SubsystemScore::from(&result);
        assert_eq!(score.matching, 90);
        assert_eq!(score.total, 100);
        assert!((score.max_diff - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn fixture_to_json_with_nulls() {
        let f = FixtureResult::new("render_only")
            .with_render(SubsystemScore::new(100, 100));
        let json = f.to_json();
        assert!(json.contains("\"physics\":null"));
        assert!(json.contains("\"scene_tree\":null"));
        assert!(json.contains("\"render\":{"));
    }

    #[test]
    fn subsystem_score_json_escapes_quotes() {
        let mut score = SubsystemScore::new(1, 1);
        score.add_note("value was \"unexpected\"");
        let json = score.to_json();
        assert!(json.contains("\\\"unexpected\\\""));
    }

    // -----------------------------------------------------------------------
    // Golden I/O tests
    // -----------------------------------------------------------------------

    #[test]
    fn load_physics_trace_from_json() {
        let json = r#"[
            {"name": "Ball", "frame": 0, "px": 0.0, "py": 5.0, "pz": 0.0, "vx": 0.0, "vy": 0.0, "vz": 0.0},
            {"name": "Ball", "frame": 1, "px": 0.0, "py": 4.837, "pz": 0.0, "vx": 0.0, "vy": -9.8, "vz": 0.0}
        ]"#;
        let trace = super::load_physics_trace_json(json).unwrap();
        assert_eq!(trace.len(), 2);
        assert_eq!(trace[0].name, "Ball");
        assert_eq!(trace[0].frame, 0);
        assert!((trace[0].position.y - 5.0).abs() < 0.001);
        assert!((trace[1].velocity.y - -9.8).abs() < 0.01);
    }

    #[test]
    fn load_physics_trace_missing_field_errors() {
        let json = r#"[{"name": "Ball", "frame": 0}]"#;
        let result = super::load_physics_trace_json(json);
        assert!(result.is_err());
    }

    #[test]
    fn load_physics_trace_invalid_json_errors() {
        let result = super::load_physics_trace_json("not json");
        assert!(result.is_err());
    }

    #[test]
    fn save_physics_trace_roundtrip() {
        let trace = vec![
            PhysicsTraceEntry3D::new(
                "Ball",
                0,
                Vector3::new(0.0, 5.0, 0.0),
                Vector3::ZERO,
                0.0,
            ),
            PhysicsTraceEntry3D::new(
                "Ball",
                1,
                Vector3::new(0.0, 4.837, 0.0),
                Vector3::new(0.0, -9.8, 0.0),
                0.0,
            ),
        ];
        let json = super::save_physics_trace_json(&trace);
        let loaded = super::load_physics_trace_json(&json).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!((loaded[0].position.y - 5.0).abs() < 0.01);
        assert!((loaded[1].velocity.y - -9.8).abs() < 0.1);
    }

    #[test]
    fn load_physics_trace_empty_array() {
        let trace = super::load_physics_trace_json("[]").unwrap();
        assert!(trace.is_empty());
    }

    #[test]
    fn load_physics_trace_file_not_found() {
        let result = super::load_physics_trace_file(std::path::Path::new("/nonexistent.json"));
        assert!(result.is_err());
    }
}
