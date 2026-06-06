//! CI lane configuration for repin regeneration and parity refresh.
//!
//! Defines the structure of the repin validation pipeline as code so that
//! the CI workflow, Makefile targets, and local developer commands all stay
//! consistent with a single source of truth.
//!
//! The repin pipeline has:
//! - **Gates**: independent validation jobs that must all pass
//! - **Test tiers**: layered test subsets (fast → golden → full)
//! - **Refresh steps**: oracle regeneration and golden fixture updates
//! - **Summary**: parity report generation from gate results

use std::fmt;

// ===========================================================================
// Test Tiers
// ===========================================================================

/// A test tier in the repin validation pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TestTier {
    /// Tier 1: Fast unit/integration tests (<10s). Skips golden, stress,
    /// render_golden, staleness, and bench_ tests.
    Fast,
    /// Tier 2: Golden comparison tests (~30s). Skips stress, render_golden,
    /// and bench_ tests but includes golden comparisons.
    Golden,
    /// Tier 3: Full test suite. No skips.
    Full,
}

impl TestTier {
    /// Returns the skip patterns for this tier.
    pub fn skip_patterns(&self) -> &[&str] {
        match self {
            TestTier::Fast => &["golden", "stress", "render_golden", "staleness", "bench_"],
            TestTier::Golden => &["stress", "render_golden", "bench_"],
            TestTier::Full => &[],
        }
    }

    /// Returns the cargo test command suffix for this tier.
    pub fn cargo_args(&self) -> String {
        let skips = self.skip_patterns();
        if skips.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = skips.iter().map(|s| format!("--skip {s}")).collect();
        format!("-- {}", parts.join(" "))
    }

    /// Returns the human-readable label.
    pub fn label(&self) -> &str {
        match self {
            TestTier::Fast => "Tier 1 (fast)",
            TestTier::Golden => "Tier 2 (golden)",
            TestTier::Full => "Tier 3 (full)",
        }
    }

    /// Returns all tiers in order.
    pub fn all() -> &'static [TestTier] {
        &[TestTier::Fast, TestTier::Golden, TestTier::Full]
    }
}

impl fmt::Display for TestTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ===========================================================================
// Repin Gates
// ===========================================================================

/// A validation gate in the repin pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepinGate {
    /// Oracle parity tests (Tier 1 + Tier 2 + oracle-specific).
    OracleParity,
    /// Render golden pixel/image tests.
    RenderGoldens,
    /// Physics trace determinism golden tests.
    PhysicsTraceGoldens,
    /// Runtime compatibility slices (headless, 2D, 3D, platform).
    RuntimeCompatSlices,
    /// Submodule pin verification and fixture existence.
    PinVerification,
}

impl RepinGate {
    /// Returns all gates in pipeline order.
    pub fn all() -> &'static [RepinGate] {
        &[
            RepinGate::OracleParity,
            RepinGate::RenderGoldens,
            RepinGate::PhysicsTraceGoldens,
            RepinGate::RuntimeCompatSlices,
            RepinGate::PinVerification,
        ]
    }

    /// Returns the human-readable label for this gate.
    pub fn label(&self) -> &str {
        match self {
            RepinGate::OracleParity => "Oracle parity tests",
            RepinGate::RenderGoldens => "Render golden tests",
            RepinGate::PhysicsTraceGoldens => "Physics trace goldens",
            RepinGate::RuntimeCompatSlices => "Runtime compat slices",
            RepinGate::PinVerification => "Pin verification",
        }
    }

    /// Returns whether this gate requires a Git submodule checkout.
    pub fn requires_submodule(&self) -> bool {
        matches!(
            self,
            RepinGate::OracleParity
                | RepinGate::PhysicsTraceGoldens
                | RepinGate::RuntimeCompatSlices
                | RepinGate::PinVerification
        )
    }

    /// Returns whether this gate is blocking (failure = pipeline failure).
    pub fn is_blocking(&self) -> bool {
        // All gates are blocking in the repin pipeline.
        true
    }
}

impl fmt::Display for RepinGate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ===========================================================================
// Runtime Compat Slices
// ===========================================================================

