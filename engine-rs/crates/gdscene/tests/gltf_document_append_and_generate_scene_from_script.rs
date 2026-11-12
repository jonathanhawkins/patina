//! pat-s3rrv.49: the runtime glTF API a script actually calls.
//!
//! Godot's runtime load path is
//! `var d = GLTFDocument.new(); var s = GLTFState.new()` →
//! `d.append_from_file(path, s)` → `d.generate_scene(s)`
//! (upstream/godot/modules/gltf/gltf_document.cpp). Patina had the whole family
//! ClassDB-registered — GLTFDocument's six properties and 23 method names,
//! GLTFState, FBXDocument/FBXState — and dispatched **not one** of them, so
//! `GLTFDocument.new()` raised a TypeError. A registered, introspectable class
//! family that no script could reach, sitting next to a working parser
//! (`gdresource::gltf_import`) with nothing connecting the two.
//!
//! Every assertion here runs real GDScript against a real `.glb` through the
//! live `MainLoop`, so "the method parses but reads nothing" fails.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Serializes the tests in this binary that touch the res:// path roots (each test roots res:// at its own temp project) (pat-hukbz).
///
/// That state is process-global. Nextest gives each test its own process, but
/// plain `cargo test` runs the tests as threads in ONE process, where they
/// overwrote each other's state mid-test. Bind the guard for the whole test.
/// Poison-tolerant, so one failing test does not fail the rest.
#[must_use = "bind it for the whole test: `let _guard = serial();`"]
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The gdresource glTF fixture, embedded so this test does not depend on a
/// path between two crates' directories at runtime.
const QUAD_GLB: &[u8] = include_bytes!("../../gdresource/fixtures/quad.glb");

/// Loads the fixture through the documented script path and records what came
/// back. Written to survive Patina's parser: no multi-line literals.
const LOADER: &str = "\
extends Node
var err = -1
var root_name = \"\"
var child_count = -1
var mesh_children = 0
var node_count = -1
var appended_meshes = -1
func _ready():
    var d = GLTFDocument.new()
    var s = GLTFState.new()
    self.err = d.append_from_file(\"res://quad.glb\", s)
    self.appended_meshes = s.get_meshes().size()
    self.node_count = s.get_nodes().size()
    var n = d.generate_scene(s)
    add_child(n)
    self.root_name = n.get_name()
    self.child_count = n.get_child_count()
    for c in n.get_children():
        if c.get_class() == \"MeshInstance3D\":
            self.mesh_children += 1
";

fn script_prop(tree: &SceneTree, node: NodeId, prop: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

fn as_int(v: Variant) -> i64 {
    match v {
        Variant::Int(i) => i,
        other => panic!("expected Int, got {other:?}"),
    }
}

/// Writes the fixture into a temp project and points `res://` at it, so the
/// script's `append_from_file("res://quad.glb", …)` resolves the way it would
/// in a real game rather than through a relative path between crate dirs.
fn project_with_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("quad.glb"), QUAD_GLB).expect("write fixture");
    gdcore::file_access::set_path_roots(Some(dir.path().to_path_buf()), None);
    dir
}

/// **The acceptance test.**
#[test]
fn gltf_document_append_and_generate_scene_from_script() {
    let _guard = serial();
    // Held for the whole test: dropping it deletes the file `res://` points at.
    let _project: tempfile::TempDir = project_with_fixture();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let loader = tree.add_child(root, Node::new("Loader", "Node")).unwrap();
    tree.attach_script(
        loader,
        Box::new(GDScriptNodeInstance::from_source(LOADER, loader).expect("loader parses")),
    );
    let mut ml = MainLoop::new(tree);
    // ⚠️ `MainLoop::step` alone does NOT fire `_ready` for a script attached
    // after construction, so without this every assertion below would read its
    // initialiser and fail for the wrong reason. Same sequence as
    // `astar_from_gdscript.rs`.
    ml.tree_mut().process_script_ready(loader);
    ml.step(1.0 / 60.0);

    // (1) `append_from_file` returns a Godot Error code, and OK is 0. Before
    // this bead the script died at `GLTFDocument.new()` with a TypeError, so
    // `err` would still be its -1 initialiser.
    assert_eq!(
        as_int(script_prop(ml.tree(), loader, "err")),
        0,
        "append_from_file must return OK (0) for a valid .glb; -1 means the \
         script never got past GLTFDocument.new()",
    );

    // (2) The state answers from the PARSED document, not from defaults. The
    // fixture is a single quad: one mesh, one node.
    assert_eq!(
        as_int(script_prop(ml.tree(), loader, "appended_meshes")),
        1,
        "GLTFState.get_meshes() must report the fixture's real mesh count",
    );
    assert_eq!(
        as_int(script_prop(ml.tree(), loader, "node_count")),
        1,
        "GLTFState.get_nodes() must enumerate the fixture's real nodes",
    );

    // (3) `generate_scene` returns a live node that `add_child` accepted.
    assert_eq!(
        as_int(script_prop(ml.tree(), loader, "child_count")),
        1,
        "the generated root must carry one child per glTF root node",
    );
    assert_eq!(
        as_int(script_prop(ml.tree(), loader, "mesh_children")),
        1,
        "a glTF node that references a mesh must become a MeshInstance3D — a 0 \
         here means the tree was built out of bare Node3Ds and would render \
         nothing",
    );

    // (4) …and it is really in the tree, observed from the Rust side rather
    // than from the script that created it.
    let generated = ml
        .tree()
        .get_node(loader)
        .expect("loader node")
        .children()
        .first()
        .copied()
        .expect("generate_scene's root was added under the loader");
    assert_eq!(
        ml.tree()
            .get_node(generated)
            .map(|n| n.class_name().to_string()),
        Some("Node3D".to_string()),
        "generate_scene's root is a Node3D, as Godot's is",
    );
}

