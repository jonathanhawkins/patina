//! Conformance gate: run a whole Godot project through the runner and diff the
//! result against a headless-Godot capture (pat-mgixk.403).
//!
//! The 2026-10-30 port audit found 93 UNWIRED and 19 STUB features sitting
//! behind beads that had closed green, because the `*_parity_test.rs` suites
//! compare Patina against *itself*. This module closes that hole: real Godot
//! 4.6.1 records a trace envelope for each project under
//! `engine-rs/fixtures/conformance/`, Patina produces the same envelope from
//! its own shipping boot path, and the two are diffed.
//!
//! # The envelope
//!
//! ```json
//! {
//!   "project": "timer_queue_free",
//!   "frames": 12,
//!   "prints":  ["ready target=true", ...],
//!   "signals": [{"source": "Main/Timer", "signal": "timeout", "frame": 6}],
//!   "tree":    [{"path": "Main", "class": "Node2D", "properties": {...}}]
//! }
//! ```
//!
//! Node paths are relative to `/root`, so the two engines' differing
//! root-window naming never reads as a tree-shape difference.
//!
//! # What is compared, and what is not
//!
//! * **Tree shape** (`path` + `class`, sorted) — exact. This is the check that
//!   sees a node Godot freed and Patina kept, or a deferred `add_child` that
//!   never landed.
//! * **Signals** (`source` + `signal`) — exact as a multiset. The *frame* a
//!   signal landed on is carried in the envelope for diagnosis but not
//!   compared: the two engines number their first frame differently and that
//!   offset is a property of the harness, not of the port.
//! * **`print()` output** — exact as a multiset, not as an ordered list.
//!   Godot gives a script no hook on `print`, so the Godot side is read off the
//!   process stdout (in true chronological order) while the Patina side is
//!   drained per node from each script's own buffer (in tree order). Comparing
//!   the two orders would fail on that difference in *measurement* — the
//!   bottom-up `_ready` order shows up as a reordering — so the multiset is the
//!   honest comparison. It still catches a missing line, an extra line, and any
//!   wrong value inside a line.
//! * **Properties** — compared only for the names present on BOTH sides, with a
//!   float tolerance. A tracked property Patina does not model is reported as a
//!   note rather than a failure, so the table stays readable instead of every
//!   project failing on the same handful of unmodelled fields.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::{LifecycleManager, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;
use serde_json::{json, Map, Value};

use crate::project_boot;

/// Prefix every conformance fixture puts on its observable `print()` lines.
///
/// Godot's stdout also carries engine warnings, driver chatter and the probe's
/// own message; the marker is what separates the fixture's output from all of
/// it. Patina's side filters on the same prefix so neither engine's incidental
/// logging reaches the diff.
pub const PRINT_MARKER: &str = "[T] ";

/// Frames each project runs for. 12 at a fixed 60 fps is 0.2 s — long enough
/// for a 0.1 s `Timer` to fire and for a deferred call to land, short enough to
/// stay deterministic.
pub const DEFAULT_FRAMES: u64 = 12;

/// Fixed frame delta, matching the capture's `--fixed-fps 60`.
pub const DEFAULT_DELTA: f64 = 1.0 / 60.0;

/// Absolute tolerance for a float property comparison.
///
/// The capture rounds to 3 decimals; a position accumulated over 12 physics
/// ticks can legitimately land a hair off. Anything larger than this is a real
/// behavioural difference.
pub const FLOAT_TOLERANCE: f64 = 0.01;

/// The only node properties compared across engines.
///
/// A narrow, explicit list is the point: a full property dump diverges on
/// dozens of fields no game observes, and the resulting FAIL carries no
/// information about the port. Keep this in step with `TRACKED_PROPERTIES` in
/// `tools/oracle/conformance_probe.gd`.
pub const TRACKED_PROPERTIES: &[&str] = &[
    "autostart",
    "one_shot",
    "position",
    "rotation",
    "scale",
    "text",
    "velocity",
    "visible",
    "wait_time",
];

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// One recorded signal emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalRecord {
    /// Emitting node, as a path relative to `/root`.
    pub source: String,
    /// Signal name.
    pub name: String,
    /// Frame the emission landed on. Diagnostic only — see the module docs.
    pub frame: u64,
}

