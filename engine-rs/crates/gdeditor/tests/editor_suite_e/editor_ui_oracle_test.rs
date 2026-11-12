//! Editor-UI oracle (Layer 4) — `editor_ui_oracle_test`.
//!
//! This is the editor-side sibling of the RUNTIME oracle
//! (`engine-rs/tests/oracle_regression_test.rs` ::
//! `golden_all_scenes_property_parity_report`). It closes the "the author chose
//! the pass condition" loophole that every hand-authored editor-parity test
//! has: instead of asserting Patina matches a value WE picked, it asserts
//! Patina's **served editor** reproduces what **real Godot 4.6.1's editor**
//! produced, captured ahead of time into `fixtures/editor_oracle/*.json`
//! (pat-crasr — DO NOT regenerate here; we only consume the JSON goldens).
//!
//! What it does, per captured scene (demo_2d, demo_3d, hierarchy, minimal_3d,
//! signals_complex, ui_menu):
//!   1. Boots the real editor server (`EditorServerHandle::start`) and loads the
//!      same `fixtures/scenes/<scene>.tscn` over HTTP via `POST /api/scene/load`
//!      — i.e. it drives the ASSEMBLED, running app, not a lib model. This is
//!      the app-level gate the planner methodology requires (a lib/model test is
//!      necessary but NOT sufficient; see .orchestrator/planner.toml preamble).
//!   2. Walks `GET /api/scene` and `GET /api/node/<id>` to read Patina's
//!      editor-visible inspector property list + node class per node.
//!   3. Diffs that against the Godot golden inspector
//!      (`inspector_<scene>.json`: per-node list of PROPERTY_USAGE_EDITOR
//!      properties) and scene tree (`scene_tree_<scene>.json`).
//!   4. Aggregates an overall editor-parity %, writes a readable per-scene
//!      report plus the machine-readable `OVERALL <total> <matched> <pct>%` row
//!      to `fixtures/editor_oracle/parity_report.txt`. That row is what the
//!      orchestrator planner's `EditorParityAbove` condition reads
//!      (`apps/orchestrator/.../planner.rs::read_editor_parity`), via the regex
//!      `^\s*OVERALL\s+(\d+)\s+(\d+)\s+(\d+(?:\.\d+)?)%\s*$`.
//!
//! ── MATCH CRITERION (documented, deliberately strict + honest) ──────────────
//! Scope: the "scene-content" nodes only — every Godot golden node at or below
//! the scene root (the `scene_tree` golden's `tree.path`, e.g. `/root/World`).
//! The editor host `/root` Window node is OUT of scope: it is the editor's own
//! viewport window, not part of any saved scene (the scene_tree golden itself
//! starts at the scene root, excluding `/root`). Its property count is reported
//! separately for transparency, never folded into the parity number.
//!
//! For each in-scope Godot node, matched to a Patina node by EXACT path:
//!   * `total`  += every editor-visible (PROPERTY_USAGE_EDITOR) property Godot
//!                 lists on that node — the full class schema Godot's inspector
//!                 shows.
//!   * a property counts as MATCHED (the OVERALL number) when Patina's served
//!     editor exposes a property of the SAME NAME on that node AND the
//!     normalized value is EQUAL (float tolerance 0.01, the same tolerance the
//!     runtime oracle uses). Name-only coverage is reported as a secondary
//!     column so the two effects (missing property vs wrong value) stay legible.
//!
//! Why the number is expected to be LOW, and why that is the correct result:
//! Godot's inspector enumerates a node's FULL class property schema (~27 for a
//! Node2D), whereas Patina's `/api/node/<id>` currently surfaces only the
//! properties actually stored on the node (those set in the `.tscn` plus engine
//! defaults). So most Godot editor properties have no Patina counterpart yet.
//! That gap is exactly what this oracle exists to quantify — the test reports
//! the REAL measured number and intentionally does NOT hard-fail on low parity
//! (raising it is the planner threshold's job, not this test's). Inflating or
//! cherry-picking the number would re-open the loophole this layer closes.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

