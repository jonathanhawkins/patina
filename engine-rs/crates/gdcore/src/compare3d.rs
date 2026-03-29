//! 3D physics and render comparison utilities.
//!
//! Provides types and functions for comparing 3D physics traces and render
//! outputs against golden reference data. Mirrors the 2D comparison patterns
//! in `gdrender2d::compare` but extended for 3D Vector3 positions/velocities.

use crate::math::Vector3;

/// A single entry in a 3D physics trace, capturing the state of one body
/// at one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsTraceEntry3D {
    /// The scene node name.
    pub name: String,
    /// Frame number.
    pub frame: u64,
    /// Position at this frame.
    pub position: Vector3,
    /// Linear velocity at this frame.
    pub velocity: Vector3,
    /// Angular velocity magnitude at this frame (radians/second).
    pub angular_velocity: f32,
}

impl PhysicsTraceEntry3D {
    /// Creates a new trace entry.
    pub fn new(
        name: &str,
        frame: u64,
        position: Vector3,
        velocity: Vector3,
        angular_velocity: f32,
    ) -> Self {
        Self {
            name: name.to_string(),
            frame,
            position,
            velocity,
            angular_velocity,
        }
    }
}

/// Result of comparing two 3D physics traces.
#[derive(Debug, Clone)]
pub struct PhysicsTraceCompareResult {
    /// Total number of entries compared.
    pub total_entries: usize,
    /// Number of entries that matched within tolerance.
    pub matching_entries: usize,
    /// Maximum position distance observed.
    pub max_position_diff: f32,
    /// Maximum velocity distance observed.
    pub max_velocity_diff: f32,
    /// Average position distance across all entries.
    pub avg_position_diff: f32,
    /// Per-entry diffs for entries that exceeded tolerance.
    pub mismatches: Vec<TraceMismatch>,
}

/// A single mismatch between expected and actual trace entries.
#[derive(Debug, Clone)]
pub struct TraceMismatch {
    /// The body name.
    pub name: String,
    /// Frame number.
    pub frame: u64,
    /// Position distance.
    pub position_diff: f32,
    /// Velocity distance.
    pub velocity_diff: f32,
    /// Expected position.
    pub expected_position: Vector3,
    /// Actual position.
    pub actual_position: Vector3,
}

impl PhysicsTraceCompareResult {
    /// Returns the fraction of matching entries (0.0 to 1.0).
    pub fn match_ratio(&self) -> f64 {
        if self.total_entries == 0 {
            return 1.0;
        }
        self.matching_entries as f64 / self.total_entries as f64
    }

    /// Returns `true` if all entries matched within tolerance.
    pub fn is_exact_match(&self) -> bool {
        self.matching_entries == self.total_entries
    }

    /// Generates a human-readable parity report.
    pub fn parity_report(&self, label: &str) -> String {
        let mut report = String::new();
        report.push_str(&format!(
            "=== 3D Physics Trace Parity: {} ===\n",
            label
        ));
        report.push_str(&format!(
            "Entries: {}/{} matched ({:.1}%)\n",
            self.matching_entries,
            self.total_entries,
            self.match_ratio() * 100.0
        ));
        report.push_str(&format!(
            "Max position diff: {:.6}\n",
            self.max_position_diff
        ));
        report.push_str(&format!(
            "Max velocity diff: {:.6}\n",
            self.max_velocity_diff
        ));
        report.push_str(&format!(
            "Avg position diff: {:.6}\n",
            self.avg_position_diff
        ));

        if !self.mismatches.is_empty() {
            report.push_str(&format!("\nMismatches ({}):\n", self.mismatches.len()));
            for m in &self.mismatches {
                report.push_str(&format!(
                    "  [{}] frame {}: pos_diff={:.6} vel_diff={:.6}\n    expected=({:.4}, {:.4}, {:.4}) actual=({:.4}, {:.4}, {:.4})\n",
                    m.name,
                    m.frame,
                    m.position_diff,
                    m.velocity_diff,
                    m.expected_position.x,
                    m.expected_position.y,
                    m.expected_position.z,
                    m.actual_position.x,
                    m.actual_position.y,
                    m.actual_position.z,
                ));
            }
        }

        report
    }
}

/// Compares two 3D physics traces entry-by-entry.
///
/// Entries are matched by `(name, frame)` pairs. The tolerance applies to
/// the Euclidean distance between positions and between velocities.
///
/// # Panics
///
/// Panics if the traces have different lengths.
pub fn compare_physics_traces(
    expected: &[PhysicsTraceEntry3D],
    actual: &[PhysicsTraceEntry3D],
    position_tolerance: f32,
    velocity_tolerance: f32,
) -> PhysicsTraceCompareResult {
    assert_eq!(
        expected.len(),
        actual.len(),
        "trace lengths must match: expected {} vs actual {}",
        expected.len(),
        actual.len()
    );

    let total = expected.len();
    let mut matching = 0usize;
    let mut max_pos_diff = 0.0f32;
    let mut max_vel_diff = 0.0f32;
    let mut sum_pos_diff = 0.0f32;
    let mut mismatches = Vec::new();

    for (exp, act) in expected.iter().zip(actual.iter()) {
        let pos_diff = (exp.position - act.position).length();
        let vel_diff = (exp.velocity - act.velocity).length();

        if pos_diff > max_pos_diff {
            max_pos_diff = pos_diff;
        }
        if vel_diff > max_vel_diff {
            max_vel_diff = vel_diff;
        }
        sum_pos_diff += pos_diff;

        if pos_diff <= position_tolerance && vel_diff <= velocity_tolerance {
            matching += 1;
        } else {
            mismatches.push(TraceMismatch {
                name: exp.name.clone(),
                frame: exp.frame,
                position_diff: pos_diff,
                velocity_diff: vel_diff,
                expected_position: exp.position,
                actual_position: act.position,
            });
        }
    }

    PhysicsTraceCompareResult {
        total_entries: total,
        matching_entries: matching,
        max_position_diff: max_pos_diff,
        max_velocity_diff: max_vel_diff,
        avg_position_diff: if total > 0 {
            sum_pos_diff / total as f32
        } else {
            0.0
        },
        mismatches,
    }
}

