//! Integration tests for memory profiler CI gate.
//!
//! Validates the full CI pipeline integration: profiler → budget → gate check
//! → structured reports → exit codes for bead pat-2todb.

use gdcore::memory_profiler::{
    AllocationTag, CiMemoryGate, GateResult, MemoryBudget, MemoryProfiler,
};

// ---------------------------------------------------------------------------
// End-to-end: allocate → check gate → pass/fail
// ---------------------------------------------------------------------------

#[test]
fn clean_session_passes_gate() {
    let mut profiler = MemoryProfiler::new();
    let id1 = profiler.record_alloc(AllocationTag::Scene, 1024, "Player");
    let id2 = profiler.record_alloc(AllocationTag::Resource, 2048, "Texture");
    profiler.record_free(id1);
    profiler.record_free(id2);

    let gate = CiMemoryGate::new("clean-session");
    let result = gate.check(&profiler);
    assert!(result.passed());
    assert_eq!(result.exit_code(), 0);
}

#[test]
fn leaked_allocation_fails_gate() {
    let mut profiler = MemoryProfiler::new();
    profiler.record_alloc(AllocationTag::Scene, 1024, "Leaked node");
    let id2 = profiler.record_alloc(AllocationTag::Resource, 512, "Freed resource");
    profiler.record_free(id2);

    let gate = CiMemoryGate::new("leak-test");
    let result = gate.check(&profiler);
    assert!(!result.passed());
    assert_eq!(result.exit_code(), 1);
    assert_eq!(result.reasons().len(), 1);
    assert!(result.reasons()[0].contains("1 leak(s)"));
    assert!(result.reasons()[0].contains("1024 bytes"));
}

#[test]
fn budget_exceeded_fails_gate() {
    let mut profiler = MemoryProfiler::new();
    let id = profiler.record_alloc(AllocationTag::Render, 10_000, "Large buffer");

    let budget = MemoryBudget::unlimited().with_total_limit(5000);
    let gate = CiMemoryGate::new("budget-test")
        .with_budget(budget)
        .with_zero_leaks(false);

    let result = gate.check(&profiler);
    assert!(!result.passed());
    assert!(result.reasons()[0].contains("total_bytes"));

    profiler.record_free(id);
}

#[test]
fn tag_budget_exceeded_fails_gate() {
    let mut profiler = MemoryProfiler::new();
    profiler.record_alloc(AllocationTag::Audio, 8000, "audio buffer");

    let budget = MemoryBudget::unlimited().with_tag_limit(AllocationTag::Audio, 4000);
    let gate = CiMemoryGate::new("tag-budget")
        .with_budget(budget)
        .with_zero_leaks(false);

    let result = gate.check(&profiler);
    assert!(!result.passed());
    assert!(result.reasons()[0].contains("tag_audio"));
}

#[test]
fn count_limit_exceeded_fails_gate() {
    let mut profiler = MemoryProfiler::new();
    for i in 0..20 {
        profiler.record_alloc(AllocationTag::General, 10, &format!("item {}", i));
    }

    let budget = MemoryBudget::unlimited().with_count_limit(10);
    let gate = CiMemoryGate::new("count-limit")
        .with_budget(budget)
        .with_zero_leaks(false);

    let result = gate.check(&profiler);
    assert!(!result.passed());
    assert!(result.reasons()[0].contains("live_count"));
}

// ---------------------------------------------------------------------------
// Combined budget + leak failures
// ---------------------------------------------------------------------------

#[test]
fn multiple_violations_all_reported() {
    let mut profiler = MemoryProfiler::new();
    profiler.record_alloc(AllocationTag::Scene, 5000, "leaked + over budget");

    let budget = MemoryBudget::unlimited()
        .with_total_limit(1000)
        .with_count_limit(0);
    let gate = CiMemoryGate::new("multi-fail").with_budget(budget);

    let result = gate.check(&profiler);
    assert!(!result.passed());
    // Should have at least 3: total_bytes, live_count, leaks
    assert!(result.reasons().len() >= 3);
}