// ───────────────────────── HTTP harness ─────────────────────────
// Copied from crates/gdeditor/tests/editor_parity_test.rs so this file is
// self-contained. The server sends `Connection: close` on every response, so
// `read_to_end` returns promptly at EOF (no keep-alive stall).

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Boots an editor server with a throwaway tree. Each scene under test is then
/// loaded into this same running server via `POST /api/scene/load`, so the test
/// exercises the real load → enumerate path of the assembled app.
fn start_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let placeholder = Node::new("Main", "Node2D");
    tree.add_child(root, placeholder).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request_str(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

// ───────────────────────── Fixture paths ─────────────────────────

fn editor_oracle_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/editor_oracle")
}

fn scenes_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/scenes")
}

/// The `res://` project root (pat-ghfv0.40) — `engine-rs/fixtures/`, which
/// contains `project.godot` and `scenes/`. Opening it lets the served editor
/// resolve `res://scenes/<scene>.tscn` and emit SubResource `resource_path`s that
/// match the Godot goldens (captured with a project root).
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// The scenes with captured Godot editor goldens (pat-crasr). Expanded in the
/// fill phase to every captured scene so the oracle measures the full class set
/// (CSG, lights, physics, UI, …), giving per-class evidence for each schema bead.
const ORACLE_SCENES: &[&str] = &[
    "animated_scene_3d",
    "csg_composition",
    "demo_2d",
    "demo_3d",
    "foggy_terrain_3d",
    "hierarchy",
    "hierarchy_3d",
    "indoor_3d",
    "minimal_3d",
    "multi_light_3d",
    "outdoor_3d",
    "physics_3d_playground",
    "platformer",
    "render_test_camera",
    "render_test_simple",
    "render_test_sprite",
    "signals_complex",
    "space_shooter",
    "spotlight_gallery_3d",
    "test_scripts",
    "tilemap_demo",
    "ui_menu",
    "vehicle_3d",
];

fn load_json(path: &std::path::Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read golden {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("invalid JSON golden {}: {e}", path.display()))
}

// ─────────────────── Value normalization / comparison ───────────────────

const FLOAT_TOLERANCE: f64 = 0.01;

/// Parses the numeric components of a vector/color value, accepting every shape
/// that appears across the two encodings:
///   * Godot golden object: `{"x":..,"y":..[,"z":..]}` or `{"r":,"g":,"b":,"a":}`
///   * Godot golden string:  `"(64, 64)"` (Vector2i etc.)
///   * Patina array:         `[x, y]` / `[r, g, b, a]` (from `gdvariant::to_json`)
/// Components are emitted in canonical order (x,y,z,w / r,g,b,a) so both sides
/// line up positionally.
fn parse_components(value: &Value) -> Option<Vec<f64>> {
    if let Some(obj) = value.as_object() {
        if obj.contains_key("x") {
            let out: Vec<f64> = ["x", "y", "z", "w"]
                .iter()
                .filter_map(|k| obj.get(*k).and_then(|v| v.as_f64()))
                .collect();
            return (!out.is_empty()).then_some(out);
        }
        if obj.contains_key("r") {
            let out: Vec<f64> = ["r", "g", "b", "a"]
                .iter()
                .filter_map(|k| obj.get(*k).and_then(|v| v.as_f64()))
                .collect();
            return (!out.is_empty()).then_some(out);
        }
        return None;
    }
    if let Some(arr) = value.as_array() {
        let mut out = Vec::with_capacity(arr.len());
        for e in arr {
            out.push(e.as_f64()?);
        }
        return Some(out);
    }
    if let Some(s) = value.as_str() {
        let inner = s.trim().trim_start_matches('(').trim_end_matches(')');
        let mut out = Vec::new();
        for part in inner.split(',') {
            out.push(part.trim().parse::<f64>().ok()?);
        }
        return (!out.is_empty()).then_some(out);
    }
    None
}

/// Extracts every numeric token from a string in order — e.g. Godot's AABB
/// display form `"[P: (0, 0, 0), S: (0, 0, 0)]"` → `[0,0,0,0,0,0]`. Used to
/// compare Godot's stringified compound values against Patina's structured ones.
fn scan_numbers(s: &str) -> Vec<f64> {
    s.split(|c: char| {
        !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')
    })
    .filter_map(|t| t.parse::<f64>().ok())
    .collect()
}

