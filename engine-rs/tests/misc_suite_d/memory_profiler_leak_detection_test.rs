//! Integration tests for the memory profiler: allocation tracking, leak
//! detection, budget enforcement, snapshots, and CI report generation.

use gdcore::memory_profiler::*;

// ---------------------------------------------------------------------------
// Basic allocation tracking
// ---------------------------------------------------------------------------

#[test]
fn fresh_profiler_reports_zero() {
    let p = MemoryProfiler::new();
    assert_eq!(p.current_bytes(), 0);
    assert_eq!(p.peak_bytes(), 0);
    assert_eq!(p.live_count(), 0);
    assert!(p.check_leaks().is_empty());
}

#[test]
fn single_alloc_and_free_cycle() {
    let mut p = MemoryProfiler::new();
    let id = p.record_alloc(AllocationTag::Scene, 4096, "root node");
    assert_eq!(p.current_bytes(), 4096);
    assert_eq!(p.live_count(), 1);

    assert!(p.record_free(id));
    assert_eq!(p.current_bytes(), 0);
    assert_eq!(p.live_count(), 0);
    assert!(p.check_leaks().is_empty());
}

#[test]
fn multiple_allocs_tracked_independently() {
    let mut p = MemoryProfiler::new();
    let a = p.record_alloc(AllocationTag::Resource, 1000, "texture");
    let b = p.record_alloc(AllocationTag::Audio, 2000, "bgm");
    let c = p.record_alloc(AllocationTag::Physics, 500, "body");

    assert_eq!(p.current_bytes(), 3500);
    assert_eq!(p.live_count(), 3);

    p.record_free(b);
    assert_eq!(p.current_bytes(), 1500);
    assert_eq!(p.live_count(), 2);

    p.record_free(a);
    p.record_free(c);
    assert_eq!(p.current_bytes(), 0);
    assert_eq!(p.live_count(), 0);
}

// ---------------------------------------------------------------------------
// Peak tracking
// ---------------------------------------------------------------------------

#[test]
fn peak_never_decreases() {
    let mut p = MemoryProfiler::new();
    let a = p.record_alloc(AllocationTag::General, 1000, "a");
    let b = p.record_alloc(AllocationTag::General, 2000, "b");
    assert_eq!(p.peak_bytes(), 3000);

    p.record_free(a);
    assert_eq!(p.peak_bytes(), 3000);

    p.record_free(b);
    assert_eq!(p.peak_bytes(), 3000);
    assert_eq!(p.current_bytes(), 0);
}

#[test]
fn peak_updates_on_new_high() {
    let mut p = MemoryProfiler::new();
    let a = p.record_alloc(AllocationTag::General, 500, "a");
    assert_eq!(p.peak_bytes(), 500);

    p.record_free(a);
    let _b = p.record_alloc(AllocationTag::General, 800, "b");
    assert_eq!(p.peak_bytes(), 800);
}

// ---------------------------------------------------------------------------
// Leak detection
// ---------------------------------------------------------------------------

#[test]
fn detects_single_leak() {
    let mut p = MemoryProfiler::new();
    p.record_alloc(AllocationTag::Resource, 2048, "leaked texture");
    let freed = p.record_alloc(AllocationTag::Scene, 512, "freed node");
    p.record_free(freed);

    let leaks = p.check_leaks();
    assert_eq!(leaks.len(), 1);
    assert_eq!(leaks[0].label, "leaked texture");
    assert_eq!(leaks[0].size, 2048);
    assert_eq!(leaks[0].tag, AllocationTag::Resource);
}

#[test]
fn detects_multiple_leaks() {
    let mut p = MemoryProfiler::new();
    p.record_alloc(AllocationTag::Script, 100, "leak1");
    p.record_alloc(AllocationTag::Render, 200, "leak2");
    p.record_alloc(AllocationTag::Audio, 300, "leak3");

    let leaks = p.check_leaks();
    assert_eq!(leaks.len(), 3);
    let total_leaked: usize = leaks.iter().map(|l| l.size).sum();
    assert_eq!(total_leaked, 600);
}

#[test]
fn no_leaks_when_all_freed() {
    let mut p = MemoryProfiler::new();
    let ids: Vec<u64> = (0..10)
        .map(|i| p.record_alloc(AllocationTag::General, 100, &format!("alloc-{i}")))
        .collect();
    for id in ids {
        p.record_free(id);
    }
    assert!(p.check_leaks().is_empty());
}

// ---------------------------------------------------------------------------
// Double free / unknown free
// ---------------------------------------------------------------------------

#[test]
fn double_free_returns_false() {
    let mut p = MemoryProfiler::new();
    let id = p.record_alloc(AllocationTag::General, 100, "x");
    assert!(p.record_free(id));
    assert!(!p.record_free(id)); // second free fails
}

#[test]
fn free_unknown_id_returns_false() {
    let mut p = MemoryProfiler::new();
    assert!(!p.record_free(999_999));
}