/// A runtime compatibility slice in the CI pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompatSlice {
    /// Headless: scene tree, ClassDB, signals, resources, object model.
    Headless,
    /// 2D: physics, rendering, cameras, collision, geometry.
    TwoD,
    /// 3D: 3D transforms, physics, nodes.
    ThreeD,
    /// Platform: input, windows, audio.
    Platform,
    /// Fuzz: property tests, robustness.
    Fuzz,
}

impl CompatSlice {
    /// Returns all slices.
    pub fn all() -> &'static [CompatSlice] {
        &[
            CompatSlice::Headless,
            CompatSlice::TwoD,
            CompatSlice::ThreeD,
            CompatSlice::Platform,
            CompatSlice::Fuzz,
        ]
    }

    /// Returns the label for this slice.
    pub fn label(&self) -> &str {
        match self {
            CompatSlice::Headless => "headless",
            CompatSlice::TwoD => "2d",
            CompatSlice::ThreeD => "3d",
            CompatSlice::Platform => "platform",
            CompatSlice::Fuzz => "fuzz",
        }
    }

    /// Returns the test name filter patterns for this slice.
    pub fn test_filters(&self) -> &[&str] {
        match self {
            CompatSlice::Headless => &[
                "change_scene_",
                "classdb_",
                "connect_deferred",
                "default_property",
                "gdscript_",
                "lifecycle_",
                "nodepath_",
                "notification_",
                "object_",
                "oracle_parity",
                "oracle_regression",
                "packed_scene_",
                "resource_",
                "scene_aware",
                "scene_instancing",
                "scene_lifecycle",
                "signal_",
                "trace_parity",
                "unique_name",
                "unified_loader",
                "instanced_resource",
                "instancing_ownership",
                "cache_regression",
            ],
            CompatSlice::TwoD => &[
                "area2d_",
                "camera_viewport",
                "character_static",
                "collision_",
                "geometry2d_",
                "node2d_",
                "physics_integration",
                "physics_stepping",
                "render_2d",
                "texture_draw",
                "vertical_slice",
            ],
            CompatSlice::ThreeD => &["node3d_", "physics3d_", "transform3d_"],
            CompatSlice::Platform => &[
                "input_action",
                "input_map",
                "input_snapshot",
                "keyboard_action",
                "mouse_input",
                "window_lifecycle",
                "window_minmax",
                "audio_deterministic",
                "audio_smoke",
            ],
            CompatSlice::Fuzz => &["fuzz_", "property_tests", "robustness"],
        }
    }
}

impl fmt::Display for CompatSlice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ===========================================================================
// Refresh Steps
// ===========================================================================

/// A step in the oracle refresh process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshStep {
    /// Update the upstream/godot submodule to the target version.
    UpdateSubmodule,
    /// Run oracle capture scripts to regenerate fixture JSONs.
    RegenerateOracle,
    /// Run render golden capture to regenerate golden images.
    RegenerateRenderGoldens,
    /// Run physics trace capture to regenerate physics goldens.
    RegeneratePhysicsGoldens,
    /// Run the full test suite to validate the refresh.
    ValidateParity,
    /// Generate the parity summary report.
    GenerateSummary,
}

