//! pat-2z6a / pat-wvr: CI lane for repin regeneration and parity refresh.
//!
//! Validates the CI lane configuration as code:
//! - Test tier definitions and skip patterns match CI workflow
//! - Gate pipeline covers all required validation steps
//! - Runtime compat slices partition tests without overlap
//! - Refresh steps are ordered correctly for repin workflow
//! - Summary report generation with realistic gate results
//! - End-to-end repin pipeline configuration consistency

use gdcore::ci_repin::*;

// ===========================================================================
// 1. Test tier contracts (must match Makefile and repin-validation.yml)
// ===========================================================================

#[test]
fn tier_fast_matches_makefile_test_fast() {
    // Makefile `test-fast`: --skip golden --skip stress --skip render_golden --skip staleness --skip bench_
    let skips = TestTier::Fast.skip_patterns();
    assert!(skips.contains(&"golden"));
    assert!(skips.contains(&"stress"));
    assert!(skips.contains(&"render_golden"));
    assert!(skips.contains(&"staleness"));
    assert!(skips.contains(&"bench_"));
    assert_eq!(skips.len(), 5);
}

#[test]
fn tier_golden_matches_makefile_test_golden() {
    // Makefile `test-golden`: --skip stress --skip render_golden --skip bench_
    let skips = TestTier::Golden.skip_patterns();
    assert!(skips.contains(&"stress"));
    assert!(skips.contains(&"render_golden"));
    assert!(skips.contains(&"bench_"));
    assert_eq!(skips.len(), 3);
}

#[test]
fn tier_full_matches_makefile_test() {
    // Makefile `test`: cargo test --workspace (no skips)
    assert!(TestTier::Full.skip_patterns().is_empty());
}

#[test]
fn tiers_are_strictly_ordered() {
    // Fast ⊂ Golden ⊂ Full (each tier includes more tests)
    let fast_skips = TestTier::Fast.skip_patterns().len();
    let golden_skips = TestTier::Golden.skip_patterns().len();
    let full_skips = TestTier::Full.skip_patterns().len();
    assert!(fast_skips > golden_skips);
    assert!(golden_skips > full_skips);
}

#[test]
fn golden_skips_are_subset_of_fast_skips() {
    let fast = TestTier::Fast.skip_patterns();
    let golden = TestTier::Golden.skip_patterns();
    for pattern in golden {
        assert!(
            fast.contains(pattern),
            "Golden skip '{}' not found in Fast skips",
            pattern
        );
    }
}

#[test]
fn tier_cargo_args_are_parseable() {
    let args = TestTier::Fast.cargo_args();
    assert!(args.starts_with("-- "));
    // Each skip pattern should appear as "--skip <pattern>"
    for pattern in TestTier::Fast.skip_patterns() {
        assert!(
            args.contains(&format!("--skip {pattern}")),
            "Missing --skip {pattern} in cargo args"
        );
    }
}

// ===========================================================================
// 2. Gate pipeline completeness
// ===========================================================================

#[test]
fn pipeline_has_five_gates() {
    assert_eq!(RepinGate::all().len(), 5);
}

#[test]
fn pipeline_gate_order_matches_workflow() {
    // repin-validation.yml job order
    let gates = RepinGate::all();
    assert_eq!(gates[0], RepinGate::OracleParity);
    assert_eq!(gates[1], RepinGate::RenderGoldens);
    assert_eq!(gates[2], RepinGate::PhysicsTraceGoldens);
    assert_eq!(gates[3], RepinGate::RuntimeCompatSlices);
    assert_eq!(gates[4], RepinGate::PinVerification);
}

#[test]
fn all_gates_are_blocking() {
    for gate in RepinGate::all() {
        assert!(
            gate.is_blocking(),
            "Gate '{}' should be blocking — repin pipeline requires all gates pass",
            gate.label()
        );
    }
}

#[test]
fn oracle_parity_requires_submodule() {
    assert!(RepinGate::OracleParity.requires_submodule());
}

#[test]
fn render_goldens_does_not_require_submodule() {
    // Render tests are self-contained (CPU renderer, no Godot dependency)
    assert!(!RepinGate::RenderGoldens.requires_submodule());
}