/// Checks whether a 3D physics trace is deterministic by comparing two runs.
///
/// Both traces must be identical (zero tolerance) for the result to pass.
pub fn assert_deterministic(run_a: &[PhysicsTraceEntry3D], run_b: &[PhysicsTraceEntry3D]) -> bool {
    if run_a.len() != run_b.len() {
        return false;
    }
    for (a, b) in run_a.iter().zip(run_b.iter()) {
        if a.name != b.name || a.frame != b.frame {
            return false;
        }
        if (a.position - b.position).length() > 0.0
            || (a.velocity - b.velocity).length() > 0.0
        {
            return false;
        }
    }
    true
}

/// Result of comparing two 3D render outputs (summary statistics).
///
/// This mirrors `gdrender2d::compare::DiffResult` but is defined here
/// so that 3D render comparison tooling can be developed independently
/// of the not-yet-created `gdrender3d` crate.
#[derive(Debug, Clone)]
pub struct RenderCompareResult3D {
    /// Number of pixels that match within tolerance.
    pub matching_pixels: u64,
    /// Total number of pixels compared.
    pub total_pixels: u64,
    /// Maximum color distance observed.
    pub max_diff: f64,
    /// Average color distance across all pixels.
    pub avg_diff: f64,
    /// Viewport width.
    pub width: u32,
    /// Viewport height.
    pub height: u32,
}

impl RenderCompareResult3D {
    /// Returns the fraction of matching pixels (0.0 to 1.0).
    pub fn match_ratio(&self) -> f64 {
        if self.total_pixels == 0 {
            return 1.0;
        }
        self.matching_pixels as f64 / self.total_pixels as f64
    }

    /// Returns `true` if all pixels match within tolerance.
    pub fn is_exact_match(&self) -> bool {
        self.matching_pixels == self.total_pixels
    }

    /// Generates a human-readable parity report for 3D render comparison.
    pub fn parity_report(&self, label: &str) -> String {
        format!(
            "=== 3D Render Parity: {} ===\n\
             Resolution: {}x{}\n\
             Pixels: {}/{} matched ({:.1}%)\n\
             Max color diff: {:.6}\n\
             Avg color diff: {:.6}\n",
            label,
            self.width,
            self.height,
            self.matching_pixels,
            self.total_pixels,
            self.match_ratio() * 100.0,
            self.max_diff,
            self.avg_diff,
        )
    }
}

// ===========================================================================
// Scene Tree Comparison
// ===========================================================================

/// A flattened entry describing one node in a scene tree, used for
/// structural comparison between Patina and Godot oracle outputs.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneTreeEntry {
    /// Node path from root (e.g., "root/Player/Sprite3D").
    pub path: String,
    /// Class name (e.g., "MeshInstance3D", "Camera3D").
    pub class_name: String,
}

impl SceneTreeEntry {
    /// Creates a new scene tree entry.
    pub fn new(path: &str, class_name: &str) -> Self {
        Self {
            path: path.to_string(),
            class_name: class_name.to_string(),
        }
    }
}

/// A single mismatch between expected and actual scene tree entries.
#[derive(Debug, Clone)]
pub enum SceneTreeMismatch {
    /// Node exists in expected but not in actual.
    Missing {
        /// Path of the missing node.
        path: String,
        /// Expected class name.
        class_name: String,
    },
    /// Node exists in actual but not in expected.
    Extra {
        /// Path of the extra node.
        path: String,
        /// Actual class name.
        class_name: String,
    },
    /// Node exists in both but with different class names.
    ClassMismatch {
        /// Path of the node.
        path: String,
        /// Expected class name.
        expected: String,
        /// Actual class name.
        actual: String,
    },
}

/// Result of comparing two scene tree snapshots.
#[derive(Debug, Clone)]
pub struct SceneTreeCompareResult {
    /// Number of nodes in the expected tree.
    pub expected_count: usize,
    /// Number of nodes in the actual tree.
    pub actual_count: usize,
    /// Number of nodes that match (same path and class).
    pub matching_nodes: usize,
    /// Mismatches found during comparison.
    pub mismatches: Vec<SceneTreeMismatch>,
}

impl SceneTreeCompareResult {
    /// Returns the fraction of matching nodes relative to the expected count.
    pub fn match_ratio(&self) -> f64 {
        if self.expected_count == 0 {
            return 1.0;
        }
        self.matching_nodes as f64 / self.expected_count as f64
    }

    /// Returns `true` if the trees are structurally identical.
    pub fn is_exact_match(&self) -> bool {
        self.mismatches.is_empty()
            && self.expected_count == self.actual_count
            && self.matching_nodes == self.expected_count
    }