/// Flattens a rectangle-like value (AABB / Rect2) to an ordered numeric array,
/// accepting Godot's string display form, Godot's `{position,size}` object form,
/// and Patina's `{position:[..],size:[..]}` array form — the same value in three
/// encodings.
fn flatten_rectlike(value: &Value) -> Option<Vec<f64>> {
    if let Some(s) = value.as_str() {
        let n = scan_numbers(s);
        return (!n.is_empty()).then_some(n);
    }
    if let Some(obj) = value.as_object() {
        if let (Some(p), Some(sz)) = (obj.get("position"), obj.get("size")) {
            let mut out = parse_components(p)?;
            out.extend(parse_components(sz)?);
            return Some(out);
        }
    }
    None
}

/// Flattens a packed numeric array to a `Vec<f64>`, accepting Godot's stringified
/// form (`"[]"`, `"[1, 2, 3]"`) and Patina's JSON-array form (`[1,2,3]`).
fn flatten_packed(value: &Value) -> Option<Vec<f64>> {
    if let Some(s) = value.as_str() {
        return Some(scan_numbers(s));
    }
    if let Some(arr) = value.as_array() {
        return arr.iter().map(|e| e.as_f64()).collect();
    }
    None
}

/// Canonicalizes a value to a comparable form keyed by its type name. Vector,
/// color, rectangle, and packed-array types collapse to a numeric `Array` (so
/// the same value compares equal across Godot's string/object encodings and
/// Patina's array encoding); every other type passes through unchanged (scalars,
/// bools, and strings already compare directly, and exotic types fall back to
/// structural equality — honest, never a forced match).
fn canon(type_name: &str, value: &Value) -> Value {
    if type_name.starts_with("Packed") {
        // Godot stringifies packed arrays (`"[]"`); Patina emits JSON arrays.
        return match flatten_packed(value) {
            Some(c) => Value::Array(c.into_iter().map(|f| serde_json::json!(f)).collect()),
            None => value.clone(),
        };
    }
    match type_name {
        "Vector2" | "Vector2i" | "Vector3" | "Vector3i" | "Color" => {
            match parse_components(value) {
                Some(c) => Value::Array(c.into_iter().map(|f| serde_json::json!(f)).collect()),
                None => value.clone(),
            }
        }
        "AABB" | "Rect2" => match flatten_rectlike(value) {
            Some(c) => Value::Array(c.into_iter().map(|f| serde_json::json!(f)).collect()),
            None => value.clone(),
        },
        _ => value.clone(),
    }
}

/// Canonicalizes a Patina property value, which `gdvariant::to_json` emits as a
/// tagged `{"type": T, "value": V}` object.
fn canon_patina(variant_json: &Value) -> Value {
    let t = variant_json
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("");
    let v = variant_json.get("value").cloned().unwrap_or(Value::Null);
    canon(t, &v)
}

fn values_match(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            (x.as_f64().unwrap_or(f64::NAN) - y.as_f64().unwrap_or(f64::NAN)).abs()
                <= FLOAT_TOLERANCE
        }
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| values_match(p, q))
        }
        (Value::Null, Value::Null) => true,
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).map_or(false, |w| values_match(v, w)))
        }
        _ => false,
    }
}

// ─────────────────── Patina-side scene enumeration ───────────────────

/// One inspector property read back from the served editor. The deeper oracle
/// (pat-ghfv0.36) compares all of these against the Godot golden, not just value.
struct PatinaProp {
    value: Value,
    type_name: String,
    hint: i64,
    hint_string: String,
    is_default: bool,
}

/// A node read back from the served editor: its class plus its inspector
/// properties keyed by property name.
struct PatinaNode {
    class: String,
    props: BTreeMap<String, PatinaProp>,
}

