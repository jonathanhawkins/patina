//! Memory usage profiling and leak detection.
//!
//! Provides allocation tracking, peak memory measurement, and leak detection
//! for the Patina Engine. Designed for CI integration: the profiler can
//! generate structured reports and fail if leaks or budget overruns are
//! detected.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use gdcore::memory_profiler::{MemoryProfiler, AllocationTag};
//!
//! let mut profiler = MemoryProfiler::new();
//! let id = profiler.record_alloc(AllocationTag::Scene, 1024, "Player node");
//! // ... use the allocation ...
//! profiler.record_free(id);
//! assert!(profiler.check_leaks().is_empty());
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// AllocationTag
// ---------------------------------------------------------------------------

/// Category tags for memory allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllocationTag {
    /// Scene tree nodes.
    Scene,
    /// Resources (textures, meshes, audio, etc.).
    Resource,
    /// Script/GDScript runtime data.
    Script,
    /// Physics simulation data.
    Physics,
    /// Rendering buffers and GPU-related allocations.
    Render,
    /// Audio buffers.
    Audio,
    /// Editor-specific allocations.
    Editor,
    /// General / uncategorized.
    General,
}

impl AllocationTag {
    /// Returns the tag name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            AllocationTag::Scene => "scene",
            AllocationTag::Resource => "resource",
            AllocationTag::Script => "script",
            AllocationTag::Physics => "physics",
            AllocationTag::Render => "render",
            AllocationTag::Audio => "audio",
            AllocationTag::Editor => "editor",
            AllocationTag::General => "general",
        }
    }
}

// ---------------------------------------------------------------------------
// AllocationRecord
// ---------------------------------------------------------------------------

/// A single tracked allocation.
#[derive(Debug, Clone)]
pub struct AllocationRecord {
    /// Unique ID for this allocation.
    pub id: u64,
    /// Category tag.
    pub tag: AllocationTag,
    /// Size in bytes.
    pub size: usize,
    /// Human-readable label describing what this allocation is for.
    pub label: String,
    /// Whether this allocation has been freed.
    pub freed: bool,
}

// ---------------------------------------------------------------------------
// MemorySnapshot
// ---------------------------------------------------------------------------

/// A point-in-time snapshot of memory usage.
#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    /// Total bytes currently allocated.
    pub total_bytes: usize,
    /// Number of live (unfreed) allocations.
    pub live_count: usize,
    /// Bytes per tag.
    pub by_tag: HashMap<AllocationTag, usize>,
    /// Label for this snapshot (e.g. "after scene load").
    pub label: String,
}

// ---------------------------------------------------------------------------
// LeakReport
// ---------------------------------------------------------------------------

/// A detected memory leak.
#[derive(Debug, Clone)]
pub struct LeakReport {
    /// The allocation that was not freed.
    pub id: u64,
    /// Category tag.
    pub tag: AllocationTag,
    /// Size in bytes.
    pub size: usize,
    /// Label from the allocation.
    pub label: String,
}

// ---------------------------------------------------------------------------
// MemoryBudget
// ---------------------------------------------------------------------------

/// A memory budget threshold for CI enforcement.
#[derive(Debug, Clone)]
pub struct MemoryBudget {
    /// Maximum allowed total bytes.
    pub max_total_bytes: Option<usize>,
    /// Maximum allowed live allocations.
    pub max_live_count: Option<usize>,
    /// Per-tag byte limits.
    pub tag_limits: HashMap<AllocationTag, usize>,
}

impl MemoryBudget {
    /// Creates a budget with no limits.
    pub fn unlimited() -> Self {
        Self {
            max_total_bytes: None,
            max_live_count: None,
            tag_limits: HashMap::new(),
        }
    }

    /// Sets the total byte limit.
    pub fn with_total_limit(mut self, bytes: usize) -> Self {
        self.max_total_bytes = Some(bytes);
        self
    }

    /// Sets the live allocation count limit.
    pub fn with_count_limit(mut self, count: usize) -> Self {
        self.max_live_count = Some(count);
        self
    }

    /// Sets a per-tag byte limit.
    pub fn with_tag_limit(mut self, tag: AllocationTag, bytes: usize) -> Self {
        self.tag_limits.insert(tag, bytes);
        self
    }
}

/// A budget violation found during checking.
#[derive(Debug, Clone)]
pub struct BudgetViolation {
    /// What was violated.
    pub kind: String,
    /// The limit that was exceeded.
    pub limit: usize,
    /// The actual value.
    pub actual: usize,
}

// ---------------------------------------------------------------------------
// MemoryProfiler
// ---------------------------------------------------------------------------

static NEXT_ALLOC_ID: AtomicU64 = AtomicU64::new(1);

/// Tracks memory allocations, detects leaks, and enforces budgets.
#[derive(Debug)]
pub struct MemoryProfiler {
    allocations: HashMap<u64, AllocationRecord>,
    peak_bytes: usize,
    total_alloc_bytes: usize,
    total_free_bytes: usize,
    snapshots: Vec<MemorySnapshot>,
}

impl MemoryProfiler {
    /// Creates a new empty profiler.
    pub fn new() -> Self {
        Self {
            allocations: HashMap::new(),
            peak_bytes: 0,
            total_alloc_bytes: 0,
            total_free_bytes: 0,
            snapshots: Vec::new(),
        }
    }

    /// Records an allocation. Returns the allocation ID.
    pub fn record_alloc(&mut self, tag: AllocationTag, size: usize, label: &str) -> u64 {
        let id = NEXT_ALLOC_ID.fetch_add(1, Ordering::Relaxed);
        self.allocations.insert(
            id,
            AllocationRecord {
                id,
                tag,
                size,
                label: label.to_owned(),
                freed: false,
            },
        );
        self.total_alloc_bytes += size;
        let current = self.current_bytes();
        if current > self.peak_bytes {
            self.peak_bytes = current;
        }
        id
    }