/// A bare `GLTFState` answers emptily instead of raising (pat-s3rrv.49).
///
/// Godot lets a script construct a state and query it before anything has been
/// appended. Raising there would make the common `if state.get_nodes().size()
/// == 0` guard impossible to write.
#[test]
fn a_fresh_gltf_state_answers_empty_rather_than_raising() {
    let _guard = serial();
    const PROBE: &str = "\
extends Node
var nodes = -1
var meshes = -1
var base = \"unset\"
func _ready():
    var s = GLTFState.new()
    self.nodes = s.get_nodes().size()
    self.meshes = s.get_meshes().size()
    self.base = s.get_base_path()
";
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = tree.add_child(root, Node::new("Probe", "Node")).unwrap();
    tree.attach_script(
        probe,
        Box::new(GDScriptNodeInstance::from_source(PROBE, probe).expect("probe parses")),
    );
    let mut ml = MainLoop::new(tree);
    // See the note in the acceptance test: `_ready` needs firing explicitly.
    ml.tree_mut().process_script_ready(probe);
    ml.step(1.0 / 60.0);

    assert_eq!(
        as_int(script_prop(ml.tree(), probe, "nodes")),
        0,
        "an empty GLTFState reports zero nodes rather than raising",
    );
    assert_eq!(
        as_int(script_prop(ml.tree(), probe, "meshes")),
        0,
        "…and zero meshes",
    );
    assert_eq!(
        script_prop(ml.tree(), probe, "base"),
        Variant::String(String::new()),
        "…and an empty base path, not the 'unset' initialiser",
    );
}

/// An unimplemented `GLTFState` getter RAISES rather than returning an empty
/// array (pat-s3rrv.49).
///
/// This is the bead's explicit instruction and it matters: a silent `[]` from
/// `get_skins()` is indistinguishable from "this asset has no skins", so a
/// script would branch the wrong way with no error to trace. The parser cannot
/// answer these yet, so it says so.
#[test]
fn an_unsupported_gltf_state_getter_raises() {
    let _guard = serial();
    const PROBE: &str = "\
extends Node
var reached_after = false
func _ready():
    var s = GLTFState.new()
    var skins = s.get_skins()
    self.reached_after = true
";
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = tree.add_child(root, Node::new("Probe", "Node")).unwrap();
    tree.attach_script(
        probe,
        Box::new(GDScriptNodeInstance::from_source(PROBE, probe).expect("probe parses")),
    );
    let mut ml = MainLoop::new(tree);
    // See the note in the acceptance test: `_ready` needs firing explicitly.
    ml.tree_mut().process_script_ready(probe);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_prop(ml.tree(), probe, "reached_after"),
        Variant::Bool(false),
        "get_skins() is not implemented, so the call must raise and abort \
         _ready — reaching the next line means it silently returned a default",
    );
}

/// `FBXDocument`/`FBXState` mint the same handles, because in Godot they
/// inherit GLTFDocument/GLTFState's whole surface (pat-s3rrv.49).
#[test]
fn fbx_document_and_state_share_the_gltf_surface() {
    let _guard = serial();
    let _project: tempfile::TempDir = project_with_fixture();

    const PROBE: &str = "\
extends Node
var err = -1
var meshes = -1
func _ready():
    var d = FBXDocument.new()
    var s = FBXState.new()
    self.err = d.append_from_file(\"res://quad.glb\", s)
    self.meshes = s.get_meshes().size()
";
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = tree.add_child(root, Node::new("Probe", "Node")).unwrap();
    tree.attach_script(
        probe,
        Box::new(GDScriptNodeInstance::from_source(PROBE, probe).expect("probe parses")),
    );
    let mut ml = MainLoop::new(tree);
    // See the note in the acceptance test: `_ready` needs firing explicitly.
    ml.tree_mut().process_script_ready(probe);
    ml.step(1.0 / 60.0);

    assert_eq!(
        as_int(script_prop(ml.tree(), probe, "err")),
        0,
        "FBXDocument inherits GLTFDocument's methods, so constructing one and \
         appending must work exactly as the glTF pair does",
    );
    assert_eq!(
        as_int(script_prop(ml.tree(), probe, "meshes")),
        1,
        "…and its state reads the same parsed document",
    );
}

/// A missing file returns an Error code instead of raising (pat-s3rrv.49).
///
/// Godot's `append_from_file` reports a bad asset through its return value so a
/// script can branch on it; only a wrong-typed argument is a program bug worth
/// raising for.
#[test]
fn a_missing_file_returns_an_error_code_not_a_raise() {
    let _guard = serial();
    let _project: tempfile::TempDir = project_with_fixture();

    const PROBE: &str = "\
extends Node
var err = -1
var reached_after = false
func _ready():
    var d = GLTFDocument.new()
    var s = GLTFState.new()
    self.err = d.append_from_file(\"res://does_not_exist.glb\", s)
    self.reached_after = true
";
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = tree.add_child(root, Node::new("Probe", "Node")).unwrap();
    tree.attach_script(
        probe,
        Box::new(GDScriptNodeInstance::from_source(PROBE, probe).expect("probe parses")),
    );
    let mut ml = MainLoop::new(tree);
    // See the note in the acceptance test: `_ready` needs firing explicitly.
    ml.tree_mut().process_script_ready(probe);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_prop(ml.tree(), probe, "reached_after"),
        Variant::Bool(true),
        "a missing file must NOT raise — the script has to keep running so it \
         can branch on the error code",
    );
    assert_ne!(
        as_int(script_prop(ml.tree(), probe, "err")),
        0,
        "…and the code must not be OK",
    );
}