#[test]
fn double_free_does_not_corrupt_byte_count() {
    let mut p = MemoryProfiler::new();
    let id = p.record_alloc(AllocationTag::General, 500, "x");
    p.record_free(id);
    p.record_free(id); // should be no-op
    assert_eq!(p.current_bytes(), 0);
}

// ---------------------------------------------------------------------------
// Bytes by tag
// ---------------------------------------------------------------------------

#[test]
fn bytes_grouped_by_tag() {
    let mut p = MemoryProfiler::new();
    p.record_alloc(AllocationTag::Scene, 100, "a");
    p.record_alloc(AllocationTag::Scene, 200, "b");
    p.record_alloc(AllocationTag::Resource, 500, "c");
    p.record_alloc(AllocationTag::Physics, 300, "d");

    let by_tag = p.bytes_by_tag();
    assert_eq!(by_tag[&AllocationTag::Scene], 300);
    assert_eq!(by_tag[&AllocationTag::Resource], 500);
    assert_eq!(by_tag[&AllocationTag::Physics], 300);
    assert!(!by_tag.contains_key(&AllocationTag::Audio));
}

#[test]
fn freed_allocations_excluded_from_tag_totals() {
    let mut p = MemoryProfiler::new();
    let a = p.record_alloc(AllocationTag::Scene, 100, "a");
    p.record_alloc(AllocationTag::Scene, 200, "b");
    p.record_free(a);

    let by_tag = p.bytes_by_tag();
    assert_eq!(by_tag[&AllocationTag::Scene], 200);
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

#[test]
fn snapshot_captures_current_state() {
    let mut p = MemoryProfiler::new();
    p.record_alloc(AllocationTag::Scene, 1024, "node");
    p.record_alloc(AllocationTag::Resource, 2048, "texture");

    let snap = p.snapshot("after load");
    assert_eq!(snap.total_bytes, 3072);
    assert_eq!(snap.live_count, 2);
    assert_eq!(snap.label, "after load");
    assert_eq!(snap.by_tag[&AllocationTag::Scene], 1024);
    assert_eq!(snap.by_tag[&AllocationTag::Resource], 2048);
}

#[test]
fn multiple_snapshots_preserved() {
    let mut p = MemoryProfiler::new();
    p.record_alloc(AllocationTag::General, 100, "a");
    p.snapshot("first");

    p.record_alloc(AllocationTag::General, 200, "b");
    p.snapshot("second");

    let snaps = p.snapshots();
    assert_eq!(snaps.len(), 2);
    assert_eq!(snaps[0].total_bytes, 100);
    assert_eq!(snaps[1].total_bytes, 300);
}

// ---------------------------------------------------------------------------
// Budget enforcement
// ---------------------------------------------------------------------------

#[test]
fn budget_total_bytes_violation() {
    let mut p = MemoryProfiler::new();
    p.record_alloc(AllocationTag::General, 2000, "big");

    let budget = MemoryBudget::unlimited().with_total_limit(1000);
    let violations = p.check_budget(&budget);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].kind, "total_bytes");
    assert_eq!(violations[0].limit, 1000);
    assert_eq!(violations[0].actual, 2000);
}

#[test]
fn budget_live_count_violation() {
    let mut p = MemoryProfiler::new();
    for i in 0..5 {
        p.record_alloc(AllocationTag::General, 10, &format!("alloc-{i}"));
    }

    let budget = MemoryBudget::unlimited().with_count_limit(3);
    let violations = p.check_budget(&budget);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].kind, "live_count");
    assert_eq!(violations[0].actual, 5);
}

#[test]
fn budget_tag_limit_violation() {
    let mut p = MemoryProfiler::new();
    p.record_alloc(AllocationTag::Render, 5000, "gpu buffer");

    let budget = MemoryBudget::unlimited().with_tag_limit(AllocationTag::Render, 4096);
    let violations = p.check_budget(&budget);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].kind, "tag_render");
}

#[test]
fn budget_no_violation_when_within_limits() {
    let mut p = MemoryProfiler::new();
    p.record_alloc(AllocationTag::Scene, 500, "node");

    let budget = MemoryBudget::unlimited()
        .with_total_limit(10_000)
        .with_count_limit(100)
        .with_tag_limit(AllocationTag::Scene, 1000);
    assert!(p.check_budget(&budget).is_empty());
}

#[test]
fn budget_multiple_violations_at_once() {
    let mut p = MemoryProfiler::new();
    for i in 0..10 {
        p.record_alloc(AllocationTag::Scene, 1000, &format!("node-{i}"));
    }

    let budget = MemoryBudget::unlimited()
        .with_total_limit(5000)
        .with_count_limit(5)
        .with_tag_limit(AllocationTag::Scene, 8000);
    let violations = p.check_budget(&budget);
    assert_eq!(violations.len(), 3); // total, count, and tag all exceeded
}