    /// Records a deallocation. Returns `false` if the ID was not found or already freed.
    pub fn record_free(&mut self, id: u64) -> bool {
        if let Some(rec) = self.allocations.get_mut(&id) {
            if rec.freed {
                return false;
            }
            rec.freed = true;
            self.total_free_bytes += rec.size;
            true
        } else {
            false
        }
    }

    /// Returns the number of bytes currently live (allocated but not freed).
    pub fn current_bytes(&self) -> usize {
        self.total_alloc_bytes.saturating_sub(self.total_free_bytes)
    }

    /// Returns the peak memory usage seen so far.
    pub fn peak_bytes(&self) -> usize {
        self.peak_bytes
    }

    /// Returns the number of live (unfreed) allocations.
    pub fn live_count(&self) -> usize {
        self.allocations.values().filter(|r| !r.freed).count()
    }

    /// Returns current bytes grouped by tag.
    pub fn bytes_by_tag(&self) -> HashMap<AllocationTag, usize> {
        let mut map = HashMap::new();
        for rec in self.allocations.values() {
            if !rec.freed {
                *map.entry(rec.tag).or_insert(0) += rec.size;
            }
        }
        map
    }

    /// Takes a labeled snapshot of current memory state.
    pub fn snapshot(&mut self, label: &str) -> MemorySnapshot {
        let snap = MemorySnapshot {
            total_bytes: self.current_bytes(),
            live_count: self.live_count(),
            by_tag: self.bytes_by_tag(),
            label: label.to_owned(),
        };
        self.snapshots.push(snap.clone());
        snap
    }

    /// Returns all snapshots taken so far.
    pub fn snapshots(&self) -> &[MemorySnapshot] {
        &self.snapshots
    }

    /// Checks for memory leaks (allocations that were never freed).
    /// Returns a list of leak reports.
    pub fn check_leaks(&self) -> Vec<LeakReport> {
        self.allocations
            .values()
            .filter(|r| !r.freed)
            .map(|r| LeakReport {
                id: r.id,
                tag: r.tag,
                size: r.size,
                label: r.label.clone(),
            })
            .collect()
    }

    /// Checks memory usage against a budget. Returns violations.
    pub fn check_budget(&self, budget: &MemoryBudget) -> Vec<BudgetViolation> {
        let mut violations = Vec::new();

        if let Some(max) = budget.max_total_bytes {
            let current = self.current_bytes();
            if current > max {
                violations.push(BudgetViolation {
                    kind: "total_bytes".into(),
                    limit: max,
                    actual: current,
                });
            }
        }

        if let Some(max) = budget.max_live_count {
            let count = self.live_count();
            if count > max {
                violations.push(BudgetViolation {
                    kind: "live_count".into(),
                    limit: max,
                    actual: count,
                });
            }
        }

        let by_tag = self.bytes_by_tag();
        for (tag, limit) in &budget.tag_limits {
            let actual = by_tag.get(tag).copied().unwrap_or(0);
            if actual > *limit {
                violations.push(BudgetViolation {
                    kind: format!("tag_{}", tag.as_str()),
                    limit: *limit,
                    actual,
                });
            }
        }

        violations
    }

    /// Generates a CI-friendly report string.
    pub fn report(&self) -> String {
        let mut out = String::new();
        out.push_str("=== Memory Profiler Report ===\n");
        out.push_str(&format!("Current: {} bytes\n", self.current_bytes()));
        out.push_str(&format!("Peak:    {} bytes\n", self.peak_bytes));
        out.push_str(&format!("Live:    {} allocations\n", self.live_count()));
        out.push_str(&format!(
            "Total allocated: {} bytes\n",
            self.total_alloc_bytes
        ));
        out.push_str(&format!(
            "Total freed:     {} bytes\n",
            self.total_free_bytes
        ));

        let by_tag = self.bytes_by_tag();
        if !by_tag.is_empty() {
            out.push_str("\nBy category:\n");
            let mut tags: Vec<_> = by_tag.into_iter().collect();
            tags.sort_by(|a, b| b.1.cmp(&a.1));
            for (tag, bytes) in tags {
                out.push_str(&format!("  {}: {} bytes\n", tag.as_str(), bytes));
            }
        }

        let leaks = self.check_leaks();
        if leaks.is_empty() {
            out.push_str("\nNo leaks detected.\n");
        } else {
            out.push_str(&format!("\n{} LEAK(S) DETECTED:\n", leaks.len()));
            for leak in &leaks {
                out.push_str(&format!(
                    "  [{}] {} bytes - {} ({})\n",
                    leak.id,
                    leak.size,
                    leak.label,
                    leak.tag.as_str()
                ));
            }
        }

        out
    }

    /// Resets the profiler, clearing all tracked allocations and snapshots.
    pub fn reset(&mut self) {
        self.allocations.clear();
        self.peak_bytes = 0;
        self.total_alloc_bytes = 0;
        self.total_free_bytes = 0;
        self.snapshots.clear();
    }