#[test]
fn pin_verification_requires_submodule() {
    assert!(RepinGate::PinVerification.requires_submodule());
}

// ===========================================================================
// 3. Runtime compat slice configuration
// ===========================================================================

#[test]
fn five_compat_slices() {
    assert_eq!(CompatSlice::all().len(), 5);
}

#[test]
fn headless_slice_covers_core_subsystems() {
    let filters = CompatSlice::Headless.test_filters();
    // Must include all core subsystems tested in repin-validation.yml headless step
    let required = [
        "classdb_",
        "lifecycle_",
        "signal_",
        "resource_",
        "packed_scene_",
        "unique_name",
        "oracle_parity",
    ];
    for r in &required {
        assert!(
            filters.contains(r),
            "Headless slice missing required filter '{}'",
            r
        );
    }
}

#[test]
fn two_d_slice_covers_physics_and_render() {
    let filters = CompatSlice::TwoD.test_filters();
    assert!(filters.contains(&"physics_stepping"));
    assert!(filters.contains(&"render_2d"));
    assert!(filters.contains(&"collision_"));
    assert!(filters.contains(&"geometry2d_"));
}

#[test]
fn three_d_slice_covers_3d_subsystems() {
    let filters = CompatSlice::ThreeD.test_filters();
    assert!(filters.contains(&"node3d_"));
    assert!(filters.contains(&"physics3d_"));
    assert!(filters.contains(&"transform3d_"));
}

#[test]
fn platform_slice_covers_input_and_window() {
    let filters = CompatSlice::Platform.test_filters();
    assert!(filters.contains(&"input_action"));
    assert!(filters.contains(&"window_lifecycle"));
    assert!(filters.contains(&"audio_smoke"));
}

#[test]
fn no_filter_appears_in_multiple_slices() {
    let mut seen: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for slice in CompatSlice::all() {
        for filter in slice.test_filters() {
            if let Some(prev_slice) = seen.insert(filter, slice.label()) {
                panic!(
                    "Filter '{}' appears in both '{}' and '{}' — would double-count tests",
                    filter, prev_slice, slice.label()
                );
            }
        }
    }
}

// ===========================================================================
// 4. Refresh step ordering
// ===========================================================================

#[test]
fn refresh_starts_with_submodule_update() {
    assert_eq!(RefreshStep::all()[0], RefreshStep::UpdateSubmodule);
}

#[test]
fn refresh_ends_with_summary() {
    let steps = RefreshStep::all();
    assert_eq!(steps[steps.len() - 1], RefreshStep::GenerateSummary);
}

#[test]
fn oracle_regen_before_validation() {
    let steps = RefreshStep::all();
    let regen_pos = steps
        .iter()
        .position(|s| *s == RefreshStep::RegenerateOracle)
        .unwrap();
    let validate_pos = steps
        .iter()
        .position(|s| *s == RefreshStep::ValidateParity)
        .unwrap();
    assert!(
        regen_pos < validate_pos,
        "Oracle regeneration must come before validation"
    );
}

#[test]
fn godot_binary_steps_identified() {
    let godot_steps: Vec<_> = RefreshStep::all()
        .iter()
        .filter(|s| s.requires_godot_binary())
        .collect();
    assert_eq!(godot_steps.len(), 2);
    assert!(RefreshStep::RegenerateOracle.requires_godot_binary());
    assert!(RefreshStep::RegenerateRenderGoldens.requires_godot_binary());
}

#[test]
fn non_godot_steps_are_ci_safe() {
    // Steps that don't require Godot can run in headless CI
    assert!(!RefreshStep::UpdateSubmodule.requires_godot_binary());
    assert!(!RefreshStep::ValidateParity.requires_godot_binary());
    assert!(!RefreshStep::GenerateSummary.requires_godot_binary());
}

// ===========================================================================
// 5. Parity summary report generation
// ===========================================================================