#[test]
fn budget_unlimited_never_violates() {
    let mut p = MemoryProfiler::new();
    for i in 0..100 {
        p.record_alloc(AllocationTag::General, 10_000, &format!("big-{i}"));
    }
    assert!(p.check_budget(&MemoryBudget::unlimited()).is_empty());
}

// ---------------------------------------------------------------------------
// Tag names
// ---------------------------------------------------------------------------

#[test]
fn all_allocation_tags_have_names() {
    let tags = [
        AllocationTag::Scene,
        AllocationTag::Resource,
        AllocationTag::Script,
        AllocationTag::Physics,
        AllocationTag::Render,
        AllocationTag::Audio,
        AllocationTag::Editor,
        AllocationTag::General,
    ];
    for tag in &tags {
        assert!(!tag.as_str().is_empty());
    }
}

// ---------------------------------------------------------------------------
// Report generation
// ---------------------------------------------------------------------------

#[test]
fn report_contains_key_sections() {
    let mut p = MemoryProfiler::new();
    p.record_alloc(AllocationTag::Scene, 1024, "player");
    let freed = p.record_alloc(AllocationTag::Resource, 512, "temp");
    p.record_free(freed);

    let report = p.report();
    assert!(report.contains("Memory Profiler Report"));
    assert!(report.contains("Current:"));
    assert!(report.contains("Peak:"));
    assert!(report.contains("Live:"));
    assert!(report.contains("scene:"));
    assert!(report.contains("LEAK"));
    assert!(report.contains("player"));
}

#[test]
fn report_says_no_leaks_when_clean() {
    let mut p = MemoryProfiler::new();
    let id = p.record_alloc(AllocationTag::General, 100, "temp");
    p.record_free(id);

    let report = p.report();
    assert!(report.contains("No leaks detected"));
}

// ---------------------------------------------------------------------------
// Reset
// ---------------------------------------------------------------------------

#[test]
fn reset_clears_everything() {
    let mut p = MemoryProfiler::new();
    p.record_alloc(AllocationTag::General, 1000, "a");
    p.snapshot("before reset");

    p.reset();
    assert_eq!(p.current_bytes(), 0);
    assert_eq!(p.peak_bytes(), 0);
    assert_eq!(p.live_count(), 0);
    assert!(p.check_leaks().is_empty());
    assert!(p.snapshots().is_empty());
}

// ---------------------------------------------------------------------------
// CI scenario: simulate a scene load/unload cycle
// ---------------------------------------------------------------------------

#[test]
fn ci_scene_load_unload_cycle() {
    let mut p = MemoryProfiler::new();

    // Load scene
    let root = p.record_alloc(AllocationTag::Scene, 256, "root");
    let player = p.record_alloc(AllocationTag::Scene, 512, "player");
    let texture = p.record_alloc(AllocationTag::Resource, 4096, "player_texture");
    let script = p.record_alloc(AllocationTag::Script, 128, "player_script");

    p.snapshot("after scene load");
    assert_eq!(p.current_bytes(), 4992);

    // Unload scene
    p.record_free(script);
    p.record_free(texture);
    p.record_free(player);
    p.record_free(root);

    p.snapshot("after scene unload");
    assert_eq!(p.current_bytes(), 0);
    assert!(p.check_leaks().is_empty());

    // Verify snapshots
    let snaps = p.snapshots();
    assert_eq!(snaps[0].total_bytes, 4992);
    assert_eq!(snaps[1].total_bytes, 0);

    // Budget check
    let budget = MemoryBudget::unlimited().with_total_limit(10_000);
    assert!(p.check_budget(&budget).is_empty());
}

#[test]
fn ci_leak_detection_with_budget() {
    let mut p = MemoryProfiler::new();

    // Simulate a leak: allocate but forget to free one resource
    let _leaked = p.record_alloc(AllocationTag::Resource, 8192, "leaked_mesh");
    let freed = p.record_alloc(AllocationTag::Resource, 1024, "temp_buffer");
    p.record_free(freed);

    // Budget should catch the oversized resource allocation
    let budget = MemoryBudget::unlimited().with_tag_limit(AllocationTag::Resource, 4096);
    let violations = p.check_budget(&budget);
    assert_eq!(violations.len(), 1);

    // Leak detection should also catch it
    let leaks = p.check_leaks();
    assert_eq!(leaks.len(), 1);
    assert_eq!(leaks[0].label, "leaked_mesh");
}

// ---------------------------------------------------------------------------
// Default trait
// ---------------------------------------------------------------------------

#[test]
fn default_is_same_as_new() {
    let a = MemoryProfiler::new();
    let b = MemoryProfiler::default();
    assert_eq!(a.current_bytes(), b.current_bytes());
    assert_eq!(a.peak_bytes(), b.peak_bytes());
    assert_eq!(a.live_count(), b.live_count());
}
