//! pat-s3rrv.197 — the named acceptance test: a GDScript-built `ImmediateMesh`
//! reaches the render path, and `SurfaceTool` is constructible from script.
//!
//! Godot ground truth: `doc/classes/ImmediateMesh.xml`, `SurfaceTool.xml`.
//!
//! Covers acceptance clauses 2, 4 and 6 end to end. Clauses 3 and 5 — attributes
//! carried into the surface arrays, and calls outside a begin/end pair erroring
//! — are pinned at the level where those rules are decided, in
//! `gdserver3d/tests/immediate_mesh_surface_semantics.rs`.
//!
//! ⚠️ THREE separate things must be wired for this to pass, and each fails
//! differently:
//!
//! 1. **ClassDB registration** — without it `ImmediateMesh.new()` is an unknown
//!    class.
//! 2. **An `Interpreter::classdb_instantiate` arm** — registration ALONE does
//!    not make a non-Node class constructible; the interpreter delegates every
//!    non-Node class to a bespoke `.new()` arm, so a registered-but-unwired
//!    class returns nothing.
//! 3. **`resolve_mesh` recognising the handle** — otherwise the assignment is
//!    accepted, an instance is produced, and it renders a DEFAULT CUBE. That is
//!    the dangerous one: the instance exists and has exactly 1 surface, so half
//!    of clause 2 passes and only the vertex count betrays it.

use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdserver3d::instance::Instance3D;
use gdvariant::Variant;

// ⚠️ `u32`, not `usize` — `RenderServer3DAdapter::new` takes u32.
const W: u32 = 64;
const H: u32 = 64;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Cam" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 4)
current = true

[node name="Mesh" type="MeshInstance3D" parent="."]
"#;

fn scene_with_script(src: &str) -> (MainLoop, NodeId) {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let mesh_node = tree
        .get_node_by_path("/root/Root/Mesh")
        .expect("MeshInstance3D instanced");

    let script = GDScriptNodeInstance::from_source(src, mesh_node).expect("script parses");
    tree.attach_script(mesh_node, Box::new(script));

    let mut ml = MainLoop::new(tree);
    // ⚠️ Explicit: `MainLoop::step` does not fire `_ready` for a script attached
    // after construction, and the whole test is what `_ready` builds.
    ml.tree_mut().process_script_ready(mesh_node);
    (ml, mesh_node)
}

/// The render instances a synced tree produces.
fn instances(ml: &MainLoop) -> Vec<Instance3D> {
    let mut adapter = RenderServer3DAdapter::new(W, H);
    let _ = adapter.render_frame(ml.tree());
    adapter.instances().to_vec()
}

fn script_int(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    match ml
        .tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
    {
        Some(Variant::Int(i)) => i,
        other => panic!("{name} should be an int, got {other:?}"),
    }
}

/// 🔑 **THE ACCEPTANCE.** A script builds an `ImmediateMesh` triangle, assigns
/// it to its `MeshInstance3D`, and the render path picks up 3 vertices.
#[test]
fn immediate_mesh_built_from_script_renders() {
    // Godot's Mesh.PRIMITIVE_TRIANGLES is 3 — NOT 2, and not our enum's
    // declaration order.
    let src = "\
extends MeshInstance3D
var surfaces = 0
func _ready():
    var m = ImmediateMesh.new()
    m.surface_begin(3)
    m.surface_add_vertex(Vector3(0, 0, 0))
    m.surface_add_vertex(Vector3(1, 0, 0))
    m.surface_add_vertex(Vector3(0, 1, 0))
    m.surface_end()
    self.surfaces = m.get_surface_count()
    self.mesh = m
";
    let (ml, node) = scene_with_script(src);

    assert_eq!(
        script_int(&ml, node, "surfaces"),
        1,
        "the script must see ONE finished surface. 0 means `surface_end` never \
         committed; a panic reaching here means `ImmediateMesh.new()` returned \
         nothing, which is ClassDB registration without an interpreter arm"
    );

    let list = instances(&ml);
    assert_eq!(list.len(), 1, "the MeshInstance3D must sync one instance");
    let mesh = list[0]
        .mesh
        .as_ref()
        .expect("the synced instance must carry geometry");

    // ⚠️ The vertex count is the assertion that matters. A default cube also
    // produces an instance with exactly ONE surface, so a surface-count-only
    // check passes against a mesh that ignored the script entirely.
    assert_eq!(
        mesh.vertices.len(),
        3,
        "the rendered mesh must be the script's TRIANGLE (3 vertices), not the \
         fallback cube `resolve_mesh` returns for an unrecognised `mesh` \
         property — a cube has 8 or more and would still report 1 surface"
    );
    assert!(
        mesh.surfaces.is_empty(),
        "one ImmediateMesh surface flattens into the primary vertex data, \
         leaving no extra Mesh3D surfaces"
    );
}

/// 🔑 Clause 4: `clear_surfaces()` leaves nothing to draw.
#[test]
fn clear_surfaces_leaves_nothing_to_render() {
    let src = "\
extends MeshInstance3D
var before = 0
var after = 0
func _ready():
    var m = ImmediateMesh.new()
    m.surface_begin(3)
    m.surface_add_vertex(Vector3(0, 0, 0))
    m.surface_add_vertex(Vector3(1, 0, 0))
    m.surface_add_vertex(Vector3(0, 1, 0))
    m.surface_end()
    self.before = m.get_surface_count()
    m.clear_surfaces()
    self.after = m.get_surface_count()
    self.mesh = m
";
    let (ml, node) = scene_with_script(src);

    // ⚠️ The `before` count is the positive control: without it, `after == 0`
    // would pass on a build where the surface was never created at all.
    assert_eq!(
        script_int(&ml, node, "before"),
        1,
        "a surface existed first"
    );
    assert_eq!(
        script_int(&ml, node, "after"),
        0,
        "and the clear emptied it"
    );
}

/// 🔑 Clause 6: `SurfaceTool` is constructible from GDScript and accumulates
/// vertices.
///
/// ⚠️ This is the class whose implementation already existed and was completely
/// unreachable — `gdserver3d::mesh::SurfaceTool` is a full procedural builder,
/// re-exported from its crate root, with no ClassDB entry and no script path.
/// Until this passes there is no way to build a mesh at runtime at all.
#[test]
fn surface_tool_is_constructible_from_script() {
    let src = "\
extends MeshInstance3D
var verts = 0
func _ready():
    var st = SurfaceTool.new()
    st.begin(3)
    st.add_vertex(Vector3(0, 0, 0))
    st.add_vertex(Vector3(1, 0, 0))
    st.add_vertex(Vector3(0, 1, 0))
    self.verts = st.get_vertex_count()
";
    let (ml, node) = scene_with_script(src);

    assert_eq!(
        script_int(&ml, node, "verts"),
        3,
        "SurfaceTool.new() must yield a live builder that counts its vertices; \
         0 means the handle was minted but `mesh_builder_call` never routed to \
         it, which is a constructor with no dispatch arm"
    );
}