/// Loads `scene` into the running editor and reads back every node's class +
/// inspector properties, keyed by absolute node path. Returns `None` if the
/// served editor could not even open the scene (itself a real, reported gap).
fn enumerate_patina_scene(port: u16, scene: &str) -> Option<BTreeMap<String, PatinaNode>> {
    // pat-ghfv0.40: root the served editor at the fixtures project so it can
    // resolve the scene's res:// path and emit SubResource resource_paths that
    // match the goldens (Godot captured them WITH a project root). Loads still
    // use the absolute path; the project root only supplies the res:// mapping.
    let open_body = serde_json::json!({ "path": fixtures_dir().to_str().unwrap() }).to_string();
    let _ = http_post(port, "/api/project/open", &open_body);

    let tscn = scenes_dir().join(format!("{scene}.tscn"));
    let body = serde_json::json!({ "path": tscn.to_str().unwrap() }).to_string();
    let resp = http_post(port, "/api/scene/load", &body);
    if !resp.contains("200 OK") {
        return None;
    }

    let scene_resp = http_get(port, "/api/scene");
    let scene_val: Value = serde_json::from_str(extract_body(&scene_resp)).ok()?;

    let mut nodes: BTreeMap<String, PatinaNode> = BTreeMap::new();
    let mut stack = vec![scene_val.get("nodes").cloned().unwrap_or(Value::Null)];
    while let Some(node) = stack.pop() {
        if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
            for c in children {
                stack.push(c.clone());
            }
        }
        let id = match node.get("id").and_then(|v| v.as_u64()) {
            Some(id) => id,
            None => continue,
        };
        let path = node
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let node_resp = http_get(port, &format!("/api/node/{id}"));
        let nv: Value = match serde_json::from_str(extract_body(&node_resp)) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let class = nv
            .get("class")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut props = BTreeMap::new();
        if let Some(arr) = nv.get("properties").and_then(|v| v.as_array()) {
            for p in arr {
                if let Some(name) = p.get("name").and_then(|n| n.as_str()) {
                    props.insert(
                        name.to_string(),
                        PatinaProp {
                            value: p.get("value").cloned().unwrap_or(Value::Null),
                            type_name: p
                                .get("type_name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            hint: p.get("hint").and_then(|v| v.as_i64()).unwrap_or(-1),
                            hint_string: p
                                .get("hint_string")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            is_default: p
                                .get("is_default")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                        },
                    );
                }
            }
        }
        nodes.insert(path, PatinaNode { class, props });
    }
    Some(nodes)
}

/// Per-class parity tally (aggregated across all scenes for the per-class table
/// that the schema beads close against). `value_matched` is the FULL match under
/// the deeper criterion (name+value+type_name+hint+hint_string+is_default).
#[derive(Default, Clone)]
struct ClassAgg {
    nodes: usize,
    total: usize,
    value_matched: usize,
    name_matched: usize,
}

/// Per-field disagreement counts among name-matched props (pat-ghfv0.36) — the
/// gap characterization: which field dimension diverges from Godot most.
#[derive(Default, Clone)]
struct FieldMiss {
    value: usize,
    type_name: usize,
    hint: usize,
    hint_string: usize,
    is_default: usize,
}

/// Aggregated comparison result for one scene.
struct SceneParity {
    scene: String,
    prop_total: usize,
    prop_value_matched: usize,
    prop_name_matched: usize,
    tree_total: usize,
    tree_matched: usize,
    out_of_scope_props: usize,
    loaded: bool,
    /// class name → tally, for this scene.
    per_class: BTreeMap<String, ClassAgg>,
    /// (class, prop name, failed-fields) for every prop that name-matched but
    /// failed the deeper criterion — residual detail.
    unmatched: Vec<(String, String, String)>,
    /// Per-field disagreement counts (among name-matched props).
    field_miss: FieldMiss,
}