impl RefreshStep {
    /// Returns all steps in order.
    pub fn all() -> &'static [RefreshStep] {
        &[
            RefreshStep::UpdateSubmodule,
            RefreshStep::RegenerateOracle,
            RefreshStep::RegenerateRenderGoldens,
            RefreshStep::RegeneratePhysicsGoldens,
            RefreshStep::ValidateParity,
            RefreshStep::GenerateSummary,
        ]
    }

    /// Returns the label.
    pub fn label(&self) -> &str {
        match self {
            RefreshStep::UpdateSubmodule => "Update upstream submodule",
            RefreshStep::RegenerateOracle => "Regenerate oracle outputs",
            RefreshStep::RegenerateRenderGoldens => "Regenerate render goldens",
            RefreshStep::RegeneratePhysicsGoldens => "Regenerate physics goldens",
            RefreshStep::ValidateParity => "Validate parity",
            RefreshStep::GenerateSummary => "Generate parity summary",
        }
    }

    /// Returns the command or script for this step.
    pub fn command(&self) -> &str {
        match self {
            RefreshStep::UpdateSubmodule => "git submodule update --remote upstream/godot",
            RefreshStep::RegenerateOracle => "tools/oracle/run_all.sh",
            RefreshStep::RegenerateRenderGoldens => "make -C engine-rs test-render",
            RefreshStep::RegeneratePhysicsGoldens => {
                "cargo test --workspace -- physics_trace deterministic_physics"
            }
            RefreshStep::ValidateParity => "make -C engine-rs test",
            RefreshStep::GenerateSummary => "# summary generated by CI parity-summary job",
        }
    }

    /// Returns true if this step requires a Godot binary.
    pub fn requires_godot_binary(&self) -> bool {
        matches!(
            self,
            RefreshStep::RegenerateOracle | RefreshStep::RegenerateRenderGoldens
        )
    }
}

impl fmt::Display for RefreshStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ===========================================================================
// Gate Result & Pipeline Summary
// ===========================================================================

/// Result of a single gate check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateResult {
    /// Gate passed.
    Pass,
    /// Gate failed.
    Fail,
    /// Gate was skipped.
    Skip,
}

impl GateResult {
    /// Returns the badge label.
    pub fn badge(&self) -> &str {
        match self {
            GateResult::Pass => "PASS",
            GateResult::Fail => "FAIL",
            GateResult::Skip => "SKIP",
        }
    }
}

impl fmt::Display for GateResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.badge())
    }
}

/// Collected gate results for a repin validation run.
#[derive(Debug, Clone)]
pub struct RepinSummary {
    /// Godot version being validated.
    pub godot_version: String,
    /// Results for each gate.
    pub gate_results: Vec<(RepinGate, GateResult)>,
    /// Test counts: (passed, failed) per tier.
    pub tier_counts: Vec<(TestTier, usize, usize)>,
}

impl RepinSummary {
    /// Creates a new empty summary for a given Godot version.
    pub fn new(godot_version: &str) -> Self {
        Self {
            godot_version: godot_version.to_string(),
            gate_results: Vec::new(),
            tier_counts: Vec::new(),
        }
    }

    /// Records a gate result.
    pub fn add_gate(&mut self, gate: RepinGate, result: GateResult) {
        self.gate_results.push((gate, result));
    }

    /// Records test counts for a tier.
    pub fn add_tier_count(&mut self, tier: TestTier, passed: usize, failed: usize) {
        self.tier_counts.push((tier, passed, failed));
    }

    /// Returns true if all gates passed (no failures).
    pub fn all_gates_pass(&self) -> bool {
        self.gate_results
            .iter()
            .all(|(_, r)| *r != GateResult::Fail)
    }

    /// Returns the number of failed gates.
    pub fn failed_gate_count(&self) -> usize {
        self.gate_results
            .iter()
            .filter(|(_, r)| *r == GateResult::Fail)
            .count()
    }

    /// Returns total passed tests across all recorded tiers.
    pub fn total_passed(&self) -> usize {
        self.tier_counts.iter().map(|(_, p, _)| p).sum()
    }

    /// Returns total failed tests across all recorded tiers.
    pub fn total_failed(&self) -> usize {
        self.tier_counts.iter().map(|(_, _, f)| f).sum()
    }

    /// Returns the overall parity percentage.
    pub fn parity_pct(&self) -> f64 {
        let total = self.total_passed() + self.total_failed();
        if total == 0 {
            return 100.0;
        }
        self.total_passed() as f64 / total as f64 * 100.0
    }