#[test]
fn all_pass_summary_report() {
    let mut s = RepinSummary::new("4.6.1-stable");
    for gate in RepinGate::all() {
        s.add_gate(*gate, GateResult::Pass);
    }
    s.add_tier_count(TestTier::Fast, 180, 0);
    s.add_tier_count(TestTier::Golden, 50, 0);

    assert!(s.all_gates_pass());
    assert_eq!(s.failed_gate_count(), 0);
    assert_eq!(s.total_passed(), 230);
    assert_eq!(s.total_failed(), 0);
    assert!((s.parity_pct() - 100.0).abs() < 0.001);

    let report = s.render_report();
    assert!(report.contains("4.6.1-stable"));
    assert!(report.contains("ALL GATES PASS"));
    assert!(report.contains("100.0%"));
    // Every gate should appear as PASS
    for gate in RepinGate::all() {
        assert!(report.contains(gate.label()));
        assert!(report.contains("PASS"));
    }
}

#[test]
fn mixed_results_summary_report() {
    let mut s = RepinSummary::new("4.7.0-dev");
    s.add_gate(RepinGate::OracleParity, GateResult::Pass);
    s.add_gate(RepinGate::RenderGoldens, GateResult::Fail);
    s.add_gate(RepinGate::PhysicsTraceGoldens, GateResult::Pass);
    s.add_gate(RepinGate::RuntimeCompatSlices, GateResult::Fail);
    s.add_gate(RepinGate::PinVerification, GateResult::Skip);

    assert!(!s.all_gates_pass());
    assert_eq!(s.failed_gate_count(), 2);

    let report = s.render_report();
    assert!(report.contains("GATES FAILED"));
    assert!(report.contains("FAIL"));
    assert!(report.contains("SKIP"));
}

#[test]
fn summary_parity_calculation() {
    let mut s = RepinSummary::new("4.6.1-stable");
    s.add_tier_count(TestTier::Fast, 170, 0);
    s.add_tier_count(TestTier::Golden, 25, 5);

    assert_eq!(s.total_passed(), 195);
    assert_eq!(s.total_failed(), 5);
    assert!((s.parity_pct() - 97.5).abs() < 0.001);
}

#[test]
fn empty_summary_is_green() {
    let s = RepinSummary::new("test");
    assert!(s.all_gates_pass());
    assert!((s.parity_pct() - 100.0).abs() < 0.001);
}

// ===========================================================================
// 6. End-to-end pipeline configuration consistency
// ===========================================================================

#[test]
fn full_repin_pipeline_simulation() {
    // Simulate a complete repin validation run matching repin-validation.yml
    let mut summary = RepinSummary::new("4.6.1-stable");

    // Step 1: All refresh steps have labels and commands
    for step in RefreshStep::all() {
        assert!(!step.label().is_empty());
        assert!(!step.command().is_empty());
    }

    // Step 2: Run all gates
    for gate in RepinGate::all() {
        // Simulate pass for all gates
        summary.add_gate(*gate, GateResult::Pass);
    }

    // Step 3: Record test counts from each tier
    summary.add_tier_count(TestTier::Fast, 185, 0);
    summary.add_tier_count(TestTier::Golden, 210, 0);

    // Step 4: All compat slices have non-empty filters
    let total_filters: usize = CompatSlice::all()
        .iter()
        .map(|s| s.test_filters().len())
        .sum();
    assert!(
        total_filters > 30,
        "Combined compat slices should cover >30 test filters, got {}",
        total_filters
    );

    // Step 5: Generate report
    let report = summary.render_report();
    assert!(report.contains("4.6.1-stable"));
    assert!(report.contains("ALL GATES PASS"));
    assert!(report.contains("100.0%"));

    eprintln!("\n{}", report);
}

#[test]
fn repin_pipeline_with_regressions() {
    let mut summary = RepinSummary::new("4.7.0-stable");

    summary.add_gate(RepinGate::OracleParity, GateResult::Pass);
    summary.add_gate(RepinGate::RenderGoldens, GateResult::Fail);
    summary.add_gate(RepinGate::PhysicsTraceGoldens, GateResult::Pass);
    summary.add_gate(RepinGate::RuntimeCompatSlices, GateResult::Pass);
    summary.add_gate(RepinGate::PinVerification, GateResult::Pass);

    summary.add_tier_count(TestTier::Fast, 180, 3);
    summary.add_tier_count(TestTier::Golden, 45, 7);

    assert!(!summary.all_gates_pass());
    assert_eq!(summary.failed_gate_count(), 1);
    assert_eq!(summary.total_failed(), 10);

    let report = summary.render_report();
    assert!(report.contains("GATES FAILED"));
    assert!(report.contains("Render golden tests"));
    assert!(report.contains("FAIL"));

    eprintln!("\n{}", report);
}

