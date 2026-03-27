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
}