/// One node in the flattened scene tree.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeRecord {
    /// Path relative to `/root` (e.g. `Main/UI/Label`).
    pub path: String,
    /// Godot class name.
    pub class: String,
    /// Tracked properties this node actually has.
    pub properties: BTreeMap<String, Value>,
}

/// A whole run's observable behaviour, from either engine.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    /// Project directory name.
    pub project: String,
    /// Frames the run completed.
    pub frames: u64,
    /// Marker-prefixed `print()` lines, marker stripped.
    pub prints: Vec<String>,
    /// Signal emissions, in the order observed.
    pub signals: Vec<SignalRecord>,
    /// The final scene tree, sorted by path.
    pub tree: Vec<NodeRecord>,
}

impl Envelope {
    /// Serializes to the on-disk golden shape.
    pub fn to_json(&self) -> Value {
        let signals: Vec<Value> = self
            .signals
            .iter()
            .map(|s| {
                json!({
                    "source": s.source,
                    "signal": s.name,
                    "frame": s.frame,
                })
            })
            .collect();
        let tree: Vec<Value> = self
            .tree
            .iter()
            .map(|n| {
                let mut props = Map::new();
                for (key, value) in &n.properties {
                    props.insert(key.clone(), value.clone());
                }
                json!({
                    "path": n.path,
                    "class": n.class,
                    "properties": Value::Object(props),
                })
            })
            .collect();
        json!({
            "project": self.project,
            "frames": self.frames,
            "prints": self.prints,
            "signals": signals,
            "tree": tree,
        })
    }

    /// Parses a golden captured by `tools/oracle/conformance_capture.py`.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let project = value
            .get("project")
            .and_then(Value::as_str)
            .ok_or("envelope has no string `project`")?
            .to_string();
        let frames = value.get("frames").and_then(Value::as_u64).unwrap_or(0);

        let prints = value
            .get("prints")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default();

        let mut signals = Vec::new();
        if let Some(array) = value.get("signals").and_then(Value::as_array) {
            for entry in array {
                signals.push(SignalRecord {
                    source: entry
                        .get("source")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name: entry
                        .get("signal")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    frame: entry.get("frame").and_then(Value::as_u64).unwrap_or(0),
                });
            }
        }

        let mut tree = Vec::new();
        if let Some(array) = value.get("tree").and_then(Value::as_array) {
            for entry in array {
                let mut properties = BTreeMap::new();
                if let Some(props) = entry.get("properties").and_then(Value::as_object) {
                    for (key, value) in props {
                        properties.insert(key.clone(), value.clone());
                    }
                }
                tree.push(NodeRecord {
                    path: entry
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    class: entry
                        .get("class")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    properties,
                });
            }
        }
        tree.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(Self {
            project,
            frames,
            prints,
            signals,
            tree,
        })
    }
}

// ---------------------------------------------------------------------------
// Capture (Patina side)
// ---------------------------------------------------------------------------