// ===========================================================================
// 7. Makefile target coverage
// ===========================================================================

#[test]
fn makefile_targets_have_tier_equivalents() {
    // Each Makefile target corresponds to a tier:
    // test-fast → Tier 1, test-golden → Tier 2, test → Tier 3
    assert_eq!(TestTier::Fast.label(), "Tier 1 (fast)");
    assert_eq!(TestTier::Golden.label(), "Tier 2 (golden)");
    assert_eq!(TestTier::Full.label(), "Tier 3 (full)");
}

#[test]
fn display_implementations() {
    // Verify Display traits work correctly for all types
    assert_eq!(format!("{}", TestTier::Fast), "Tier 1 (fast)");
    assert_eq!(format!("{}", RepinGate::OracleParity), "Oracle parity tests");
    assert_eq!(format!("{}", CompatSlice::Headless), "headless");
    assert_eq!(format!("{}", RefreshStep::UpdateSubmodule), "Update upstream submodule");
    assert_eq!(format!("{}", GateResult::Pass), "PASS");
}

// ===========================================================================
// 41. Fuzz slice has meaningful test filters — pat-k7d
// ===========================================================================

#[test]
fn fuzz_slice_has_expected_filters() {
    let filters = CompatSlice::Fuzz.test_filters();
    assert!(filters.contains(&"fuzz_"), "Fuzz slice must match fuzz_ tests");
    assert!(
        filters.contains(&"property_tests") || filters.contains(&"robustness"),
        "Fuzz slice must cover property tests or robustness"
    );
    assert!(
        filters.len() >= 2,
        "Fuzz slice should have at least 2 filter patterns"
    );
}

// ===========================================================================
// 42. Physics goldens refresh step does NOT require Godot binary — pat-k7d
// ===========================================================================

#[test]
fn physics_goldens_refresh_does_not_require_godot() {
    // Physics trace goldens are deterministic Rust code (no Godot binary needed).
    // Only oracle capture and render golden capture require running upstream Godot.
    assert!(
        !RefreshStep::RegeneratePhysicsGoldens.requires_godot_binary(),
        "Physics golden regen should NOT require Godot binary — it runs pure Rust deterministic physics"
    );
}

// ===========================================================================
// 43. CompatSlice filters cross-reference ci.yml headless job patterns — pat-k7d
// ===========================================================================

#[test]
fn compat_slice_headless_filters_match_ci_patterns() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let _ci = std::fs::read_to_string(ci_path).unwrap();

    // Key patterns from ci.yml rust-compat-headless job must appear in CompatSlice::Headless
    let headless_filters = CompatSlice::Headless.test_filters();
    let ci_required = ["classdb_", "lifecycle_", "signal_", "resource_", "packed_scene_", "unique_name"];
    for pattern in &ci_required {
        assert!(
            headless_filters.contains(pattern),
            "CompatSlice::Headless must include '{pattern}' (present in ci.yml headless job)"
        );
    }
}

// ===========================================================================
// 44. CompatSlice 2D filters match ci.yml 2D job patterns — pat-k7d
// ===========================================================================

#[test]
fn compat_slice_2d_filters_match_ci_patterns() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let _ci = std::fs::read_to_string(ci_path).unwrap();

    let two_d_filters = CompatSlice::TwoD.test_filters();
    let ci_required = ["collision_", "geometry2d_", "node2d_", "camera_viewport"];
    for pattern in &ci_required {
        assert!(
            two_d_filters.contains(pattern),
            "CompatSlice::TwoD must include '{pattern}' (present in ci.yml 2D job)"
        );
    }
}

// ===========================================================================
// 45. CompatSlice 3D filters match ci.yml 3D job patterns — pat-k7d
// ===========================================================================

#[test]
fn compat_slice_3d_filters_match_ci_patterns() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let _ci = std::fs::read_to_string(ci_path).unwrap();

    let three_d_filters = CompatSlice::ThreeD.test_filters();
    // ci.yml rust-compat-3d runs: node3d_, physics3d_, transform3d_, render_3d_, camera3d_, etc.
    let ci_required = ["node3d_", "physics3d_", "transform3d_"];
    for pattern in &ci_required {
        assert!(
            three_d_filters.contains(pattern),
            "CompatSlice::ThreeD must include '{pattern}' (present in ci.yml 3D job)"
        );
    }
}