    /// Renders a human-readable Markdown summary report.
    pub fn render_report(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "# Repin Validation: Godot {}\n\n",
            self.godot_version
        ));

        // Gate results table
        out.push_str("## Gate Results\n\n");
        out.push_str("| Gate | Status |\n");
        out.push_str("|------|--------|\n");
        for (gate, result) in &self.gate_results {
            out.push_str(&format!("| {} | {} |\n", gate.label(), result.badge()));
        }
        out.push('\n');

        // Test counts table
        if !self.tier_counts.is_empty() {
            out.push_str("## Test Counts\n\n");
            out.push_str("| Suite | Passed | Failed |\n");
            out.push_str("|-------|--------|--------|\n");
            for (tier, passed, failed) in &self.tier_counts {
                out.push_str(&format!("| {} | {} | {} |\n", tier.label(), passed, failed));
            }
            out.push_str(&format!(
                "| **Total** | **{}** | **{}** |\n",
                self.total_passed(),
                self.total_failed()
            ));
            out.push_str(&format!("\n**Parity rate: {:.1}%**\n", self.parity_pct()));
        }

        // Overall status
        out.push_str(&format!(
            "\n## Status: {}\n",
            if self.all_gates_pass() {
                "ALL GATES PASS"
            } else {
                "GATES FAILED"
            }
        ));

        out
    }
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- TestTier -----------------------------------------------------------

    #[test]
    fn tier_fast_skips_five_patterns() {
        assert_eq!(TestTier::Fast.skip_patterns().len(), 5);
        assert!(TestTier::Fast.skip_patterns().contains(&"golden"));
        assert!(TestTier::Fast.skip_patterns().contains(&"bench_"));
    }

    #[test]
    fn tier_golden_skips_three_patterns() {
        assert_eq!(TestTier::Golden.skip_patterns().len(), 3);
        assert!(!TestTier::Golden.skip_patterns().contains(&"golden"));
    }

    #[test]
    fn tier_full_skips_nothing() {
        assert!(TestTier::Full.skip_patterns().is_empty());
    }

    #[test]
    fn tier_cargo_args_format() {
        let args = TestTier::Fast.cargo_args();
        assert!(args.contains("--skip golden"));
        assert!(args.contains("--skip bench_"));

        let full_args = TestTier::Full.cargo_args();
        assert!(full_args.is_empty());
    }

    #[test]
    fn tier_ordering() {
        assert!(TestTier::Fast < TestTier::Golden);
        assert!(TestTier::Golden < TestTier::Full);
    }

    #[test]
    fn tier_all_returns_three() {
        assert_eq!(TestTier::all().len(), 3);
    }

    // -- RepinGate ----------------------------------------------------------

    #[test]
    fn all_gates_returns_five() {
        assert_eq!(RepinGate::all().len(), 5);
    }

    #[test]
    fn all_gates_are_blocking() {
        for gate in RepinGate::all() {
            assert!(gate.is_blocking(), "{} should be blocking", gate.label());
        }
    }

    #[test]
    fn submodule_requirement_correct() {
        assert!(RepinGate::OracleParity.requires_submodule());
        assert!(!RepinGate::RenderGoldens.requires_submodule());
        assert!(RepinGate::PhysicsTraceGoldens.requires_submodule());
        assert!(RepinGate::RuntimeCompatSlices.requires_submodule());
        assert!(RepinGate::PinVerification.requires_submodule());
    }

    #[test]
    fn gate_labels_non_empty() {
        for gate in RepinGate::all() {
            assert!(!gate.label().is_empty());
        }
    }

    // -- CompatSlice --------------------------------------------------------

    #[test]
    fn all_slices_returns_five() {
        assert_eq!(CompatSlice::all().len(), 5);
    }

    #[test]
    fn headless_slice_has_filters() {
        let filters = CompatSlice::Headless.test_filters();
        assert!(filters.len() > 10);
        assert!(filters.contains(&"classdb_"));
        assert!(filters.contains(&"signal_"));
        assert!(filters.contains(&"unique_name"));
    }

    #[test]
    fn two_d_slice_has_filters() {
        let filters = CompatSlice::TwoD.test_filters();
        assert!(filters.contains(&"physics_stepping"));
        assert!(filters.contains(&"render_2d"));
    }

    #[test]
    fn three_d_slice_has_filters() {
        let filters = CompatSlice::ThreeD.test_filters();
        assert!(filters.contains(&"node3d_"));
        assert!(filters.contains(&"physics3d_"));
    }

    #[test]
    fn no_slice_filter_overlap() {
        // Filters should not appear in multiple slices (no double-counting).
        let all_filters: Vec<(&str, &str)> = CompatSlice::all()
            .iter()
            .flat_map(|s| s.test_filters().iter().map(move |f| (s.label(), *f)))
            .collect();
        for i in 0..all_filters.len() {
            for j in (i + 1)..all_filters.len() {
                if all_filters[i].0 != all_filters[j].0 {
                    assert_ne!(
                        all_filters[i].1, all_filters[j].1,
                        "filter '{}' appears in both '{}' and '{}' slices",
                        all_filters[i].1, all_filters[i].0, all_filters[j].0
                    );
                }
            }
        }
    }

    // -- RefreshStep --------------------------------------------------------

    #[test]
    fn refresh_steps_returns_six() {
        assert_eq!(RefreshStep::all().len(), 6);
    }

    #[test]
    fn refresh_step_ordering_starts_with_submodule() {
        assert_eq!(RefreshStep::all()[0], RefreshStep::UpdateSubmodule);
    }

    #[test]
    fn refresh_step_ordering_ends_with_summary() {
        let steps = RefreshStep::all();
        assert_eq!(steps[steps.len() - 1], RefreshStep::GenerateSummary);
    }

    #[test]
    fn oracle_regen_requires_godot() {
        assert!(RefreshStep::RegenerateOracle.requires_godot_binary());
        assert!(!RefreshStep::ValidateParity.requires_godot_binary());
    }

    #[test]
    fn each_step_has_command() {
        for step in RefreshStep::all() {
            assert!(!step.command().is_empty(), "{} has no command", step.label());
        }
    }

    // -- RepinSummary -------------------------------------------------------

    #[test]
    fn empty_summary_is_green() {
        let s = RepinSummary::new("4.6.1-stable");
        assert!(s.all_gates_pass());
        assert_eq!(s.failed_gate_count(), 0);
        assert!((s.parity_pct() - 100.0).abs() < 0.001);
    }

    #[test]
    fn summary_tracks_failures() {
        let mut s = RepinSummary::new("4.6.1-stable");
        s.add_gate(RepinGate::OracleParity, GateResult::Pass);
        s.add_gate(RepinGate::RenderGoldens, GateResult::Fail);
        s.add_gate(RepinGate::PinVerification, GateResult::Pass);

        assert!(!s.all_gates_pass());
        assert_eq!(s.failed_gate_count(), 1);
    }

    #[test]
    fn summary_tracks_test_counts() {
        let mut s = RepinSummary::new("4.6.1-stable");
        s.add_tier_count(TestTier::Fast, 180, 2);
        s.add_tier_count(TestTier::Golden, 50, 0);

        assert_eq!(s.total_passed(), 230);
        assert_eq!(s.total_failed(), 2);
        assert!((s.parity_pct() - 99.138).abs() < 0.01);
    }

    #[test]
    fn summary_report_contains_version() {
        let mut s = RepinSummary::new("4.6.1-stable");
        s.add_gate(RepinGate::OracleParity, GateResult::Pass);
        let report = s.render_report();
        assert!(report.contains("4.6.1-stable"));
        assert!(report.contains("Oracle parity tests"));
        assert!(report.contains("PASS"));
        assert!(report.contains("ALL GATES PASS"));
    }

    #[test]
    fn summary_report_shows_failure() {
        let mut s = RepinSummary::new("4.7.0-stable");
        s.add_gate(RepinGate::PhysicsTraceGoldens, GateResult::Fail);
        let report = s.render_report();
        assert!(report.contains("FAIL"));
        assert!(report.contains("GATES FAILED"));
    }

    #[test]
    fn gate_result_badges() {
        assert_eq!(GateResult::Pass.badge(), "PASS");
        assert_eq!(GateResult::Fail.badge(), "FAIL");
        assert_eq!(GateResult::Skip.badge(), "SKIP");
    }
}