    /// Generates a human-readable parity report.
    pub fn parity_report(&self, label: &str) -> String {
        let mut report = String::new();
        report.push_str(&format!("=== Scene Tree Parity: {} ===\n", label));
        report.push_str(&format!(
            "Nodes: {}/{} matched ({:.1}%)\n",
            self.matching_nodes,
            self.expected_count,
            self.match_ratio() * 100.0,
        ));
        report.push_str(&format!(
            "Expected: {} nodes, Actual: {} nodes\n",
            self.expected_count, self.actual_count,
        ));

        if !self.mismatches.is_empty() {
            report.push_str(&format!("\nMismatches ({}):\n", self.mismatches.len()));
            for m in &self.mismatches {
                match m {
                    SceneTreeMismatch::Missing { path, class_name } => {
                        report.push_str(&format!("  MISSING: {} ({})\n", path, class_name));
                    }
                    SceneTreeMismatch::Extra { path, class_name } => {
                        report.push_str(&format!("  EXTRA:   {} ({})\n", path, class_name));
                    }
                    SceneTreeMismatch::ClassMismatch {
                        path,
                        expected,
                        actual,
                    } => {
                        report.push_str(&format!(
                            "  CLASS:   {} expected={} actual={}\n",
                            path, expected, actual
                        ));
                    }
                }
            }
        }

        report
    }
}

/// Compares two scene tree snapshots.
///
/// Entries are matched by path. Two nodes match if they share the same
/// path and class name.
pub fn compare_scene_trees(
    expected: &[SceneTreeEntry],
    actual: &[SceneTreeEntry],
) -> SceneTreeCompareResult {
    use std::collections::HashMap;

    let expected_map: HashMap<&str, &str> = expected
        .iter()
        .map(|e| (e.path.as_str(), e.class_name.as_str()))
        .collect();
    let actual_map: HashMap<&str, &str> = actual
        .iter()
        .map(|e| (e.path.as_str(), e.class_name.as_str()))
        .collect();

    let mut matching = 0usize;
    let mut mismatches = Vec::new();

    // Check all expected nodes.
    for (path, expected_class) in &expected_map {
        match actual_map.get(path) {
            Some(actual_class) if actual_class == expected_class => {
                matching += 1;
            }
            Some(actual_class) => {
                mismatches.push(SceneTreeMismatch::ClassMismatch {
                    path: path.to_string(),
                    expected: expected_class.to_string(),
                    actual: actual_class.to_string(),
                });
            }
            None => {
                mismatches.push(SceneTreeMismatch::Missing {
                    path: path.to_string(),
                    class_name: expected_class.to_string(),
                });
            }
        }
    }

    // Check for extra nodes in actual.
    for (path, class_name) in &actual_map {
        if !expected_map.contains_key(path) {
            mismatches.push(SceneTreeMismatch::Extra {
                path: path.to_string(),
                class_name: class_name.to_string(),
            });
        }
    }

    SceneTreeCompareResult {
        expected_count: expected.len(),
        actual_count: actual.len(),
        matching_nodes: matching,
        mismatches,
    }
}

// ===========================================================================
// Unified Fixture Parity Report
// ===========================================================================

/// Verdict for a single fixture comparison dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionVerdict {
    /// All checks pass within tolerance.
    Pass,
    /// Some checks fail but above a minimum threshold.
    Partial,
    /// Too many failures — below the minimum threshold.
    Fail,
    /// No data available for this dimension.
    Skipped,
}

impl DimensionVerdict {
    /// Returns a human-readable label.
    pub fn as_str(&self) -> &'static str {
        match self {
            DimensionVerdict::Pass => "PASS",
            DimensionVerdict::Partial => "PARTIAL",
            DimensionVerdict::Fail => "FAIL",
            DimensionVerdict::Skipped => "SKIPPED",
        }
    }
}

/// Combined parity report for one 3D fixture, aggregating physics trace,
/// render output, and scene tree comparisons.
#[derive(Debug, Clone)]
pub struct FixtureParityReport3D {
    /// Fixture name or label (e.g., "minimal_3d").
    pub fixture_name: String,
    /// Physics trace comparison result (if available).
    pub physics: Option<PhysicsTraceCompareResult>,
    /// Render comparison result (if available).
    pub render: Option<RenderCompareResult3D>,
    /// Scene tree comparison result (if available).
    pub scene_tree: Option<SceneTreeCompareResult>,
}

impl FixtureParityReport3D {
    /// Creates a new empty report for the given fixture.
    pub fn new(fixture_name: &str) -> Self {
        Self {
            fixture_name: fixture_name.to_string(),
            physics: None,
            render: None,
            scene_tree: None,
        }
    }

    /// Sets the physics trace comparison result.
    pub fn with_physics(mut self, result: PhysicsTraceCompareResult) -> Self {
        self.physics = Some(result);
        self
    }

    /// Sets the render comparison result.
    pub fn with_render(mut self, result: RenderCompareResult3D) -> Self {
        self.render = Some(result);
        self
    }

    /// Sets the scene tree comparison result.
    pub fn with_scene_tree(mut self, result: SceneTreeCompareResult) -> Self {
        self.scene_tree = Some(result);
        self
    }

    /// Returns the verdict for the physics dimension.
    /// Pass if match ratio >= 0.95, Partial if >= 0.70, otherwise Fail.
    pub fn physics_verdict(&self) -> DimensionVerdict {
        match &self.physics {
            None => DimensionVerdict::Skipped,
            Some(p) => {
                let ratio = p.match_ratio();
                if ratio >= 0.95 {
                    DimensionVerdict::Pass
                } else if ratio >= 0.70 {
                    DimensionVerdict::Partial
                } else {
                    DimensionVerdict::Fail
                }
            }
        }
    }