    /// Generates a structured JSON report string suitable for CI pipelines.
    pub fn json_report(&self) -> String {
        let by_tag = self.bytes_by_tag();
        let leaks = self.check_leaks();

        let mut tag_entries: Vec<String> = by_tag
            .iter()
            .map(|(tag, bytes)| format!("    \"{}\": {}", tag.as_str(), bytes))
            .collect();
        tag_entries.sort();

        let leak_entries: Vec<String> = leaks
            .iter()
            .map(|l| {
                format!(
                    "    {{\"id\": {}, \"tag\": \"{}\", \"size\": {}, \"label\": \"{}\"}}",
                    l.id,
                    l.tag.as_str(),
                    l.size,
                    l.label.replace('\\', "\\\\").replace('"', "\\\"")
                )
            })
            .collect();

        format!(
            concat!(
                "{{\n",
                "  \"current_bytes\": {},\n",
                "  \"peak_bytes\": {},\n",
                "  \"live_count\": {},\n",
                "  \"total_allocated\": {},\n",
                "  \"total_freed\": {},\n",
                "  \"by_tag\": {{\n{}\n  }},\n",
                "  \"leak_count\": {},\n",
                "  \"leaks\": [\n{}\n  ]\n",
                "}}"
            ),
            self.current_bytes(),
            self.peak_bytes,
            self.live_count(),
            self.total_alloc_bytes,
            self.total_free_bytes,
            tag_entries.join(",\n"),
            leaks.len(),
            leak_entries.join(",\n"),
        )
    }
}

impl Default for MemoryProfiler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// CiMemoryGate
// ---------------------------------------------------------------------------

/// Outcome of a CI memory gate check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateResult {
    /// All checks passed.
    Pass,
    /// One or more checks failed.
    Fail(Vec<String>),
}

impl GateResult {
    /// Returns the process exit code: 0 for pass, 1 for fail.
    pub fn exit_code(&self) -> i32 {
        match self {
            GateResult::Pass => 0,
            GateResult::Fail(_) => 1,
        }
    }

    /// Returns `true` if the gate passed.
    pub fn passed(&self) -> bool {
        matches!(self, GateResult::Pass)
    }

    /// Returns failure reasons, if any.
    pub fn reasons(&self) -> &[String] {
        match self {
            GateResult::Pass => &[],
            GateResult::Fail(reasons) => reasons,
        }
    }
}

/// CI gate that runs a memory profiler check and produces structured output.
///
/// Designed for integration into CI pipelines: configure budgets and
/// zero-leak policies, run the gate, and use the exit code to pass/fail
/// the build.
#[derive(Debug)]
pub struct CiMemoryGate {
    budget: MemoryBudget,
    require_zero_leaks: bool,
    label: String,
}

impl CiMemoryGate {
    /// Creates a new CI gate with the given label and no constraints.
    pub fn new(label: &str) -> Self {
        Self {
            budget: MemoryBudget::unlimited(),
            require_zero_leaks: true,
            label: label.to_owned(),
        }
    }

    /// Sets the memory budget to enforce.
    pub fn with_budget(mut self, budget: MemoryBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Sets whether zero leaks are required (default: true).
    pub fn with_zero_leaks(mut self, required: bool) -> Self {
        self.require_zero_leaks = required;
        self
    }

    /// Creates a gate configured from environment variables.
    ///
    /// Recognized variables:
    /// - `PATINA_MEM_MAX_BYTES` — total byte limit
    /// - `PATINA_MEM_MAX_ALLOCS` — live allocation count limit
    /// - `PATINA_MEM_REQUIRE_ZERO_LEAKS` — "true"/"false" (default: true)
    /// - `PATINA_MEM_TAG_LIMIT_<TAG>` — per-tag byte limit (e.g. `PATINA_MEM_TAG_LIMIT_SCENE`)
    pub fn from_env(label: &str) -> Self {
        Self::from_env_reader(label, |key: &str| std::env::var(key))
    }

    /// Creates a gate from a custom env reader function.
    ///
    /// Useful for testing without modifying real environment variables.
    pub fn from_env_reader<F>(label: &str, reader: F) -> Self
    where
        F: Fn(&str) -> Result<String, std::env::VarError>,
    {
        let mut budget = MemoryBudget::unlimited();

        if let Ok(val) = reader("PATINA_MEM_MAX_BYTES") {
            if let Ok(n) = val.parse::<usize>() {
                budget.max_total_bytes = Some(n);
            }
        }

        if let Ok(val) = reader("PATINA_MEM_MAX_ALLOCS") {
            if let Ok(n) = val.parse::<usize>() {
                budget.max_live_count = Some(n);
            }
        }

        let require_zero_leaks = reader("PATINA_MEM_REQUIRE_ZERO_LEAKS")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);

        let tag_names = [
            ("SCENE", AllocationTag::Scene),
            ("RESOURCE", AllocationTag::Resource),
            ("SCRIPT", AllocationTag::Script),
            ("PHYSICS", AllocationTag::Physics),
            ("RENDER", AllocationTag::Render),
            ("AUDIO", AllocationTag::Audio),
            ("EDITOR", AllocationTag::Editor),
            ("GENERAL", AllocationTag::General),
        ];
        for (env_name, tag) in &tag_names {
            let key = format!("PATINA_MEM_TAG_LIMIT_{}", env_name);
            if let Ok(val) = reader(&key) {
                if let Ok(n) = val.parse::<usize>() {
                    budget.tag_limits.insert(*tag, n);
                }
            }
        }

        Self {
            budget,
            require_zero_leaks,
            label: label.to_owned(),
        }
    }

    /// Runs the gate check against a profiler. Returns the result.
    pub fn check(&self, profiler: &MemoryProfiler) -> GateResult {
        let mut reasons = Vec::new();

        // Check budget violations.
        for v in profiler.check_budget(&self.budget) {
            reasons.push(format!(
                "Budget exceeded: {} — limit {} actual {}",
                v.kind, v.limit, v.actual
            ));
        }

        // Check leaks.
        if self.require_zero_leaks {
            let leaks = profiler.check_leaks();
            if !leaks.is_empty() {
                reasons.push(format!(
                    "{} leak(s) detected ({} bytes)",
                    leaks.len(),
                    leaks.iter().map(|l| l.size).sum::<usize>()
                ));
            }
        }

        if reasons.is_empty() {
            GateResult::Pass
        } else {
            GateResult::Fail(reasons)
        }
    }