// ---------------------------------------------------------------------------
// Snapshot integration
// ---------------------------------------------------------------------------

#[test]
fn snapshot_captured_before_gate_check() {
    let mut profiler = MemoryProfiler::new();
    let id = profiler.record_alloc(AllocationTag::Scene, 2048, "Player");
    profiler.snapshot("after scene load");
    profiler.record_free(id);
    profiler.snapshot("after cleanup");

    let snaps = profiler.snapshots();
    assert_eq!(snaps.len(), 2);
    assert_eq!(snaps[0].total_bytes, 2048);
    assert_eq!(snaps[0].live_count, 1);
    assert_eq!(snaps[0].label, "after scene load");
    assert_eq!(snaps[1].total_bytes, 0);
    assert_eq!(snaps[1].live_count, 0);

    let gate = CiMemoryGate::new("snapshot-test");
    assert!(gate.check(&profiler).passed());
}

// ---------------------------------------------------------------------------
// JSON reports
// ---------------------------------------------------------------------------

#[test]
fn json_report_contains_all_fields() {
    let mut profiler = MemoryProfiler::new();
    profiler.record_alloc(AllocationTag::Scene, 1000, "node A");
    profiler.record_alloc(AllocationTag::Resource, 2000, "texture B");

    let json = profiler.json_report();
    assert!(json.contains("\"current_bytes\": 3000"));
    assert!(json.contains("\"peak_bytes\": 3000"));
    assert!(json.contains("\"live_count\": 2"));
    assert!(json.contains("\"total_allocated\": 3000"));
    assert!(json.contains("\"total_freed\": 0"));
    assert!(json.contains("\"scene\": 1000"));
    assert!(json.contains("\"resource\": 2000"));
    assert!(json.contains("\"leak_count\": 2"));
}

#[test]
fn json_report_after_free_shows_zero_current() {
    let mut profiler = MemoryProfiler::new();
    let id = profiler.record_alloc(AllocationTag::General, 500, "temp");
    profiler.record_free(id);

    let json = profiler.json_report();
    assert!(json.contains("\"current_bytes\": 0"));
    assert!(json.contains("\"peak_bytes\": 500"));
    assert!(json.contains("\"total_allocated\": 500"));
    assert!(json.contains("\"total_freed\": 500"));
    assert!(json.contains("\"leak_count\": 0"));
}

#[test]
fn gate_json_report_pass() {
    let profiler = MemoryProfiler::new();
    let gate = CiMemoryGate::new("pass-json");
    let json = gate.run_json_report(&profiler);
    assert!(json.contains("\"gate\": \"pass-json\""));
    assert!(json.contains("\"status\": \"pass\""));
    assert!(json.contains("\"exit_code\": 0"));
}

#[test]
fn gate_json_report_fail() {
    let mut profiler = MemoryProfiler::new();
    profiler.record_alloc(AllocationTag::Physics, 1024, "leaked body");
    let gate = CiMemoryGate::new("fail-json");
    let json = gate.run_json_report(&profiler);
    assert!(json.contains("\"status\": \"fail\""));
    assert!(json.contains("\"exit_code\": 1"));
    assert!(json.contains("leak(s)"));
}

#[test]
fn gate_text_report_pass() {
    let profiler = MemoryProfiler::new();
    let gate = CiMemoryGate::new("text-pass");
    let report = gate.run_report(&profiler);
    assert!(report.contains("CI Memory Gate: text-pass"));
    assert!(report.contains("GATE: PASS"));
    assert!(report.contains("No leaks detected"));
}

#[test]
fn gate_text_report_fail() {
    let mut profiler = MemoryProfiler::new();
    profiler.record_alloc(AllocationTag::Editor, 256, "leaked panel");
    let gate = CiMemoryGate::new("text-fail");
    let report = gate.run_report(&profiler);
    assert!(report.contains("GATE: FAIL"));
    assert!(report.contains("1 violation(s)"));
}