fn compare_scene(port: u16, scene: &str) -> SceneParity {
    let inspector = load_json(&editor_oracle_dir().join(format!("inspector_{scene}.json")));
    let scene_tree = load_json(&editor_oracle_dir().join(format!("scene_tree_{scene}.json")));
    let scene_root_path = scene_tree
        .get("tree")
        .and_then(|t| t.get("path"))
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_string();
    let scope_prefix = format!("{scene_root_path}/");

    let patina = enumerate_patina_scene(port, scene);
    let loaded = patina.is_some();
    let patina = patina.unwrap_or_default();

    let mut p = SceneParity {
        scene: scene.to_string(),
        prop_total: 0,
        prop_value_matched: 0,
        prop_name_matched: 0,
        tree_total: 0,
        tree_matched: 0,
        out_of_scope_props: 0,
        loaded,
        per_class: BTreeMap::new(),
        unmatched: Vec::new(),
        field_miss: FieldMiss::default(),
    };

    let nodes = match inspector.get("nodes").and_then(|n| n.as_array()) {
        Some(n) => n,
        None => return p,
    };

    for gn in nodes {
        let gpath = gn.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let gprops = gn
            .get("inspector_properties")
            .and_then(|v| v.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]);

        let in_scope = gpath == scene_root_path || gpath.starts_with(&scope_prefix);
        if !in_scope {
            // Editor host `/root` Window (and anything else above the scene
            // root): reported, never counted in the parity number.
            p.out_of_scope_props += gprops.len();
            continue;
        }

        // Scene-tree structure: node present at this path with the same class.
        p.tree_total += 1;
        let gclass = gn.get("class").and_then(|v| v.as_str()).unwrap_or("");
        let patina_node = patina.get(gpath);
        if let Some(pn) = patina_node {
            if pn.class == gclass {
                p.tree_matched += 1;
            }
        }

        let agg = p.per_class.entry(gclass.to_string()).or_default();
        agg.nodes += 1;

        // Inspector properties: total is driven by Godot's editor schema.
        // Deeper criterion (pat-ghfv0.36): a prop MATCHES only when Patina agrees
        // with the golden on value AND type_name AND hint AND hint_string AND
        // is_default. Each failing dimension is tallied for characterization.
        for gp in gprops {
            p.prop_total += 1;
            agg.total += 1;
            let pname = gp.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let gtype = gp.get("type_name").and_then(|v| v.as_str()).unwrap_or("");
            let ghint = gp.get("hint").and_then(|v| v.as_i64()).unwrap_or(-1);
            let ghint_str = gp.get("hint_string").and_then(|v| v.as_str()).unwrap_or("");
            let gis_default = gp
                .get("is_default")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let gval = canon(gtype, gp.get("value").unwrap_or(&Value::Null));

            if let Some(pn) = patina_node {
                if let Some(pv) = pn.props.get(pname) {
                    p.prop_name_matched += 1;
                    agg.name_matched += 1;

                    let value_ok = values_match(&gval, &canon_patina(&pv.value));
                    let type_ok = pv.type_name == gtype;
                    let hint_ok = pv.hint == ghint;
                    let hint_str_ok = pv.hint_string == ghint_str;
                    let default_ok = pv.is_default == gis_default;

                    let mut failed: Vec<&str> = Vec::new();
                    if !value_ok {
                        p.field_miss.value += 1;
                        failed.push("value");
                    }
                    if !type_ok {
                        p.field_miss.type_name += 1;
                        failed.push("type_name");
                    }
                    if !hint_ok {
                        p.field_miss.hint += 1;
                        failed.push("hint");
                    }
                    if !hint_str_ok {
                        p.field_miss.hint_string += 1;
                        failed.push("hint_string");
                    }
                    if !default_ok {
                        p.field_miss.is_default += 1;
                        failed.push("is_default");
                    }

                    if failed.is_empty() {
                        p.prop_value_matched += 1;
                        agg.value_matched += 1;
                    } else {
                        p.unmatched
                            .push((gclass.to_string(), pname.to_string(), failed.join("+")));
                    }
                }
            }
        }
    }

    p
}

// ─────────────────── Report formatting / parsing ───────────────────

fn pct(matched: usize, total: usize) -> f64 {
    if total > 0 {
        (matched as f64 / total as f64) * 100.0
    } else {
        0.0
    }
}

/// The exact regex the orchestrator planner uses to read this report
/// (planner.rs::parse_editor_parity_output). Used here to prove our own output
/// is parsable by the consumer, not just by us.
fn planner_overall_regex() -> Regex {
    Regex::new(r"(?m)^\s*OVERALL\s+(\d+)\s+(\d+)\s+(\d+(?:\.\d+)?)%\s*$").unwrap()
}

// ───────────────────────────── Tests ─────────────────────────────