    /// Runs the gate and returns a CI-friendly report string.
    pub fn run_report(&self, profiler: &MemoryProfiler) -> String {
        let result = self.check(profiler);
        let mut out = String::new();
        out.push_str(&format!("=== CI Memory Gate: {} ===\n", self.label));
        out.push_str(&profiler.report());
        match &result {
            GateResult::Pass => {
                out.push_str("\nGATE: PASS\n");
            }
            GateResult::Fail(reasons) => {
                out.push_str(&format!("\nGATE: FAIL ({} violation(s))\n", reasons.len()));
                for r in reasons {
                    out.push_str(&format!("  - {}\n", r));
                }
            }
        }
        out
    }

    /// Runs the gate and returns a structured JSON report.
    pub fn run_json_report(&self, profiler: &MemoryProfiler) -> String {
        let result = self.check(profiler);
        let (status, reasons_json) = match &result {
            GateResult::Pass => ("pass".to_string(), "[]".to_string()),
            GateResult::Fail(reasons) => {
                let entries: Vec<String> = reasons
                    .iter()
                    .map(|r| format!("    \"{}\"", r.replace('\\', "\\\\").replace('"', "\\\"")))
                    .collect();
                (
                    "fail".to_string(),
                    format!("[\n{}\n  ]", entries.join(",\n")),
                )
            }
        };

        format!(
            concat!(
                "{{\n",
                "  \"gate\": \"{}\",\n",
                "  \"status\": \"{}\",\n",
                "  \"exit_code\": {},\n",
                "  \"reasons\": {},\n",
                "  \"profiler\": {}\n",
                "}}"
            ),
            self.label.replace('\\', "\\\\").replace('"', "\\\""),
            status,
            result.exit_code(),
            reasons_json,
            profiler.json_report(),
        )
    }

    /// Returns the label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns a reference to the budget.
    pub fn budget(&self) -> &MemoryBudget {
        &self.budget
    }

    /// Returns whether zero leaks are required.
    pub fn requires_zero_leaks(&self) -> bool {
        self.require_zero_leaks
    }
}

// ---------------------------------------------------------------------------
// SnapshotDiff — compare two snapshots
// ---------------------------------------------------------------------------

/// The difference between two memory snapshots.
#[derive(Debug, Clone)]
pub struct SnapshotDiff {
    /// Label of the "before" snapshot.
    pub before_label: String,
    /// Label of the "after" snapshot.
    pub after_label: String,
    /// Change in total bytes (positive = growth).
    pub bytes_delta: i64,
    /// Change in live allocation count.
    pub count_delta: i64,
    /// Per-tag byte deltas.
    pub tag_deltas: HashMap<AllocationTag, i64>,
}

impl SnapshotDiff {
    /// Computes the diff between two snapshots.
    pub fn between(before: &MemorySnapshot, after: &MemorySnapshot) -> Self {
        let bytes_delta = after.total_bytes as i64 - before.total_bytes as i64;
        let count_delta = after.live_count as i64 - before.live_count as i64;

        let mut tag_deltas = HashMap::new();
        // Collect all tags from both snapshots.
        let mut all_tags = std::collections::HashSet::new();
        for tag in before.by_tag.keys() {
            all_tags.insert(*tag);
        }
        for tag in after.by_tag.keys() {
            all_tags.insert(*tag);
        }
        for tag in all_tags {
            let b = *before.by_tag.get(&tag).unwrap_or(&0) as i64;
            let a = *after.by_tag.get(&tag).unwrap_or(&0) as i64;
            let delta = a - b;
            if delta != 0 {
                tag_deltas.insert(tag, delta);
            }
        }

        Self {
            before_label: before.label.clone(),
            after_label: after.label.clone(),
            bytes_delta,
            count_delta,
            tag_deltas,
        }
    }

    /// Returns true if memory grew.
    pub fn grew(&self) -> bool {
        self.bytes_delta > 0
    }

    /// Returns true if memory shrank.
    pub fn shrank(&self) -> bool {
        self.bytes_delta < 0
    }

    /// Renders a human-readable diff report.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Diff: {} → {}\n",
            self.before_label, self.after_label
        ));
        let sign = if self.bytes_delta >= 0 { "+" } else { "" };
        out.push_str(&format!("  Bytes:  {}{}\n", sign, self.bytes_delta));
        let csign = if self.count_delta >= 0 { "+" } else { "" };
        out.push_str(&format!("  Count:  {}{}\n", csign, self.count_delta));
        if !self.tag_deltas.is_empty() {
            out.push_str("  By tag:\n");
            let mut tags: Vec<_> = self.tag_deltas.iter().collect();
            tags.sort_by_key(|(_, d)| std::cmp::Reverse(d.abs()));
            for (tag, delta) in tags {
                let s = if *delta >= 0 { "+" } else { "" };
                out.push_str(&format!("    {}: {}{}\n", tag.as_str(), s, delta));
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// MemoryTrend — detect slow leaks across multiple snapshots
// ---------------------------------------------------------------------------

/// Result of analyzing memory growth trend across snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryTrend {
    /// Memory usage is stable (no significant growth).
    Stable,
    /// Memory is growing — possible slow leak.
    Growing,
    /// Memory is shrinking.
    Shrinking,
    /// Not enough data to determine trend (need >= 3 snapshots).
    Insufficient,
}

impl MemoryTrend {
    /// Returns a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            MemoryTrend::Stable => "Stable",
            MemoryTrend::Growing => "Growing (possible leak)",
            MemoryTrend::Shrinking => "Shrinking",
            MemoryTrend::Insufficient => "Insufficient data",
        }
    }
}