/// Boots `project_dir` through the runner's own project boot and records its
/// trace envelope.
///
/// This is the *same* path `patina-runner <project.godot>` takes — same
/// `project_boot::boot_project`, same lifecycle order, same `MainLoop`
/// configuration, same `register_physics_bodies` — so nothing here can pass a
/// project the shipping runner would fail. `patina-runner --trace-out` calls
/// straight into this function rather than keeping a second copy.
pub fn capture_project(project_dir: &Path, frames: u64, delta: f64) -> Result<Envelope, String> {
    let project = project_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| format!("no project name in '{}'", project_dir.display()))?;

    let booted = project_boot::boot_project(project_dir)?;
    let mut tree = booted.tree;
    tree.event_trace_mut().enable();

    // Autoloads first, then the declared main scene — Godot's boot order, and
    // the order `run_booted_project` uses.
    for (_, node_id) in &booted.autoload_roots {
        LifecycleManager::enter_tree(&mut tree, *node_id);
    }
    if let Some(main_root) = booted.main_scene_root {
        LifecycleManager::enter_tree(&mut tree, main_root);
    }

    let mut main_loop = MainLoop::new(tree);
    main_loop
        .audio_bridge_mut()
        .set_base_dir(project_dir.to_path_buf());
    main_loop.configure_from_project_settings(&booted.settings);
    main_loop.set_input_map(booted.input_map);
    main_loop.register_physics_bodies();

    // Everything printed during `_ready` is already in the script buffers.
    let mut prints = Vec::new();
    drain_prints(main_loop.tree_mut(), &mut prints);

    for _ in 0..frames {
        main_loop.step(delta);
        drain_prints(main_loop.tree_mut(), &mut prints);
        if main_loop.tree().should_quit() {
            break;
        }
    }

    let frame_count = main_loop.frame_count();
    let tree_ref = main_loop.tree();
    Ok(Envelope {
        project,
        frames: frame_count,
        prints,
        signals: collect_signals(tree_ref),
        tree: collect_tree(tree_ref),
    })
}

/// Drains every live script's `print()` buffer, keeping only marker lines.
///
/// Called after `_ready` and once per frame, so a node freed mid-run has
/// already given up its output before it leaves the tree.
fn drain_prints(tree: &mut SceneTree, out: &mut Vec<String>) {
    for node_id in tree.all_nodes_in_tree_order() {
        if let Some(script) = tree.get_script_mut(node_id) {
            for line in script.drain_print_output() {
                if let Some(rest) = line.strip_prefix(PRINT_MARKER) {
                    out.push(rest.to_string());
                }
            }
        }
    }
}

/// Reads the signal emissions out of the tree's event trace.
fn collect_signals(tree: &SceneTree) -> Vec<SignalRecord> {
    let prefix = root_prefix(tree);
    tree.event_trace()
        .events()
        .iter()
        .filter(|event| event.event_type == TraceEventType::SignalEmit)
        .map(|event| SignalRecord {
            source: strip_root(&event.node_path, &prefix),
            name: event.detail.clone(),
            frame: event.frame,
        })
        .collect()
}

/// Flattens the tree into path-sorted records carrying the tracked properties.
fn collect_tree(tree: &SceneTree) -> Vec<NodeRecord> {
    let prefix = root_prefix(tree);
    let mut rows: Vec<NodeRecord> = Vec::new();
    for node_id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(node_id) else {
            continue;
        };
        let Some(absolute) = tree.node_path(node_id) else {
            continue;
        };
        // `/root` itself has no counterpart in the Godot capture, which also
        // reports paths relative to it.
        let Some(relative) = absolute.strip_prefix(&prefix) else {
            continue;
        };
        let mut properties = BTreeMap::new();
        for name in TRACKED_PROPERTIES {
            if !node.has_property(name) {
                continue;
            }
            if let Some(value) = property_json(&node.get_property(name)) {
                properties.insert((*name).to_string(), value);
            }
        }
        rows.push(NodeRecord {
            path: relative.to_string(),
            class: node.class_name().to_string(),
            properties,
        });
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows
}

/// `"/root/"` — the prefix stripped from every reported path.
fn root_prefix(tree: &SceneTree) -> String {
    match tree.node_path(tree.root_id()) {
        Some(path) => format!("{path}/"),
        None => "/root/".to_string(),
    }
}

fn strip_root(path: &str, prefix: &str) -> String {
    path.strip_prefix(prefix).unwrap_or(path).to_string()
}