// ---------------------------------------------------------------------------
// Environment-based configuration
// ---------------------------------------------------------------------------

#[test]
fn from_env_with_all_vars() {
    let gate = CiMemoryGate::from_env_reader("env-full", |key| match key {
        "PATINA_MEM_MAX_BYTES" => Ok("65536".into()),
        "PATINA_MEM_MAX_ALLOCS" => Ok("100".into()),
        "PATINA_MEM_REQUIRE_ZERO_LEAKS" => Ok("true".into()),
        "PATINA_MEM_TAG_LIMIT_RENDER" => Ok("32768".into()),
        "PATINA_MEM_TAG_LIMIT_AUDIO" => Ok("16384".into()),
        _ => Err(std::env::VarError::NotPresent),
    });

    assert_eq!(gate.budget().max_total_bytes, Some(65536));
    assert_eq!(gate.budget().max_live_count, Some(100));
    assert!(gate.requires_zero_leaks());
    assert_eq!(
        gate.budget().tag_limits.get(&AllocationTag::Render),
        Some(&32768)
    );
    assert_eq!(
        gate.budget().tag_limits.get(&AllocationTag::Audio),
        Some(&16384)
    );
}

#[test]
fn from_env_no_vars_has_defaults() {
    let gate = CiMemoryGate::from_env_reader("env-empty", |_| Err(std::env::VarError::NotPresent));

    assert!(gate.budget().max_total_bytes.is_none());
    assert!(gate.budget().max_live_count.is_none());
    assert!(gate.requires_zero_leaks());
    assert!(gate.budget().tag_limits.is_empty());
}

#[test]
fn from_env_disable_zero_leaks() {
    let gate = CiMemoryGate::from_env_reader("env-no-leaks", |key| match key {
        "PATINA_MEM_REQUIRE_ZERO_LEAKS" => Ok("false".into()),
        _ => Err(std::env::VarError::NotPresent),
    });
    assert!(!gate.requires_zero_leaks());
}

#[test]
fn from_env_disable_zero_leaks_with_zero() {
    let gate = CiMemoryGate::from_env_reader("env-0", |key| match key {
        "PATINA_MEM_REQUIRE_ZERO_LEAKS" => Ok("0".into()),
        _ => Err(std::env::VarError::NotPresent),
    });
    assert!(!gate.requires_zero_leaks());
}

#[test]
fn from_env_invalid_numbers_ignored() {
    let gate = CiMemoryGate::from_env_reader("env-bad", |key| match key {
        "PATINA_MEM_MAX_BYTES" => Ok("not_a_number".into()),
        "PATINA_MEM_MAX_ALLOCS" => Ok("".into()),
        _ => Err(std::env::VarError::NotPresent),
    });
    assert!(gate.budget().max_total_bytes.is_none());
    assert!(gate.budget().max_live_count.is_none());
}

// ---------------------------------------------------------------------------
// Env-configured gate end-to-end
// ---------------------------------------------------------------------------

#[test]
fn env_gate_passes_within_limits() {
    let mut profiler = MemoryProfiler::new();
    let id = profiler.record_alloc(AllocationTag::Scene, 1000, "player");
    profiler.record_free(id);

    let gate = CiMemoryGate::from_env_reader("env-pass", |key| match key {
        "PATINA_MEM_MAX_BYTES" => Ok("10000".into()),
        _ => Err(std::env::VarError::NotPresent),
    });

    assert!(gate.check(&profiler).passed());
}