/// The oracle itself: boot the served editor, replay every captured scene,
/// diff against the Godot goldens, write the report, and assert it is present,
/// parsable by the planner's regex, and non-empty. It deliberately does NOT
/// assert a parity floor — a low % is a valid, honest measurement of the editor
/// inspector gap and is the planner threshold's concern, not this test's.
#[test]
fn editor_ui_oracle_report() {
    // Each scene gets its OWN fresh server instance. Loading many scenes
    // sequentially into a single shared server wedges it after a handful of
    // loads (state/resource accumulation), which would falsely zero-out every
    // later scene; isolating per scene measures each load honestly.
    let mut results: Vec<SceneParity> = Vec::new();
    for scene in ORACLE_SCENES {
        let (handle, port) = start_server();
        results.push(compare_scene(port, scene));
        handle.stop();
    }

    let grand_total: usize = results.iter().map(|r| r.prop_total).sum();
    let grand_matched: usize = results.iter().map(|r| r.prop_value_matched).sum();
    let grand_name: usize = results.iter().map(|r| r.prop_name_matched).sum();
    let grand_tree_total: usize = results.iter().map(|r| r.tree_total).sum();
    let grand_tree_matched: usize = results.iter().map(|r| r.tree_matched).sum();
    let grand_oos: usize = results.iter().map(|r| r.out_of_scope_props).sum();
    let overall = pct(grand_matched, grand_total);

    // Merge per-class tallies across every scene (drives the per-class table the
    // schema beads close against).
    let mut by_class: BTreeMap<String, ClassAgg> = BTreeMap::new();
    for r in &results {
        for (cls, a) in &r.per_class {
            let e = by_class.entry(cls.clone()).or_default();
            e.nodes += a.nodes;
            e.total += a.total;
            e.value_matched += a.value_matched;
            e.name_matched += a.name_matched;
        }
    }

    // Per-field disagreement totals (pat-ghfv0.36 gap characterization).
    let mut fm = FieldMiss::default();
    for r in &results {
        fm.value += r.field_miss.value;
        fm.type_name += r.field_miss.type_name;
        fm.hint += r.field_miss.hint;
        fm.hint_string += r.field_miss.hint_string;
        fm.is_default += r.field_miss.is_default;
    }
    // Which (class, prop, failed-fields) mismatched, aggregated with a count.
    let mut miss_by_prop: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for r in &results {
        for u in &r.unmatched {
            *miss_by_prop.entry(u.clone()).or_default() += 1;
        }
    }

    // ── Build the report ────────────────────────────────────────────────
    let mut report = String::new();
    report.push_str("=== Patina Editor UI Oracle — inspector parity vs Godot 4.6.1 golden ===\n");
    report
        .push_str("Source of truth: fixtures/editor_oracle/{inspector,scene_tree}_<scene>.json\n");
    report.push_str("(captured from real Godot 4.6.1 — pat-crasr). Generated by\n");
    report.push_str("gdeditor/tests/editor_ui_oracle_test.rs::editor_ui_oracle_report.\n\n");
    report
        .push_str("MATCHED = Patina's served editor exposes a property of the same name on the\n");
    report.push_str("same-path node AND its normalized value equals Godot's (float tol 0.01).\n");
    report.push_str("TOTAL   = every PROPERTY_USAGE_EDITOR property Godot shows on the scene's\n");
    report.push_str("          content nodes (editor host /root Window excluded from parity).\n");
    report.push_str(&format!(
        "Excluded (editor host /root Window etc.): {grand_oos} Godot properties out of scope.\n\n"
    ));
    report.push_str(&format!(
        "{:<16} {:>7} {:>7} {:>8}  {:>9}  {:>10}\n",
        "Scene", "Total", "Match", "Parity", "Name-cov", "Tree"
    ));
    report.push_str(&"-".repeat(72));
    report.push('\n');
    for r in &results {
        let note = if r.loaded { "" } else { "  [LOAD FAILED]" };
        report.push_str(&format!(
            "{:<16} {:>7} {:>7} {:>7.1}%  {:>8.1}%  {:>4}/{:<4}{}\n",
            r.scene,
            r.prop_total,
            r.prop_value_matched,
            pct(r.prop_value_matched, r.prop_total),
            pct(r.prop_name_matched, r.prop_total),
            r.tree_matched,
            r.tree_total,
            note,
        ));
    }
    report.push_str(&"-".repeat(72));
    report.push('\n');
    report.push_str(&format!(
        "Secondary: name-coverage {:.1}% ({}/{}), scene-tree structure {:.1}% ({}/{}).\n",
        pct(grand_name, grand_total),
        grand_name,
        grand_total,
        pct(grand_tree_matched, grand_tree_total),
        grand_tree_matched,
        grand_tree_total,
    ));

    // ── Per-class breakdown (evidence for the per-class schema beads) ────
    // Sorted by property total descending. `Parity` = value-matched / total;
    // `Name` = name-matched / total (schema exposes the property but its value
    // may differ from Godot's override — the honest residual for each class).
    report.push('\n');
    report.push_str("=== Per-class inspector parity (all scenes) ===\n");
    report.push_str(&format!(
        "{:<20} {:>5} {:>7} {:>7} {:>8}  {:>8}\n",
        "Class", "Nodes", "Total", "Match", "Parity", "Name"
    ));
    report.push_str(&"-".repeat(60));
    report.push('\n');
    let mut class_rows: Vec<(&String, &ClassAgg)> = by_class.iter().collect();
    class_rows.sort_by(|a, b| b.1.total.cmp(&a.1.total).then(a.0.cmp(b.0)));
    for (cls, a) in &class_rows {
        report.push_str(&format!(
            "{:<20} {:>5} {:>7} {:>7} {:>7.1}%  {:>7.1}%\n",
            cls,
            a.nodes,
            a.total,
            a.value_matched,
            pct(a.value_matched, a.total),
            pct(a.name_matched, a.total),
        ));
    }
    report.push_str(&"-".repeat(60));
    report.push('\n');
    // Machine-readable per-class rows (consumed by per-class bead verification):
    // `CLASS <name> <total> <matched> <pct>%`.
    for (cls, a) in &class_rows {
        report.push_str(&format!(
            "CLASS {} {} {} {:.1}%\n",
            cls,
            a.total,
            a.value_matched,
            pct(a.value_matched, a.total),
        ));
    }
    report.push('\n');

    // ── Deeper-criterion field disagreement (pat-ghfv0.36) ──────────────
    // MATCHED now requires name+value+type_name+hint+hint_string+is_default all
    // agree. This breaks down WHICH field dimension diverges from Godot, among
    // the name-matched props, so the gap is characterizable (→ next beads).
    report.push_str("=== Deeper criterion: per-field disagreements (name-matched props) ===\n");
    report.push_str(&format!(
        "value:{}  type_name:{}  hint:{}  hint_string:{}  is_default:{}\n",
        fm.value, fm.type_name, fm.hint, fm.hint_string, fm.is_default
    ));
    // Top mismatching (class, prop → failed fields), by frequency.
    let mut miss_rows: Vec<(&(String, String, String), &usize)> = miss_by_prop.iter().collect();
    miss_rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for ((cls, prop, fields), n) in miss_rows.iter().take(20) {
        report.push_str(&format!("  {cls} {prop} [{fields}] x{n}\n"));
    }
    report.push('\n');

    // Machine-readable row consumed by the planner's EditorParityAbove condition.
    report.push_str(&format!(
        "OVERALL {grand_total} {grand_matched} {overall:.1}%\n"
    ));

    let report_path = editor_oracle_dir().join("parity_report.txt");
    std::fs::write(&report_path, &report)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", report_path.display()));

    // Residual diagnostic (NOT committed): every remaining value-mismatch, keyed
    // by (class, prop, type) with a count — evidence for which props keep a class
    // below 100% and why. Written to the temp dir so it never pollutes fixtures.
    let mut residual: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for r in &results {
        for u in &r.unmatched {
            *residual.entry(u.clone()).or_default() += 1;
        }
    }
    let mut residual_rows: Vec<((String, String, String), usize)> = residual.into_iter().collect();
    residual_rows.sort_by(|a, b| a.0 .0.cmp(&b.0 .0).then(b.1.cmp(&a.1)));
    let mut residual_txt = String::from("class\tprop\ttype\tcount\n");
    for ((cls, prop, ty), n) in &residual_rows {
        residual_txt.push_str(&format!("{cls}\t{prop}\t{ty}\t{n}\n"));
    }
    let residual_path = std::env::temp_dir().join("patina_parity_unmatched.txt");
    let _ = std::fs::write(&residual_path, &residual_txt);
    eprintln!(
        "residual (unmatched) props written to {}",
        residual_path.display()
    );

    eprintln!("\n{report}");

    // ── Assertions: the report exists, is parsable by the EXACT planner regex,
    //    and is non-empty. NOT a parity floor (low parity is a valid result). ──
    let reread = std::fs::read_to_string(&report_path).expect("re-read parity_report.txt");
    let cap = planner_overall_regex()
        .captures(&reread)
        .expect("report must contain a planner-parsable `OVERALL <total> <matched> <pct>%` row");
    let parsed_total: usize = cap[1].parse().unwrap();
    let parsed_matched: usize = cap[2].parse().unwrap();

    assert_eq!(parsed_total, grand_total, "OVERALL total round-trips");
    assert_eq!(parsed_matched, grand_matched, "OVERALL matched round-trips");
    assert!(
        grand_total > 0,
        "oracle must compare at least one Godot editor property (goldens present + scenes load)"
    );
    assert!(
        parsed_matched <= parsed_total,
        "matched can never exceed total"
    );
}