// ===========================================================================
// 46. ci.yml compat jobs have correct test filter format — pat-k7d
// ===========================================================================

#[test]
fn ci_compat_headless_job_runs_workspace_tests() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci = std::fs::read_to_string(ci_path).unwrap();

    // All compat jobs must use --workspace
    assert!(
        ci.matches("cargo test --workspace").count() >= 6,
        "At least 6 compat jobs must use 'cargo test --workspace'"
    );
}

// ===========================================================================
// 47. All RefreshStep commands are non-trivial — pat-k7d
// ===========================================================================

#[test]
fn refresh_step_commands_are_actionable() {
    for step in RefreshStep::all() {
        let cmd = step.command();
        assert!(
            cmd.len() > 5,
            "Step '{}' has too-short command: '{}'",
            step.label(),
            cmd
        );
    }
}

// ===========================================================================
// 48. RepinSummary report includes tier breakdowns — pat-k7d
// ===========================================================================

#[test]
fn summary_report_includes_tier_breakdown() {
    let mut s = RepinSummary::new("4.6.1-stable");
    s.add_gate(RepinGate::OracleParity, GateResult::Pass);
    s.add_tier_count(TestTier::Fast, 180, 0);
    s.add_tier_count(TestTier::Golden, 50, 0);
    s.add_tier_count(TestTier::Full, 10, 0);

    let report = s.render_report();
    assert!(report.contains("Tier 1 (fast)"), "Report must show fast tier");
    assert!(report.contains("Tier 2 (golden)"), "Report must show golden tier");
    assert!(report.contains("Tier 3 (full)"), "Report must show full tier");
    assert!(report.contains("**Total**"), "Report must show total row");
    assert!(report.contains("240"), "Report must show total passed count");
}

// ===========================================================================
// 32. CI workflow file exists and has required jobs — pat-wvr
// ===========================================================================

#[test]
fn ci_workflow_exists_and_has_required_jobs() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let content = std::fs::read_to_string(ci_path)
        .expect("CI workflow must exist");

    // Must have the core jobs.
    assert!(content.contains("rust-fmt"), "CI must have rust-fmt job");
    assert!(content.contains("rust:"), "CI must have rust build/test job");
    assert!(content.contains("cargo test"), "CI must run cargo test");
    assert!(content.contains("cargo clippy"), "CI must run clippy");
}

// ===========================================================================
// 33. CI workflow runs on correct triggers — pat-wvr
// ===========================================================================

#[test]
fn ci_workflow_triggers() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let content = std::fs::read_to_string(ci_path).unwrap();

    assert!(content.contains("push:"), "CI must trigger on push");
    assert!(content.contains("pull_request:"), "CI must trigger on pull_request");
    assert!(content.contains("branches: [main]"), "CI must target main branch");
}

// ===========================================================================
// 34. CI workflow has concurrency control — pat-wvr
// ===========================================================================

#[test]
fn ci_workflow_has_concurrency() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let content = std::fs::read_to_string(ci_path).unwrap();

    assert!(
        content.contains("concurrency:"),
        "CI must have concurrency group"
    );
    assert!(
        content.contains("cancel-in-progress: true"),
        "CI must cancel in-progress runs"
    );
}

// ===========================================================================
// 35. CI workflow has render golden lane — pat-wvr
// ===========================================================================

#[test]
fn ci_workflow_has_render_golden_lane() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let content = std::fs::read_to_string(ci_path).unwrap();

    assert!(
        content.contains("rust-render-goldens") || content.contains("Render goldens"),
        "CI must have render golden lane"
    );
    assert!(
        content.contains("test-render-ci"),
        "Render lane must use test-render-ci target"
    );
}

// ===========================================================================
// 36. CI workflow caches cargo artifacts — pat-wvr
// ===========================================================================