#[test]
fn env_gate_fails_over_limits() {
    let mut profiler = MemoryProfiler::new();
    profiler.record_alloc(AllocationTag::Render, 50000, "huge buffer");

    let gate = CiMemoryGate::from_env_reader("env-fail", |key| match key {
        "PATINA_MEM_MAX_BYTES" => Ok("10000".into()),
        "PATINA_MEM_REQUIRE_ZERO_LEAKS" => Ok("false".into()),
        _ => Err(std::env::VarError::NotPresent),
    });

    let result = gate.check(&profiler);
    assert!(!result.passed());
    assert!(result.reasons()[0].contains("total_bytes"));
}

// ---------------------------------------------------------------------------
// GateResult API
// ---------------------------------------------------------------------------

#[test]
fn gate_result_pass_properties() {
    let r = GateResult::Pass;
    assert!(r.passed());
    assert_eq!(r.exit_code(), 0);
    assert!(r.reasons().is_empty());
}

#[test]
fn gate_result_fail_properties() {
    let r = GateResult::Fail(vec!["reason A".into(), "reason B".into()]);
    assert!(!r.passed());
    assert_eq!(r.exit_code(), 1);
    assert_eq!(r.reasons().len(), 2);
    assert_eq!(r.reasons()[0], "reason A");
}

// ---------------------------------------------------------------------------
// Peak tracking through gate
// ---------------------------------------------------------------------------

#[test]
fn peak_survives_free_and_gate_reports_it() {
    let mut profiler = MemoryProfiler::new();
    let a = profiler.record_alloc(AllocationTag::General, 5000, "big");
    let b = profiler.record_alloc(AllocationTag::General, 3000, "medium");
    assert_eq!(profiler.peak_bytes(), 8000);
    profiler.record_free(a);
    profiler.record_free(b);
    assert_eq!(profiler.peak_bytes(), 8000);
    assert_eq!(profiler.current_bytes(), 0);

    let gate = CiMemoryGate::new("peak-test");
    assert!(gate.check(&profiler).passed());
    let json = profiler.json_report();
    assert!(json.contains("\"peak_bytes\": 8000"));
    assert!(json.contains("\"current_bytes\": 0"));
}

// ---------------------------------------------------------------------------
// Realistic CI scenario
// ---------------------------------------------------------------------------

#[test]
fn realistic_ci_scenario() {
    let mut profiler = MemoryProfiler::new();

    // Simulate scene load
    let scene_nodes: Vec<u64> = (0..10)
        .map(|i| profiler.record_alloc(AllocationTag::Scene, 256, &format!("node_{}", i)))
        .collect();
    let tex = profiler.record_alloc(AllocationTag::Resource, 4096, "main texture");
    let audio = profiler.record_alloc(AllocationTag::Audio, 2048, "bgm");

    profiler.snapshot("after load");

    // Simulate scene teardown
    for id in &scene_nodes {
        profiler.record_free(*id);
    }
    profiler.record_free(tex);
    profiler.record_free(audio);

    profiler.snapshot("after teardown");

    // Configure gate from "CI environment"
    let gate = CiMemoryGate::from_env_reader("ci-pipeline", |key| match key {
        "PATINA_MEM_MAX_BYTES" => Ok("65536".into()),
        "PATINA_MEM_MAX_ALLOCS" => Ok("50".into()),
        "PATINA_MEM_TAG_LIMIT_RENDER" => Ok("32768".into()),
        _ => Err(std::env::VarError::NotPresent),
    });

    let result = gate.check(&profiler);
    assert!(
        result.passed(),
        "CI gate should pass: {:?}",
        result.reasons()
    );

    // Verify snapshots were captured
    let snaps = profiler.snapshots();
    assert_eq!(snaps.len(), 2);
    assert_eq!(snaps[0].label, "after load");
    assert!(snaps[0].total_bytes > 0);
    assert_eq!(snaps[1].label, "after teardown");
    assert_eq!(snaps[1].total_bytes, 0);

    // Verify JSON report is valid
    let json = gate.run_json_report(&profiler);
    assert!(json.contains("\"status\": \"pass\""));
    assert!(json.contains("\"gate\": \"ci-pipeline\""));
}