    /// Returns the verdict for the render dimension.
    /// Pass if match ratio >= 0.95, Partial if >= 0.70, otherwise Fail.
    pub fn render_verdict(&self) -> DimensionVerdict {
        match &self.render {
            None => DimensionVerdict::Skipped,
            Some(r) => {
                let ratio = r.match_ratio();
                if ratio >= 0.95 {
                    DimensionVerdict::Pass
                } else if ratio >= 0.70 {
                    DimensionVerdict::Partial
                } else {
                    DimensionVerdict::Fail
                }
            }
        }
    }

    /// Returns the verdict for the scene tree dimension.
    /// Pass if exact match, Partial if ratio >= 0.80, otherwise Fail.
    pub fn scene_tree_verdict(&self) -> DimensionVerdict {
        match &self.scene_tree {
            None => DimensionVerdict::Skipped,
            Some(s) => {
                if s.is_exact_match() {
                    DimensionVerdict::Pass
                } else if s.match_ratio() >= 0.80 {
                    DimensionVerdict::Partial
                } else {
                    DimensionVerdict::Fail
                }
            }
        }
    }

    /// Returns the overall verdict: Pass only if all non-skipped dimensions pass.
    pub fn overall_verdict(&self) -> DimensionVerdict {
        let verdicts = [
            self.physics_verdict(),
            self.render_verdict(),
            self.scene_tree_verdict(),
        ];

        let active: Vec<_> = verdicts
            .iter()
            .filter(|v| **v != DimensionVerdict::Skipped)
            .collect();

        if active.is_empty() {
            return DimensionVerdict::Skipped;
        }
        if active.iter().all(|v| **v == DimensionVerdict::Pass) {
            DimensionVerdict::Pass
        } else if active.iter().any(|v| **v == DimensionVerdict::Fail) {
            DimensionVerdict::Fail
        } else {
            DimensionVerdict::Partial
        }
    }

    /// Renders a human-readable text report.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "=== Fixture Parity Report: {} ===\n\n",
            self.fixture_name
        ));

        if let Some(ref p) = self.physics {
            out.push_str(&p.parity_report(&self.fixture_name));
            out.push_str(&format!("Verdict: {}\n\n", self.physics_verdict().as_str()));
        }

        if let Some(ref r) = self.render {
            out.push_str(&r.parity_report(&self.fixture_name));
            out.push_str(&format!("Verdict: {}\n\n", self.render_verdict().as_str()));
        }

        if let Some(ref s) = self.scene_tree {
            out.push_str(&s.parity_report(&self.fixture_name));
            out.push_str(&format!(
                "Verdict: {}\n\n",
                self.scene_tree_verdict().as_str()
            ));
        }

        out.push_str(&format!(
            "--- Overall: {} ---\n",
            self.overall_verdict().as_str()
        ));

        out
    }

    /// Renders a JSON report.
    pub fn render_json(&self) -> String {
        let physics_json = match &self.physics {
            None => "null".to_string(),
            Some(p) => format!(
                concat!(
                    "{{\n",
                    "      \"match_ratio\": {:.4},\n",
                    "      \"total_entries\": {},\n",
                    "      \"matching_entries\": {},\n",
                    "      \"max_position_diff\": {:.6},\n",
                    "      \"max_velocity_diff\": {:.6},\n",
                    "      \"avg_position_diff\": {:.6},\n",
                    "      \"mismatch_count\": {},\n",
                    "      \"verdict\": \"{}\"\n",
                    "    }}"
                ),
                p.match_ratio(),
                p.total_entries,
                p.matching_entries,
                p.max_position_diff,
                p.max_velocity_diff,
                p.avg_position_diff,
                p.mismatches.len(),
                self.physics_verdict().as_str(),
            ),
        };

        let render_json = match &self.render {
            None => "null".to_string(),
            Some(r) => format!(
                concat!(
                    "{{\n",
                    "      \"match_ratio\": {:.4},\n",
                    "      \"total_pixels\": {},\n",
                    "      \"matching_pixels\": {},\n",
                    "      \"resolution\": \"{}x{}\",\n",
                    "      \"max_diff\": {:.6},\n",
                    "      \"avg_diff\": {:.6},\n",
                    "      \"verdict\": \"{}\"\n",
                    "    }}"
                ),
                r.match_ratio(),
                r.total_pixels,
                r.matching_pixels,
                r.width,
                r.height,
                r.max_diff,
                r.avg_diff,
                self.render_verdict().as_str(),
            ),
        };

        let scene_tree_json = match &self.scene_tree {
            None => "null".to_string(),
            Some(s) => format!(
                concat!(
                    "{{\n",
                    "      \"match_ratio\": {:.4},\n",
                    "      \"expected_count\": {},\n",
                    "      \"actual_count\": {},\n",
                    "      \"matching_nodes\": {},\n",
                    "      \"mismatch_count\": {},\n",
                    "      \"verdict\": \"{}\"\n",
                    "    }}"
                ),
                s.match_ratio(),
                s.expected_count,
                s.actual_count,
                s.matching_nodes,
                s.mismatches.len(),
                self.scene_tree_verdict().as_str(),
            ),
        };

        format!(
            concat!(
                "{{\n",
                "  \"fixture\": \"{}\",\n",
                "  \"physics\": {},\n",
                "  \"render\": {},\n",
                "  \"scene_tree\": {},\n",
                "  \"overall_verdict\": \"{}\"\n",
                "}}"
            ),
            self.fixture_name,
            physics_json,
            render_json,
            scene_tree_json,
            self.overall_verdict().as_str(),
        )
    }
}