/// Every captured scene must have both goldens, and they must parse with the
/// expected shape. Guards against a half-captured fixture set silently shrinking
/// the oracle's coverage.
#[test]
fn all_oracle_goldens_present_and_well_formed() {
    for scene in ORACLE_SCENES {
        let inspector = load_json(&editor_oracle_dir().join(format!("inspector_{scene}.json")));
        let nodes = inspector
            .get("nodes")
            .and_then(|n| n.as_array())
            .unwrap_or_else(|| panic!("inspector_{scene}.json has a `nodes` array"));
        assert!(!nodes.is_empty(), "inspector_{scene}.json has nodes");
        // Each node has a path, class, and an inspector_properties list.
        for n in nodes {
            assert!(n.get("path").is_some(), "{scene}: node has path");
            assert!(n.get("class").is_some(), "{scene}: node has class");
            assert!(
                n.get("inspector_properties")
                    .and_then(|p| p.as_array())
                    .is_some(),
                "{scene}: node has inspector_properties array"
            );
        }
        let tree = load_json(&editor_oracle_dir().join(format!("scene_tree_{scene}.json")));
        assert!(
            tree.get("tree").and_then(|t| t.get("path")).is_some(),
            "scene_tree_{scene}.json has tree.path (the scene root)"
        );
    }
}