/// Rounds to the 3 decimals the Godot capture stores, so the two sides agree on
/// representation before the tolerance is applied.
fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Serializes one property to the capture's JSON shape, or `None` for `Nil`
/// (which Godot would not have reported either).
fn property_json(value: &Variant) -> Option<Value> {
    match value {
        Variant::Nil => None,
        Variant::Bool(b) => Some(json!(b)),
        Variant::Int(i) => Some(json!(i)),
        Variant::Float(f) => Some(json!(round3(*f))),
        Variant::String(s) => Some(json!(s)),
        Variant::StringName(s) => Some(json!(s.as_str())),
        Variant::Vector2(v) => Some(json!([round3(v.x as f64), round3(v.y as f64)])),
        Variant::Vector3(v) => Some(json!([
            round3(v.x as f64),
            round3(v.y as f64),
            round3(v.z as f64)
        ])),
        // Anything else is compared by its debug form. A tracked property
        // should not land here; if one does, the mismatch names the type.
        other => Some(json!(format!("{other:?}"))),
    }
}

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

/// The verdict for one project.
#[derive(Debug, Clone)]
pub struct ProjectReport {
    /// Project directory name.
    pub project: String,
    /// `true` when `failures` is empty.
    pub passed: bool,
    /// Differences that fail the project, most structural first.
    pub failures: Vec<String>,
    /// Differences recorded but not failed (see the module docs).
    pub notes: Vec<String>,
}

impl ProjectReport {
    /// One row of the PASS/FAIL table.
    pub fn table_row(&self) -> String {
        format!(
            "{:<24} {:<6} {} failure(s), {} note(s)",
            self.project,
            if self.passed { "PASS" } else { "FAIL" },
            self.failures.len(),
            self.notes.len()
        )
    }
}

/// Diffs a Patina envelope against its headless-Godot golden.
pub fn diff(golden: &Envelope, actual: &Envelope) -> ProjectReport {
    let mut failures = Vec::new();
    let mut notes = Vec::new();

    // 1. Tree shape. Checked first because every other difference is easier to
    //    read once you know the trees agree.
    let golden_shape: Vec<String> = golden
        .tree
        .iter()
        .map(|n| format!("{}|{}", n.path, n.class))
        .collect();
    let actual_shape: Vec<String> = actual
        .tree
        .iter()
        .map(|n| format!("{}|{}", n.path, n.class))
        .collect();
    if golden_shape != actual_shape {
        let golden_set: BTreeSet<&String> = golden_shape.iter().collect();
        let actual_set: BTreeSet<&String> = actual_shape.iter().collect();
        for missing in golden_set.difference(&actual_set) {
            failures.push(format!("tree: missing node {missing}"));
        }
        for extra in actual_set.difference(&golden_set) {
            failures.push(format!("tree: unexpected node {extra}"));
        }
        if failures.is_empty() {
            failures.push(format!(
                "tree: same nodes in a different order: {golden_shape:?} vs {actual_shape:?}"
            ));
        }
    }

    // 2. Signals, as a multiset of `source::signal`.
    let golden_signals = signal_multiset(&golden.signals);
    let actual_signals = signal_multiset(&actual.signals);
    for (key, count) in &golden_signals {
        let seen = actual_signals.get(key).copied().unwrap_or(0);
        if seen < *count {
            failures.push(format!(
                "signal: `{key}` emitted {seen}x, Godot emitted {count}x"
            ));
        }
    }
    for (key, count) in &actual_signals {
        if !golden_signals.contains_key(key) {
            failures.push(format!(
                "signal: `{key}` emitted {count}x, Godot never emitted it"
            ));
        }
    }

    // 3. `print()` output, as a multiset.
    let golden_prints = string_multiset(&golden.prints);
    let actual_prints = string_multiset(&actual.prints);
    for (line, count) in &golden_prints {
        let seen = actual_prints.get(line).copied().unwrap_or(0);
        if seen != *count {
            failures.push(format!(
                "print: `{line}` printed {seen}x, Godot printed it {count}x"
            ));
        }
    }
    for (line, count) in &actual_prints {
        if !golden_prints.contains_key(line) {
            failures.push(format!(
                "print: `{line}` printed {count}x, Godot never printed it"
            ));
        }
    }

    // 4. Properties, for the names present on both sides.
    let actual_by_path: BTreeMap<&str, &NodeRecord> =
        actual.tree.iter().map(|n| (n.path.as_str(), n)).collect();
    for golden_node in &golden.tree {
        let Some(actual_node) = actual_by_path.get(golden_node.path.as_str()) else {
            continue; // Already reported as a tree-shape failure.
        };
        for (name, expected) in &golden_node.properties {
            match actual_node.properties.get(name) {
                None => notes.push(format!(
                    "{}: property `{name}` not modelled (Godot: {expected})",
                    golden_node.path
                )),
                Some(got) if !values_match(expected, got) => failures.push(format!(
                    "{}: property `{name}` is {got}, Godot has {expected}",
                    golden_node.path
                )),
                Some(_) => {}
            }
        }
    }

    ProjectReport {
        project: golden.project.clone(),
        passed: failures.is_empty(),
        failures,
        notes,
    }
}