/// Aggregated report across multiple fixtures.
#[derive(Debug, Clone)]
pub struct AggregateParityReport3D {
    /// Individual fixture reports.
    pub fixtures: Vec<FixtureParityReport3D>,
}

impl AggregateParityReport3D {
    /// Creates a new empty aggregate report.
    pub fn new() -> Self {
        Self {
            fixtures: Vec::new(),
        }
    }

    /// Adds a fixture report.
    pub fn add(&mut self, report: FixtureParityReport3D) {
        self.fixtures.push(report);
    }

    /// Returns the number of fixtures.
    pub fn fixture_count(&self) -> usize {
        self.fixtures.len()
    }

    /// Counts how many fixtures have each overall verdict.
    pub fn verdict_counts(&self) -> (usize, usize, usize, usize) {
        let mut pass = 0;
        let mut partial = 0;
        let mut fail = 0;
        let mut skipped = 0;
        for f in &self.fixtures {
            match f.overall_verdict() {
                DimensionVerdict::Pass => pass += 1,
                DimensionVerdict::Partial => partial += 1,
                DimensionVerdict::Fail => fail += 1,
                DimensionVerdict::Skipped => skipped += 1,
            }
        }
        (pass, partial, fail, skipped)
    }

    /// Returns `true` if all fixtures pass.
    pub fn all_pass(&self) -> bool {
        self.fixtures
            .iter()
            .all(|f| f.overall_verdict() == DimensionVerdict::Pass)
    }

    /// Renders a human-readable summary.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str("=== 3D Parity Aggregate Report ===\n\n");

        out.push_str(&format!(
            "{:<30} {:>10} {:>10} {:>10} {:>10}\n",
            "Fixture", "Physics", "Render", "Tree", "Overall"
        ));
        out.push_str(&format!("{}\n", "-".repeat(72)));

        for f in &self.fixtures {
            out.push_str(&format!(
                "{:<30} {:>10} {:>10} {:>10} {:>10}\n",
                f.fixture_name,
                f.physics_verdict().as_str(),
                f.render_verdict().as_str(),
                f.scene_tree_verdict().as_str(),
                f.overall_verdict().as_str(),
            ));
        }

        let (pass, partial, fail, skipped) = self.verdict_counts();
        out.push_str(&format!("\n--- Summary ---\n"));
        out.push_str(&format!("Total fixtures: {}\n", self.fixtures.len()));
        out.push_str(&format!("  Pass:    {}\n", pass));
        out.push_str(&format!("  Partial: {}\n", partial));
        out.push_str(&format!("  Fail:    {}\n", fail));
        out.push_str(&format!("  Skipped: {}\n", skipped));

        out
    }

    /// Renders a JSON report.
    pub fn render_json(&self) -> String {
        let fixture_jsons: Vec<String> =
            self.fixtures.iter().map(|f| f.render_json()).collect();

        let (pass, partial, fail, skipped) = self.verdict_counts();
        format!(
            concat!(
                "{{\n",
                "  \"fixture_count\": {},\n",
                "  \"summary\": {{\n",
                "    \"pass\": {},\n",
                "    \"partial\": {},\n",
                "    \"fail\": {},\n",
                "    \"skipped\": {},\n",
                "    \"all_pass\": {}\n",
                "  }},\n",
                "  \"fixtures\": [\n    {}\n  ]\n",
                "}}"
            ),
            self.fixtures.len(),
            pass,
            partial,
            fail,
            skipped,
            self.all_pass(),
            fixture_jsons.join(",\n    "),
        )
    }
}