/// Analyzes a sequence of snapshots to detect memory growth trends.
///
/// Returns `Growing` if memory increased in more than half of the
/// consecutive snapshot pairs, `Shrinking` if it decreased in more
/// than half, and `Stable` otherwise.
pub fn analyze_trend(snapshots: &[MemorySnapshot]) -> MemoryTrend {
    if snapshots.len() < 3 {
        return MemoryTrend::Insufficient;
    }
    let mut growing = 0usize;
    let mut shrinking = 0usize;
    for pair in snapshots.windows(2) {
        let delta = pair[1].total_bytes as i64 - pair[0].total_bytes as i64;
        if delta > 0 {
            growing += 1;
        } else if delta < 0 {
            shrinking += 1;
        }
    }
    let pairs = snapshots.len() - 1;
    if growing > pairs / 2 {
        MemoryTrend::Growing
    } else if shrinking > pairs / 2 {
        MemoryTrend::Shrinking
    } else {
        MemoryTrend::Stable
    }
}

/// Computes the growth rate in bytes per snapshot interval.
/// Returns `None` if fewer than 2 snapshots.
pub fn growth_rate(snapshots: &[MemorySnapshot]) -> Option<f64> {
    if snapshots.len() < 2 {
        return None;
    }
    let first = snapshots.first().unwrap().total_bytes as f64;
    let last = snapshots.last().unwrap().total_bytes as f64;
    Some((last - first) / (snapshots.len() - 1) as f64)
}

// ---------------------------------------------------------------------------
// CiTestRunner — run a test closure with memory profiling
// ---------------------------------------------------------------------------

/// Runs a test closure with memory profiling and gate enforcement.
///
/// Returns the gate result and the profiler for further inspection.
///
/// # Example
/// ```rust,ignore
/// let (result, profiler) = run_profiled_test(
///     CiMemoryGate::new("scene-load"),
///     |profiler| {
///         let id = profiler.record_alloc(AllocationTag::Scene, 1024, "node");
///         profiler.record_free(id);
///     },
/// );
/// assert!(result.passed());
/// ```
pub fn run_profiled_test<F>(gate: CiMemoryGate, test_fn: F) -> (GateResult, MemoryProfiler)
where
    F: FnOnce(&mut MemoryProfiler),
{
    let mut profiler = MemoryProfiler::new();
    test_fn(&mut profiler);
    let result = gate.check(&profiler);
    (result, profiler)
}