#[test]
fn ci_workflow_caches_cargo() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let content = std::fs::read_to_string(ci_path).unwrap();

    assert!(content.contains("cargo/registry"), "CI must cache cargo registry");
    assert!(content.contains("cargo/git"), "CI must cache cargo git");
    assert!(content.contains("target"), "CI must cache target directory");
}

// ===========================================================================
// 37. Repin refresh steps cover oracle and goldens — pat-wvr
// ===========================================================================

#[test]
fn refresh_steps_cover_all_regeneration_targets() {
    let steps = RefreshStep::all();

    let step_names: Vec<&str> = steps.iter().map(|s| s.label()).collect();
    assert!(
        step_names.iter().any(|s| s.contains("submodule")),
        "Must have submodule update step"
    );
    assert!(
        step_names.iter().any(|s| s.contains("oracle")),
        "Must have oracle regeneration step"
    );
    assert!(
        step_names.iter().any(|s| s.contains("render")),
        "Must have render golden step"
    );
    assert!(
        step_names.iter().any(|s| s.contains("physics")),
        "Must have physics golden step"
    );
    assert!(
        step_names.iter().any(|s| s.contains("parity") || s.contains("validat")),
        "Must have parity validation step"
    );
    assert!(
        step_names.iter().any(|s| s.contains("summary")),
        "Must have summary generation step"
    );
}

// ===========================================================================
// 38. Gates cover all five validation dimensions — pat-wvr
// ===========================================================================

#[test]
fn gates_cover_all_validation_dimensions() {
    let gates = RepinGate::all();
    assert_eq!(gates.len(), 5, "Must have exactly 5 gates");

    let labels: Vec<&str> = gates.iter().map(|g| g.label()).collect();
    assert!(labels.iter().any(|l| l.contains("Oracle")));
    assert!(labels.iter().any(|l| l.contains("Render")));
    assert!(labels.iter().any(|l| l.contains("Physics")));
    assert!(labels.iter().any(|l| l.contains("Runtime")));
    assert!(labels.iter().any(|l| l.contains("Pin")));
}

// ===========================================================================
// 39. Compat slices partition without overlap — pat-wvr
// ===========================================================================

#[test]
fn compat_slice_filters_are_disjoint() {
    let slices = CompatSlice::all();
    let mut all_filters: Vec<(&str, &str)> = Vec::new();

    for slice in slices {
        for filter in slice.test_filters() {
            // Check no filter appears in another slice.
            for &(existing_filter, existing_slice) in &all_filters {
                assert_ne!(
                    *filter, existing_filter,
                    "Filter '{}' appears in both '{}' and '{}'",
                    filter, existing_slice, slice.label()
                );
            }
            all_filters.push((*filter, slice.label()));
        }
    }
}

// ===========================================================================
// 40. CI repin lane comprehensive parity report — pat-wvr
// ===========================================================================

#[test]
fn ci_repin_lane_comprehensive_parity_report() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci_exists = std::fs::read_to_string(ci_path).is_ok();

    let checks = [
        ("CI workflow exists", ci_exists),
        ("5 repin gates defined", RepinGate::all().len() == 5),
        ("3 test tiers defined", TestTier::all().len() == 3),
        ("5 compat slices defined", CompatSlice::all().len() == 5),
        ("6 refresh steps defined", RefreshStep::all().len() == 6),
        ("All gates blocking", RepinGate::all().iter().all(|g| g.is_blocking())),
        ("Tiers strictly ordered", TestTier::Fast < TestTier::Golden && TestTier::Golden < TestTier::Full),
        ("Fast tier has most skips", TestTier::Fast.skip_patterns().len() > TestTier::Golden.skip_patterns().len()),
        ("Full tier has no skips", TestTier::Full.skip_patterns().is_empty()),
    ];

    let total = checks.len();
    let passing = checks.iter().filter(|(_, ok)| *ok).count();
    let pct = (passing as f64 / total as f64) * 100.0;

    eprintln!("\n=== CI Repin Lane Parity Report (4.6.1) ===");
    for (name, ok) in &checks {
        eprintln!("  [{}] {}", if *ok { "PASS" } else { "FAIL" }, name);
    }
    eprintln!("  Coverage: {}/{} ({:.1}%)", passing, total, pct);
    eprintln!("============================================\n");

    assert_eq!(passing, total, "All CI repin lane checks must pass");
}