impl Default for AggregateParityReport3D {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, frame: u64, px: f32, py: f32, pz: f32) -> PhysicsTraceEntry3D {
        PhysicsTraceEntry3D::new(
            name,
            frame,
            Vector3::new(px, py, pz),
            Vector3::ZERO,
            0.0,
        )
    }

    fn entry_with_vel(
        name: &str,
        frame: u64,
        px: f32,
        py: f32,
        pz: f32,
        vx: f32,
        vy: f32,
        vz: f32,
    ) -> PhysicsTraceEntry3D {
        PhysicsTraceEntry3D::new(
            name,
            frame,
            Vector3::new(px, py, pz),
            Vector3::new(vx, vy, vz),
            0.0,
        )
    }

    #[test]
    fn identical_traces_exact_match() {
        let trace = vec![
            entry("Ball", 0, 0.0, 0.0, 0.0),
            entry("Ball", 1, 0.0, 1.0, 0.0),
            entry("Ball", 2, 0.0, 3.0, 0.0),
        ];
        let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);
        assert!(result.is_exact_match());
        assert_eq!(result.total_entries, 3);
        assert_eq!(result.matching_entries, 3);
        assert!(result.mismatches.is_empty());
    }

    #[test]
    fn completely_different_traces() {
        let expected = vec![entry("Ball", 0, 0.0, 0.0, 0.0)];
        let actual = vec![entry("Ball", 0, 10.0, 10.0, 10.0)];
        let result = compare_physics_traces(&expected, &actual, 0.0, 0.0);
        assert_eq!(result.matching_entries, 0);
        assert_eq!(result.mismatches.len(), 1);
        assert!(result.max_position_diff > 17.0); // sqrt(300) ≈ 17.32
    }

    #[test]
    fn tolerance_allows_near_matches() {
        let expected = vec![entry("Ball", 0, 0.0, 0.0, 0.0)];
        let actual = vec![entry("Ball", 0, 0.001, 0.0, 0.0)];

        let strict = compare_physics_traces(&expected, &actual, 0.0, 0.0);
        assert_eq!(strict.matching_entries, 0);

        let lenient = compare_physics_traces(&expected, &actual, 0.01, 0.01);
        assert_eq!(lenient.matching_entries, 1);
    }

    #[test]
    fn velocity_tolerance_independent() {
        let expected = vec![entry_with_vel("Ball", 0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0)];
        let actual = vec![entry_with_vel("Ball", 0, 0.0, 0.0, 0.0, 5.0, 0.0, 0.0)];

        // Position matches but velocity doesn't.
        let result = compare_physics_traces(&expected, &actual, 1.0, 1.0);
        assert_eq!(result.matching_entries, 0);

        let result2 = compare_physics_traces(&expected, &actual, 1.0, 10.0);
        assert_eq!(result2.matching_entries, 1);
    }

    #[test]
    fn match_ratio_calculation() {
        let expected = vec![
            entry("A", 0, 0.0, 0.0, 0.0),
            entry("B", 0, 0.0, 0.0, 0.0),
            entry("A", 1, 1.0, 0.0, 0.0),
            entry("B", 1, 0.0, 0.0, 0.0),
        ];
        let actual = vec![
            entry("A", 0, 0.0, 0.0, 0.0), // match
            entry("B", 0, 0.0, 0.0, 0.0), // match
            entry("A", 1, 1.0, 0.0, 0.0), // match
            entry("B", 1, 99.0, 0.0, 0.0), // mismatch
        ];
        let result = compare_physics_traces(&expected, &actual, 0.001, 0.001);
        assert!((result.match_ratio() - 0.75).abs() < 0.001);
    }

    #[test]
    fn empty_traces_exact_match() {
        let result = compare_physics_traces(&[], &[], 0.0, 0.0);
        assert!(result.is_exact_match());
        assert_eq!(result.match_ratio(), 1.0);
    }

    #[test]
    #[should_panic(expected = "trace lengths must match")]
    fn mismatched_lengths_panics() {
        let a = vec![entry("A", 0, 0.0, 0.0, 0.0)];
        let b = vec![];
        compare_physics_traces(&a, &b, 0.0, 0.0);
    }

    #[test]
    fn parity_report_contains_summary() {
        let expected = vec![entry("Ball", 0, 0.0, 0.0, 0.0)];
        let actual = vec![entry("Ball", 0, 0.1, 0.0, 0.0)];
        let result = compare_physics_traces(&expected, &actual, 0.001, 0.001);
        let report = result.parity_report("gravity_fall_3d");
        assert!(report.contains("gravity_fall_3d"));
        assert!(report.contains("0/1 matched"));
        assert!(report.contains("Mismatches (1)"));
    }

    #[test]
    fn parity_report_clean_when_all_match() {
        let trace = vec![entry("Ball", 0, 1.0, 2.0, 3.0)];
        let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);
        let report = result.parity_report("determinism_check");
        assert!(report.contains("1/1 matched (100.0%)"));
        assert!(!report.contains("Mismatches"));
    }

    #[test]
    fn deterministic_identical_runs() {
        let run = vec![
            entry("Ball", 0, 0.0, 0.0, 0.0),
            entry("Ball", 1, 0.0, 1.0, 0.0),
        ];
        assert!(assert_deterministic(&run, &run));
    }

    #[test]
    fn non_deterministic_different_positions() {
        let a = vec![entry("Ball", 0, 0.0, 0.0, 0.0)];
        let b = vec![entry("Ball", 0, 0.001, 0.0, 0.0)];
        assert!(!assert_deterministic(&a, &b));
    }

    #[test]
    fn render_compare_result_match_ratio() {
        let result = RenderCompareResult3D {
            matching_pixels: 75,
            total_pixels: 100,
            max_diff: 0.5,
            avg_diff: 0.01,
            width: 10,
            height: 10,
        };
        assert!((result.match_ratio() - 0.75).abs() < 0.001);
        assert!(!result.is_exact_match());
    }

    #[test]
    fn render_compare_result_exact_match() {
        let result = RenderCompareResult3D {
            matching_pixels: 100,
            total_pixels: 100,
            max_diff: 0.0,
            avg_diff: 0.0,
            width: 10,
            height: 10,
        };
        assert!(result.is_exact_match());
    }

    #[test]
    fn render_compare_result_zero_pixels() {
        let result = RenderCompareResult3D {
            matching_pixels: 0,
            total_pixels: 0,
            max_diff: 0.0,
            avg_diff: 0.0,
            width: 0,
            height: 0,
        };
        assert_eq!(result.match_ratio(), 1.0);
    }

    #[test]
    fn render_parity_report_contains_fields() {
        let result = RenderCompareResult3D {
            matching_pixels: 90,
            total_pixels: 100,
            max_diff: 0.05,
            avg_diff: 0.002,
            width: 256,
            height: 256,
        };
        let report = result.parity_report("minimal_3d_scene");
        assert!(report.contains("minimal_3d_scene"));
        assert!(report.contains("256x256"));
        assert!(report.contains("90/100"));
        assert!(report.contains("90.0%"));
    }

    #[test]
    fn max_and_avg_diffs_tracked() {
        let expected = vec![
            entry("A", 0, 0.0, 0.0, 0.0),
            entry("A", 1, 0.0, 0.0, 0.0),
        ];
        let actual = vec![
            entry("A", 0, 1.0, 0.0, 0.0), // dist = 1.0
            entry("A", 1, 3.0, 0.0, 0.0), // dist = 3.0
        ];
        let result = compare_physics_traces(&expected, &actual, 10.0, 10.0);
        assert!((result.max_position_diff - 3.0).abs() < 0.001);
        assert!((result.avg_position_diff - 2.0).abs() < 0.001);
    }

    // -----------------------------------------------------------------------
    // Scene Tree Comparison Tests
    // -----------------------------------------------------------------------

    #[test]
    fn identical_scene_trees_exact_match() {
        let tree = vec![
            SceneTreeEntry::new("root", "Node3D"),
            SceneTreeEntry::new("root/Camera3D", "Camera3D"),
            SceneTreeEntry::new("root/Mesh", "MeshInstance3D"),
        ];
        let result = compare_scene_trees(&tree, &tree);
        assert!(result.is_exact_match());
        assert_eq!(result.matching_nodes, 3);
        assert_eq!(result.match_ratio(), 1.0);
    }

    #[test]
    fn empty_scene_trees_exact_match() {
        let result = compare_scene_trees(&[], &[]);
        assert!(result.is_exact_match());
        assert_eq!(result.match_ratio(), 1.0);
    }

    #[test]
    fn missing_node_detected() {
        let expected = vec![
            SceneTreeEntry::new("root", "Node3D"),
            SceneTreeEntry::new("root/Light", "DirectionalLight3D"),
        ];
        let actual = vec![SceneTreeEntry::new("root", "Node3D")];
        let result = compare_scene_trees(&expected, &actual);
        assert!(!result.is_exact_match());
        assert_eq!(result.matching_nodes, 1);
        assert_eq!(result.mismatches.len(), 1);
        assert!(matches!(
            &result.mismatches[0],
            SceneTreeMismatch::Missing { path, .. } if path == "root/Light"
        ));
    }

    #[test]
    fn extra_node_detected() {
        let expected = vec![SceneTreeEntry::new("root", "Node3D")];
        let actual = vec![
            SceneTreeEntry::new("root", "Node3D"),
            SceneTreeEntry::new("root/Extra", "Sprite3D"),
        ];
        let result = compare_scene_trees(&expected, &actual);
        assert!(!result.is_exact_match());
        assert_eq!(result.matching_nodes, 1);
        assert!(result
            .mismatches
            .iter()
            .any(|m| matches!(m, SceneTreeMismatch::Extra { path, .. } if path == "root/Extra")));
    }

    #[test]
    fn class_mismatch_detected() {
        let expected = vec![SceneTreeEntry::new("root/Mesh", "MeshInstance3D")];
        let actual = vec![SceneTreeEntry::new("root/Mesh", "CSGBox3D")];
        let result = compare_scene_trees(&expected, &actual);
        assert!(!result.is_exact_match());
        assert_eq!(result.matching_nodes, 0);
        assert!(matches!(
            &result.mismatches[0],
            SceneTreeMismatch::ClassMismatch { expected, actual, .. }
                if expected == "MeshInstance3D" && actual == "CSGBox3D"
        ));
    }

    #[test]
    fn scene_tree_parity_report_contains_fields() {
        let expected = vec![
            SceneTreeEntry::new("root", "Node3D"),
            SceneTreeEntry::new("root/Camera", "Camera3D"),
        ];
        let actual = vec![SceneTreeEntry::new("root", "Node3D")];
        let result = compare_scene_trees(&expected, &actual);
        let report = result.parity_report("minimal_3d");
        assert!(report.contains("minimal_3d"));
        assert!(report.contains("1/2 matched"));
        assert!(report.contains("MISSING"));
    }

    // -----------------------------------------------------------------------
    // Fixture Parity Report Tests
    // -----------------------------------------------------------------------

    #[test]
    fn fixture_report_all_skipped() {
        let report = FixtureParityReport3D::new("empty");
        assert_eq!(report.overall_verdict(), DimensionVerdict::Skipped);
    }

    #[test]
    fn fixture_report_all_pass() {
        let physics = compare_physics_traces(
            &[entry("A", 0, 0.0, 0.0, 0.0)],
            &[entry("A", 0, 0.0, 0.0, 0.0)],
            0.01,
            0.01,
        );
        let render = RenderCompareResult3D {
            matching_pixels: 100,
            total_pixels: 100,
            max_diff: 0.0,
            avg_diff: 0.0,
            width: 10,
            height: 10,
        };
        let scene_tree = compare_scene_trees(
            &[SceneTreeEntry::new("root", "Node3D")],
            &[SceneTreeEntry::new("root", "Node3D")],
        );

        let report = FixtureParityReport3D::new("test")
            .with_physics(physics)
            .with_render(render)
            .with_scene_tree(scene_tree);

        assert_eq!(report.overall_verdict(), DimensionVerdict::Pass);
    }

    #[test]
    fn fixture_report_partial_physics() {
        let physics = compare_physics_traces(
            &[
                entry("A", 0, 0.0, 0.0, 0.0),
                entry("A", 1, 0.0, 0.0, 0.0),
                entry("A", 2, 0.0, 0.0, 0.0),
                entry("A", 3, 0.0, 0.0, 0.0),
            ],
            &[
                entry("A", 0, 0.0, 0.0, 0.0),
                entry("A", 1, 0.0, 0.0, 0.0),
                entry("A", 2, 0.0, 0.0, 0.0),
                entry("A", 3, 99.0, 0.0, 0.0), // 1 mismatch out of 4 = 75% match
            ],
            0.01,
            0.01,
        );
        let report = FixtureParityReport3D::new("test").with_physics(physics);
        assert_eq!(report.physics_verdict(), DimensionVerdict::Partial);
        assert_eq!(report.overall_verdict(), DimensionVerdict::Partial);
    }

    #[test]
    fn fixture_report_fail_propagates() {
        // 0% match → Fail
        let physics = compare_physics_traces(
            &[entry("A", 0, 0.0, 0.0, 0.0)],
            &[entry("A", 0, 99.0, 99.0, 99.0)],
            0.01,
            0.01,
        );
        let report = FixtureParityReport3D::new("test").with_physics(physics);
        assert_eq!(report.physics_verdict(), DimensionVerdict::Fail);
        assert_eq!(report.overall_verdict(), DimensionVerdict::Fail);
    }

    #[test]
    fn fixture_report_text_contains_sections() {
        let physics = compare_physics_traces(
            &[entry("A", 0, 0.0, 0.0, 0.0)],
            &[entry("A", 0, 0.0, 0.0, 0.0)],
            0.01,
            0.01,
        );
        let report = FixtureParityReport3D::new("minimal_3d").with_physics(physics);
        let text = report.render_text();
        assert!(text.contains("Fixture Parity Report: minimal_3d"));
        assert!(text.contains("Overall: PASS"));
    }

    #[test]
    fn fixture_report_json_structure() {
        let report = FixtureParityReport3D::new("test_fixture")
            .with_render(RenderCompareResult3D {
                matching_pixels: 95,
                total_pixels: 100,
                max_diff: 0.01,
                avg_diff: 0.001,
                width: 64,
                height: 64,
            });
        let json = report.render_json();
        assert!(json.contains("\"fixture\": \"test_fixture\""));
        assert!(json.contains("\"physics\": null"));
        assert!(json.contains("\"resolution\": \"64x64\""));
        assert!(json.contains("\"overall_verdict\": \"PASS\""));
    }

    #[test]
    fn dimension_verdict_labels() {
        assert_eq!(DimensionVerdict::Pass.as_str(), "PASS");
        assert_eq!(DimensionVerdict::Partial.as_str(), "PARTIAL");
        assert_eq!(DimensionVerdict::Fail.as_str(), "FAIL");
        assert_eq!(DimensionVerdict::Skipped.as_str(), "SKIPPED");
    }

    // -----------------------------------------------------------------------
    // Aggregate Report Tests
    // -----------------------------------------------------------------------

    #[test]
    fn aggregate_empty() {
        let agg = AggregateParityReport3D::new();
        assert_eq!(agg.fixture_count(), 0);
        assert!(agg.all_pass());
    }

    #[test]
    fn aggregate_all_pass() {
        let mut agg = AggregateParityReport3D::new();
        let physics = compare_physics_traces(
            &[entry("A", 0, 0.0, 0.0, 0.0)],
            &[entry("A", 0, 0.0, 0.0, 0.0)],
            0.01,
            0.01,
        );
        agg.add(FixtureParityReport3D::new("scene_a").with_physics(physics.clone()));
        agg.add(FixtureParityReport3D::new("scene_b").with_physics(physics));
        assert!(agg.all_pass());
        let (pass, partial, fail, skipped) = agg.verdict_counts();
        assert_eq!(pass, 2);
        assert_eq!(partial, 0);
        assert_eq!(fail, 0);
        assert_eq!(skipped, 0);
    }

    #[test]
    fn aggregate_mixed_verdicts() {
        let mut agg = AggregateParityReport3D::new();

        // Pass fixture
        let good_physics = compare_physics_traces(
            &[entry("A", 0, 0.0, 0.0, 0.0)],
            &[entry("A", 0, 0.0, 0.0, 0.0)],
            0.01,
            0.01,
        );
        agg.add(FixtureParityReport3D::new("good").with_physics(good_physics));

        // Fail fixture
        let bad_physics = compare_physics_traces(
            &[entry("A", 0, 0.0, 0.0, 0.0)],
            &[entry("A", 0, 99.0, 99.0, 99.0)],
            0.01,
            0.01,
        );
        agg.add(FixtureParityReport3D::new("bad").with_physics(bad_physics));

        assert!(!agg.all_pass());
        let (pass, _, fail, _) = agg.verdict_counts();
        assert_eq!(pass, 1);
        assert_eq!(fail, 1);
    }

    #[test]
    fn aggregate_text_report_contains_table() {
        let mut agg = AggregateParityReport3D::new();
        agg.add(FixtureParityReport3D::new("scene_a"));
        let text = agg.render_text();
        assert!(text.contains("Aggregate Report"));
        assert!(text.contains("scene_a"));
        assert!(text.contains("Total fixtures: 1"));
    }

    #[test]
    fn aggregate_json_report_structure() {
        let mut agg = AggregateParityReport3D::new();
        agg.add(FixtureParityReport3D::new("scene_a"));
        let json = agg.render_json();
        assert!(json.contains("\"fixture_count\": 1"));
        assert!(json.contains("\"all_pass\""));
        assert!(json.contains("\"fixtures\""));
    }
}