// ── Unit tests for the normalization + comparison logic ──

#[test]
fn vector2_object_matches_patina_array() {
    // Godot golden encodes Vector2 as {x,y}; Patina as [x,y].
    let g = canon("Vector2", &serde_json::json!({"x": 100.0, "y": 200.0}));
    let p = canon_patina(&serde_json::json!({"type": "Vector2", "value": [100.0, 200.0]}));
    assert!(values_match(&g, &p));
}

#[test]
fn vector2i_string_form_parses() {
    // Vector2i appears as a string "(64, 64)" in the golden.
    let g = canon("Vector2i", &serde_json::json!("(64, 64)"));
    let p = canon_patina(&serde_json::json!({"type": "Vector2", "value": [64.0, 64.0]}));
    assert!(values_match(&g, &p));
}

#[test]
fn color_object_matches_patina_array_in_rgba_order() {
    let g = canon(
        "Color",
        &serde_json::json!({"r": 1.0, "g": 0.5, "b": 0.0, "a": 1.0}),
    );
    let p = canon_patina(&serde_json::json!({"type": "Color", "value": [1.0, 0.5, 0.0, 1.0]}));
    assert!(values_match(&g, &p));
}

#[test]
fn scalar_and_bool_compare_directly() {
    assert!(values_match(
        &canon("int", &serde_json::json!(7)),
        &canon_patina(&serde_json::json!({"type": "Int", "value": 7}))
    ));
    assert!(values_match(
        &canon("bool", &serde_json::json!(true)),
        &canon_patina(&serde_json::json!({"type": "Bool", "value": true}))
    ));
    // Different values must NOT match.
    assert!(!values_match(
        &canon("int", &serde_json::json!(7)),
        &canon_patina(&serde_json::json!({"type": "Int", "value": 8}))
    ));
}

#[test]
fn float_within_tolerance_matches() {
    assert!(values_match(
        &canon("float", &serde_json::json!(1.0)),
        &canon_patina(&serde_json::json!({"type": "Float", "value": 1.005}))
    ));
    assert!(!values_match(
        &canon("float", &serde_json::json!(1.0)),
        &canon_patina(&serde_json::json!({"type": "Float", "value": 1.5}))
    ));
}

#[test]
fn overall_row_is_parsable_by_planner_regex() {
    // Exactly the format the report emits must satisfy the planner's regex.
    let sample = "header line\nOVERALL 350 14 4.0%\n";
    let cap = planner_overall_regex()
        .captures(sample)
        .expect("planner regex matches our OVERALL row");
    assert_eq!(&cap[1], "350");
    assert_eq!(&cap[2], "14");
    assert_eq!(&cap[3], "4.0");
}