/// Runs a test closure with periodic snapshots and leak+trend detection.
///
/// The `phases` function receives the profiler and should call
/// `profiler.snapshot("label")` between logical phases.
/// Returns the gate result, trend analysis, and profiler.
pub fn run_phased_test<F>(
    gate: CiMemoryGate,
    phases: F,
) -> (GateResult, MemoryTrend, MemoryProfiler)
where
    F: FnOnce(&mut MemoryProfiler),
{
    let mut profiler = MemoryProfiler::new();
    phases(&mut profiler);
    let result = gate.check(&profiler);
    let trend = analyze_trend(profiler.snapshots());
    (result, trend, profiler)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_profiler_is_empty() {
        let p = MemoryProfiler::new();
        assert_eq!(p.current_bytes(), 0);
        assert_eq!(p.peak_bytes(), 0);
        assert_eq!(p.live_count(), 0);
        assert!(p.check_leaks().is_empty());
    }

    #[test]
    fn alloc_and_free() {
        let mut p = MemoryProfiler::new();
        let id = p.record_alloc(AllocationTag::Scene, 1024, "test node");
        assert_eq!(p.current_bytes(), 1024);
        assert_eq!(p.live_count(), 1);
        assert!(p.record_free(id));
        assert_eq!(p.current_bytes(), 0);
        assert_eq!(p.live_count(), 0);
    }

    #[test]
    fn peak_tracking() {
        let mut p = MemoryProfiler::new();
        let a = p.record_alloc(AllocationTag::General, 500, "a");
        let b = p.record_alloc(AllocationTag::General, 500, "b");
        assert_eq!(p.peak_bytes(), 1000);
        p.record_free(a);
        assert_eq!(p.peak_bytes(), 1000); // peak doesn't decrease
        p.record_free(b);
        assert_eq!(p.current_bytes(), 0);
        assert_eq!(p.peak_bytes(), 1000);
    }

    #[test]
    fn leak_detection() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::Resource, 2048, "leaked texture");
        let id2 = p.record_alloc(AllocationTag::Scene, 512, "freed node");
        p.record_free(id2);

        let leaks = p.check_leaks();
        assert_eq!(leaks.len(), 1);
        assert_eq!(leaks[0].label, "leaked texture");
        assert_eq!(leaks[0].size, 2048);
        assert_eq!(leaks[0].tag, AllocationTag::Resource);
    }

    #[test]
    fn double_free_returns_false() {
        let mut p = MemoryProfiler::new();
        let id = p.record_alloc(AllocationTag::General, 100, "x");
        assert!(p.record_free(id));
        assert!(!p.record_free(id));
    }

    #[test]
    fn free_unknown_id_returns_false() {
        let mut p = MemoryProfiler::new();
        assert!(!p.record_free(9999));
    }

    #[test]
    fn bytes_by_tag() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::Scene, 100, "a");
        p.record_alloc(AllocationTag::Scene, 200, "b");
        p.record_alloc(AllocationTag::Resource, 500, "c");
        let by_tag = p.bytes_by_tag();
        assert_eq!(by_tag[&AllocationTag::Scene], 300);
        assert_eq!(by_tag[&AllocationTag::Resource], 500);
    }

    #[test]
    fn allocation_tag_names() {
        assert_eq!(AllocationTag::Scene.as_str(), "scene");
        assert_eq!(AllocationTag::Render.as_str(), "render");
        assert_eq!(AllocationTag::General.as_str(), "general");
    }

    // -----------------------------------------------------------------------
    // JSON report
    // -----------------------------------------------------------------------

    #[test]
    fn json_report_empty_profiler() {
        let p = MemoryProfiler::new();
        let json = p.json_report();
        assert!(json.contains("\"current_bytes\": 0"));
        assert!(json.contains("\"leak_count\": 0"));
    }

    #[test]
    fn json_report_with_allocations() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::Scene, 1024, "node");
        p.record_alloc(AllocationTag::Resource, 2048, "texture");
        let json = p.json_report();
        assert!(json.contains("\"current_bytes\": 3072"));
        assert!(json.contains("\"scene\": 1024"));
        assert!(json.contains("\"resource\": 2048"));
        assert!(json.contains("\"leak_count\": 2"));
    }

    #[test]
    fn json_report_escapes_labels() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::General, 100, "has \"quotes\" here");
        let json = p.json_report();
        assert!(json.contains("has \\\"quotes\\\" here"));
    }

    // -----------------------------------------------------------------------
    // CiMemoryGate
    // -----------------------------------------------------------------------

    #[test]
    fn gate_pass_clean_profiler() {
        let p = MemoryProfiler::new();
        let gate = CiMemoryGate::new("test");
        let result = gate.check(&p);
        assert!(result.passed());
        assert_eq!(result.exit_code(), 0);
        assert!(result.reasons().is_empty());
    }

    #[test]
    fn gate_fail_on_leaks() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::Scene, 512, "leaked");
        let gate = CiMemoryGate::new("leak-check");
        let result = gate.check(&p);
        assert!(!result.passed());
        assert_eq!(result.exit_code(), 1);
        assert!(result.reasons()[0].contains("1 leak(s)"));
    }

    #[test]
    fn gate_pass_leaks_allowed() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::Scene, 512, "intentional");
        let gate = CiMemoryGate::new("relaxed").with_zero_leaks(false);
        assert!(gate.check(&p).passed());
    }

    #[test]
    fn gate_fail_on_budget_total() {
        let mut p = MemoryProfiler::new();
        let id = p.record_alloc(AllocationTag::General, 2000, "big");
        // Free it so no leak, but still over budget during check
        // Actually we need it live for budget check
        let _ = id;
        let budget = MemoryBudget::unlimited().with_total_limit(1000);
        let gate = CiMemoryGate::new("budget")
            .with_budget(budget)
            .with_zero_leaks(false);
        let result = gate.check(&p);
        assert!(!result.passed());
        assert!(result.reasons()[0].contains("total_bytes"));
    }

    #[test]
    fn gate_fail_on_budget_count() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::General, 10, "a");
        p.record_alloc(AllocationTag::General, 10, "b");
        p.record_alloc(AllocationTag::General, 10, "c");
        let budget = MemoryBudget::unlimited().with_count_limit(2);
        let gate = CiMemoryGate::new("count")
            .with_budget(budget)
            .with_zero_leaks(false);
        assert!(!gate.check(&p).passed());
    }

    #[test]
    fn gate_fail_on_tag_limit() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::Render, 5000, "gpu buffer");
        let budget = MemoryBudget::unlimited().with_tag_limit(AllocationTag::Render, 1000);
        let gate = CiMemoryGate::new("tag")
            .with_budget(budget)
            .with_zero_leaks(false);
        let result = gate.check(&p);
        assert!(!result.passed());
        assert!(result.reasons()[0].contains("tag_render"));
    }

    #[test]
    fn gate_multiple_failures() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::Scene, 5000, "leaked+over budget");
        let budget = MemoryBudget::unlimited().with_total_limit(1000);
        let gate = CiMemoryGate::new("multi").with_budget(budget);
        let result = gate.check(&p);
        assert!(!result.passed());
        assert!(result.reasons().len() >= 2); // budget + leak
    }

    #[test]
    fn gate_from_env_reader() {
        let gate = CiMemoryGate::from_env_reader("env-test", |key| match key {
            "PATINA_MEM_MAX_BYTES" => Ok("4096".into()),
            "PATINA_MEM_MAX_ALLOCS" => Ok("10".into()),
            "PATINA_MEM_REQUIRE_ZERO_LEAKS" => Ok("false".into()),
            "PATINA_MEM_TAG_LIMIT_RENDER" => Ok("2048".into()),
            _ => Err(std::env::VarError::NotPresent),
        });
        assert_eq!(gate.budget().max_total_bytes, Some(4096));
        assert_eq!(gate.budget().max_live_count, Some(10));
        assert!(!gate.requires_zero_leaks());
        assert_eq!(
            gate.budget().tag_limits.get(&AllocationTag::Render),
            Some(&2048)
        );
    }

    #[test]
    fn gate_from_env_defaults() {
        let gate =
            CiMemoryGate::from_env_reader("defaults", |_| Err(std::env::VarError::NotPresent));
        assert!(gate.budget().max_total_bytes.is_none());
        assert!(gate.requires_zero_leaks());
    }

    #[test]
    fn gate_run_report_contains_status() {
        let p = MemoryProfiler::new();
        let gate = CiMemoryGate::new("ci-test");
        let report = gate.run_report(&p);
        assert!(report.contains("GATE: PASS"));
        assert!(report.contains("CI Memory Gate: ci-test"));
    }

    #[test]
    fn gate_run_report_fail_lists_reasons() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::General, 100, "leaked");
        let gate = CiMemoryGate::new("fail-test");
        let report = gate.run_report(&p);
        assert!(report.contains("GATE: FAIL"));
        assert!(report.contains("leak(s)"));
    }

    #[test]
    fn gate_run_json_report_pass() {
        let p = MemoryProfiler::new();
        let gate = CiMemoryGate::new("json-test");
        let json = gate.run_json_report(&p);
        assert!(json.contains("\"status\": \"pass\""));
        assert!(json.contains("\"exit_code\": 0"));
    }

    #[test]
    fn gate_run_json_report_fail() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::General, 100, "leaked");
        let gate = CiMemoryGate::new("json-fail");
        let json = gate.run_json_report(&p);
        assert!(json.contains("\"status\": \"fail\""));
        assert!(json.contains("\"exit_code\": 1"));
    }

    // -----------------------------------------------------------------------
    // SnapshotDiff
    // -----------------------------------------------------------------------

    #[test]
    fn snapshot_diff_growth() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::Scene, 100, "a");
        let s1 = p.snapshot("before");
        p.record_alloc(AllocationTag::Scene, 200, "b");
        let s2 = p.snapshot("after");

        let diff = SnapshotDiff::between(&s1, &s2);
        assert!(diff.grew());
        assert!(!diff.shrank());
        assert_eq!(diff.bytes_delta, 200);
        assert_eq!(diff.count_delta, 1);
        assert_eq!(diff.before_label, "before");
        assert_eq!(diff.after_label, "after");
    }

    #[test]
    fn snapshot_diff_shrink() {
        let mut p = MemoryProfiler::new();
        let a = p.record_alloc(AllocationTag::Resource, 500, "big");
        p.record_alloc(AllocationTag::Resource, 100, "small");
        let s1 = p.snapshot("full");
        p.record_free(a);
        let s2 = p.snapshot("freed");

        let diff = SnapshotDiff::between(&s1, &s2);
        assert!(diff.shrank());
        assert!(!diff.grew());
        assert_eq!(diff.bytes_delta, -500);
    }

    #[test]
    fn snapshot_diff_no_change() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::General, 100, "x");
        let s1 = p.snapshot("a");
        let s2 = p.snapshot("b");

        let diff = SnapshotDiff::between(&s1, &s2);
        assert!(!diff.grew());
        assert!(!diff.shrank());
        assert_eq!(diff.bytes_delta, 0);
        assert_eq!(diff.count_delta, 0);
    }

    #[test]
    fn snapshot_diff_tag_deltas() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::Scene, 100, "s");
        let s1 = p.snapshot("t1");
        p.record_alloc(AllocationTag::Render, 300, "r");
        let s2 = p.snapshot("t2");

        let diff = SnapshotDiff::between(&s1, &s2);
        assert_eq!(*diff.tag_deltas.get(&AllocationTag::Render).unwrap(), 300);
        // Scene didn't change, so it shouldn't appear in deltas
        assert!(!diff.tag_deltas.contains_key(&AllocationTag::Scene));
    }

    #[test]
    fn snapshot_diff_render_contains_labels() {
        let mut p = MemoryProfiler::new();
        let s1 = p.snapshot("phase1");
        p.record_alloc(AllocationTag::General, 50, "x");
        let s2 = p.snapshot("phase2");

        let diff = SnapshotDiff::between(&s1, &s2);
        let rendered = diff.render();
        assert!(rendered.contains("phase1"));
        assert!(rendered.contains("phase2"));
        assert!(rendered.contains("+50"));
    }

    #[test]
    fn snapshot_diff_render_negative() {
        let mut p = MemoryProfiler::new();
        let id = p.record_alloc(AllocationTag::General, 200, "x");
        let s1 = p.snapshot("before");
        p.record_free(id);
        let s2 = p.snapshot("after");

        let diff = SnapshotDiff::between(&s1, &s2);
        let rendered = diff.render();
        assert!(rendered.contains("-200"));
    }

    // -----------------------------------------------------------------------
    // MemoryTrend + analyze_trend
    // -----------------------------------------------------------------------

    #[test]
    fn trend_insufficient_empty() {
        assert_eq!(analyze_trend(&[]), MemoryTrend::Insufficient);
    }

    #[test]
    fn trend_insufficient_two_snapshots() {
        let mut p = MemoryProfiler::new();
        let s1 = p.snapshot("a");
        p.record_alloc(AllocationTag::General, 100, "x");
        let s2 = p.snapshot("b");
        assert_eq!(analyze_trend(&[s1, s2]), MemoryTrend::Insufficient);
    }

    #[test]
    fn trend_growing() {
        let mut p = MemoryProfiler::new();
        let s1 = p.snapshot("t1");
        p.record_alloc(AllocationTag::General, 100, "a");
        let s2 = p.snapshot("t2");
        p.record_alloc(AllocationTag::General, 100, "b");
        let s3 = p.snapshot("t3");
        p.record_alloc(AllocationTag::General, 100, "c");
        let s4 = p.snapshot("t4");

        assert_eq!(analyze_trend(&[s1, s2, s3, s4]), MemoryTrend::Growing);
    }

    #[test]
    fn trend_shrinking() {
        let mut p = MemoryProfiler::new();
        let a = p.record_alloc(AllocationTag::General, 300, "a");
        let b = p.record_alloc(AllocationTag::General, 200, "b");
        let c = p.record_alloc(AllocationTag::General, 100, "c");
        let s1 = p.snapshot("t1");
        p.record_free(c);
        let s2 = p.snapshot("t2");
        p.record_free(b);
        let s3 = p.snapshot("t3");
        p.record_free(a);
        let s4 = p.snapshot("t4");

        assert_eq!(analyze_trend(&[s1, s2, s3, s4]), MemoryTrend::Shrinking);
    }

    #[test]
    fn trend_stable() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::General, 100, "base");
        let s1 = p.snapshot("t1");
        // Alloc and free — net zero each phase
        let id = p.record_alloc(AllocationTag::General, 50, "tmp");
        p.record_free(id);
        let s2 = p.snapshot("t2");
        let id = p.record_alloc(AllocationTag::General, 50, "tmp2");
        p.record_free(id);
        let s3 = p.snapshot("t3");

        assert_eq!(analyze_trend(&[s1, s2, s3]), MemoryTrend::Stable);
    }

    #[test]
    fn trend_labels() {
        assert_eq!(MemoryTrend::Stable.label(), "Stable");
        assert_eq!(MemoryTrend::Growing.label(), "Growing (possible leak)");
        assert_eq!(MemoryTrend::Shrinking.label(), "Shrinking");
        assert_eq!(MemoryTrend::Insufficient.label(), "Insufficient data");
    }

    // -----------------------------------------------------------------------
    // growth_rate
    // -----------------------------------------------------------------------

    #[test]
    fn growth_rate_none_for_empty() {
        assert!(growth_rate(&[]).is_none());
    }

    #[test]
    fn growth_rate_none_for_single() {
        let mut p = MemoryProfiler::new();
        let s = p.snapshot("only");
        assert!(growth_rate(&[s]).is_none());
    }

    #[test]
    fn growth_rate_positive() {
        let mut p = MemoryProfiler::new();
        let s1 = p.snapshot("t1");
        p.record_alloc(AllocationTag::General, 100, "a");
        let s2 = p.snapshot("t2");
        p.record_alloc(AllocationTag::General, 100, "b");
        let s3 = p.snapshot("t3");

        let rate = growth_rate(&[s1, s2, s3]).unwrap();
        assert!((rate - 100.0).abs() < 0.01);
    }

    #[test]
    fn growth_rate_negative() {
        let mut p = MemoryProfiler::new();
        let a = p.record_alloc(AllocationTag::General, 200, "a");
        let s1 = p.snapshot("t1");
        p.record_free(a);
        let s2 = p.snapshot("t2");

        let rate = growth_rate(&[s1, s2]).unwrap();
        assert!((rate - (-200.0)).abs() < 0.01);
    }

    #[test]
    fn growth_rate_zero() {
        let mut p = MemoryProfiler::new();
        p.record_alloc(AllocationTag::General, 100, "x");
        let s1 = p.snapshot("t1");
        let s2 = p.snapshot("t2");

        let rate = growth_rate(&[s1, s2]).unwrap();
        assert!(rate.abs() < 0.01);
    }

    // -----------------------------------------------------------------------
    // run_profiled_test
    // -----------------------------------------------------------------------

    #[test]
    fn profiled_test_pass() {
        let gate = CiMemoryGate::new("test");
        let (result, profiler) = run_profiled_test(gate, |p| {
            let id = p.record_alloc(AllocationTag::General, 100, "tmp");
            p.record_free(id);
        });
        assert!(result.passed());
        assert_eq!(profiler.current_bytes(), 0);
    }

    #[test]
    fn profiled_test_fail_leak() {
        let gate = CiMemoryGate::new("leak-test");
        let (result, profiler) = run_profiled_test(gate, |p| {
            p.record_alloc(AllocationTag::Scene, 512, "leaked");
        });
        assert!(!result.passed());
        assert_eq!(profiler.live_count(), 1);
    }

    #[test]
    fn profiled_test_fail_budget() {
        let budget = MemoryBudget::unlimited().with_total_limit(100);
        let gate = CiMemoryGate::new("budget")
            .with_budget(budget)
            .with_zero_leaks(false);
        let (result, _) = run_profiled_test(gate, |p| {
            p.record_alloc(AllocationTag::General, 500, "over");
        });
        assert!(!result.passed());
    }

    // -----------------------------------------------------------------------
    // run_phased_test
    // -----------------------------------------------------------------------

    #[test]
    fn phased_test_growing() {
        let gate = CiMemoryGate::new("phased").with_zero_leaks(false);
        let (result, trend, profiler) = run_phased_test(gate, |p| {
            p.snapshot("phase0");
            p.record_alloc(AllocationTag::General, 100, "a");
            p.snapshot("phase1");
            p.record_alloc(AllocationTag::General, 100, "b");
            p.snapshot("phase2");
            p.record_alloc(AllocationTag::General, 100, "c");
            p.snapshot("phase3");
        });
        assert!(result.passed()); // no leak check
        assert_eq!(trend, MemoryTrend::Growing);
        assert_eq!(profiler.snapshots().len(), 4);
    }

    #[test]
    fn phased_test_stable() {
        let gate = CiMemoryGate::new("phased-stable");
        let (result, trend, _) = run_phased_test(gate, |p| {
            p.snapshot("p0");
            let id = p.record_alloc(AllocationTag::General, 100, "x");
            p.record_free(id);
            p.snapshot("p1");
            let id = p.record_alloc(AllocationTag::General, 100, "y");
            p.record_free(id);
            p.snapshot("p2");
        });
        assert!(result.passed());
        assert_eq!(trend, MemoryTrend::Stable);
    }

    #[test]
    fn phased_test_insufficient_no_snapshots() {
        let gate = CiMemoryGate::new("phased-none");
        let (_, trend, _) = run_phased_test(gate, |p| {
            let id = p.record_alloc(AllocationTag::General, 100, "tmp");
            p.record_free(id);
        });
        assert_eq!(trend, MemoryTrend::Insufficient);
    }
}