fn signal_multiset(signals: &[SignalRecord]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for signal in signals {
        *out.entry(format!("{}::{}", signal.source, signal.name))
            .or_insert(0) += 1;
    }
    out
}

fn string_multiset(lines: &[String]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for line in lines {
        *out.entry(line.clone()).or_insert(0) += 1;
    }
    out
}

/// Compares two property values, with a tolerance on every float — including
/// the components of a `Vector2`/`Vector3` array.
fn values_match(expected: &Value, got: &Value) -> bool {
    match (expected, got) {
        (Value::Number(a), Value::Number(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= FLOAT_TOLERANCE,
            _ => a == b,
        },
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| values_match(x, y))
        }
        _ => expected == got,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(path: &str, class: &str, props: &[(&str, Value)]) -> NodeRecord {
        NodeRecord {
            path: path.to_string(),
            class: class.to_string(),
            properties: props
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        }
    }

    fn envelope(tree: Vec<NodeRecord>) -> Envelope {
        Envelope {
            project: "demo".to_string(),
            frames: 12,
            prints: Vec::new(),
            signals: Vec::new(),
            tree,
        }
    }

    #[test]
    fn identical_envelopes_pass() {
        let golden = envelope(vec![node("Main", "Node2D", &[("visible", json!(true))])]);
        let report = diff(&golden, &golden.clone());
        assert!(report.passed, "{:?}", report.failures);
    }

    /// The check that makes the gate worth having: a node Godot freed but
    /// Patina kept is a FAIL, not a note.
    #[test]
    fn a_node_godot_freed_but_patina_kept_fails() {
        let golden = envelope(vec![node("Main", "Node2D", &[])]);
        let actual = envelope(vec![
            node("Main", "Node2D", &[]),
            node("Main/Target", "Node2D", &[]),
        ]);
        let report = diff(&golden, &actual);
        assert!(!report.passed);
        assert!(
            report
                .failures
                .iter()
                .any(|f| f.contains("unexpected node Main/Target")),
            "{:?}",
            report.failures
        );
    }

    #[test]
    fn a_property_patina_does_not_model_is_a_note_not_a_failure() {
        let golden = envelope(vec![node("Main", "Node2D", &[("visible", json!(true))])]);
        let actual = envelope(vec![node("Main", "Node2D", &[])]);
        let report = diff(&golden, &actual);
        assert!(report.passed, "{:?}", report.failures);
        assert_eq!(report.notes.len(), 1, "{:?}", report.notes);
    }

    #[test]
    fn a_wrong_property_value_fails() {
        let golden = envelope(vec![node("Main", "Node2D", &[("text", json!("ready"))])]);
        let actual = envelope(vec![node("Main", "Node2D", &[("text", json!("idle"))])]);
        let report = diff(&golden, &actual);
        assert!(!report.passed);
        assert!(
            report.failures[0].contains("`text`"),
            "{:?}",
            report.failures
        );
    }

    #[test]
    fn floats_compare_within_tolerance_but_not_beyond_it() {
        let near = envelope(vec![node(
            "P",
            "Node2D",
            &[("position", json!([12.0, 0.0]))],
        )]);
        let close = envelope(vec![node(
            "P",
            "Node2D",
            &[("position", json!([12.005, 0.0]))],
        )]);
        let far = envelope(vec![node(
            "P",
            "Node2D",
            &[("position", json!([11.0, 0.0]))],
        )]);
        assert!(diff(&near, &close).passed);
        assert!(!diff(&near, &far).passed);
    }

    #[test]
    fn a_missing_print_line_fails() {
        let mut golden = envelope(vec![node("Main", "Node2D", &[])]);
        golden.prints = vec!["ready".to_string(), "done".to_string()];
        let mut actual = envelope(vec![node("Main", "Node2D", &[])]);
        actual.prints = vec!["ready".to_string()];
        let report = diff(&golden, &actual);
        assert!(!report.passed);
        assert!(
            report.failures.iter().any(|f| f.contains("`done`")),
            "{:?}",
            report.failures
        );
    }

    /// Reordered prints must NOT fail: the two engines' print output is read
    /// through different mechanisms, so their order is a measurement artifact.
    #[test]
    fn reordered_prints_pass() {
        let mut golden = envelope(vec![node("Main", "Node2D", &[])]);
        golden.prints = vec!["a".to_string(), "b".to_string()];
        let mut actual = envelope(vec![node("Main", "Node2D", &[])]);
        actual.prints = vec!["b".to_string(), "a".to_string()];
        assert!(diff(&golden, &actual).passed);
    }

    #[test]
    fn a_signal_patina_never_emitted_fails() {
        let mut golden = envelope(vec![node("Main", "Node2D", &[])]);
        golden.signals = vec![SignalRecord {
            source: "Main/Timer".to_string(),
            name: "timeout".to_string(),
            frame: 6,
        }];
        let actual = envelope(vec![node("Main", "Node2D", &[])]);
        let report = diff(&golden, &actual);
        assert!(!report.passed);
        assert!(
            report
                .failures
                .iter()
                .any(|f| f.contains("Main/Timer::timeout")),
            "{:?}",
            report.failures
        );
    }

    /// A signal that landed on a different frame is not a failure — only its
    /// identity and count are compared.
    #[test]
    fn a_signal_on_a_different_frame_passes() {
        let mut golden = envelope(vec![node("Main", "Node2D", &[])]);
        golden.signals = vec![SignalRecord {
            source: "Main/Timer".to_string(),
            name: "timeout".to_string(),
            frame: 6,
        }];
        let mut actual = envelope(vec![node("Main", "Node2D", &[])]);
        actual.signals = vec![SignalRecord {
            source: "Main/Timer".to_string(),
            name: "timeout".to_string(),
            frame: 7,
        }];
        assert!(diff(&golden, &actual).passed);
    }

    #[test]
    fn envelope_json_round_trips() {
        let mut original = envelope(vec![node(
            "Main/Label",
            "Label",
            &[("text", json!("hi")), ("visible", json!(false))],
        )]);
        original.prints = vec!["ready".to_string()];
        original.signals = vec![SignalRecord {
            source: "Main/Button".to_string(),
            name: "pressed".to_string(),
            frame: 0,
        }];
        let parsed = Envelope::from_json(&original.to_json()).expect("parses");
        assert_eq!(parsed, original);
    }
}
