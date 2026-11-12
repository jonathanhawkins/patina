//! pat-s3rrv.201 acceptance: a GDScript builds a mesh with `SurfaceTool` and
//! the 3D render pass draws it.
//!
//! Godot ground truth: `upstream/godot/scene/resources/surface_tool.cpp`.
//!
//! ⚠️ The bug this proves fixed is NOT the one the bead describes. The bead says
//! nothing script-facing can reach SurfaceTool; in fact `SurfaceTool.new()`,
//! ClassDB registration and the interpreter handle arm all already existed
//! (pat-s3rrv.197). What did **not** exist was the last hop:
//! `scripting::surface_tool_geometry` and `immediate_mesh_geometry` were
//! written for `RenderServer3DAdapter::resolve_mesh` and **nothing ever called
//! them** — `render_server_3d.rs` contained zero `ObjectId` mentions.
//!
//! So `resolve_mesh` fell through `get_mesh_path`, which matches only
//! `Variant::String`, and every script-built mesh in the engine rendered a
//! **default cube**. That failure is deceptive rather than loud: the instance
//! exists and carries exactly one surface, so any check that counts surfaces
//! passes against geometry that ignored the script entirely. Only the vertex
//! count betrays it — which is why this test asserts that above all.
//!
//! ### Documented omission
//!
//! The acceptance also asks for per-vertex **colours** on the render instance.
//! `Mesh3D` has no colour channel, and adding one breaks every `Mesh3D { .. }`
//! literal without `..Default::default()` — including files another agent holds
//! in flight. The script below still *sets* colours, so the path is exercised
//! and `SurfaceTool` stores them; they simply cannot be asserted on the drawn
//! mesh until that channel exists.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscene::MainLoop;
use gdscript_interop::bindings::ScriptInstance;
use gdserver3d::instance::Instance3D;
use gdvariant::Variant;

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
    // `MainLoop::step` does not fire `_ready` for a script attached after
    // construction, and `_ready` is what builds the mesh.
    ml.tree_mut().process_script_ready(mesh_node);
    (ml, mesh_node)
}

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

/// A quad as two triangles in the XY plane, wound so the face normal is +Z.
/// UVs are distinct per corner so the mapping is checkable, and colours are set
/// so that path runs even though the drawn mesh cannot carry them yet.
const BUILD_QUAD: &str = r#"
extends MeshInstance3D
var built = 0
func _ready():
    var st = SurfaceTool.new()
    st.begin(3)
    st.set_color(Color(1, 0, 0, 1))
    st.set_uv(Vector2(0, 0))
    st.add_vertex(Vector3(0, 0, 0))
    st.set_uv(Vector2(1, 0))
    st.add_vertex(Vector3(1, 0, 0))
    st.set_uv(Vector2(1, 1))
    st.add_vertex(Vector3(1, 1, 0))
    st.set_uv(Vector2(0, 0))
    st.add_vertex(Vector3(0, 0, 0))
    st.set_uv(Vector2(1, 1))
    st.add_vertex(Vector3(1, 1, 0))
    st.set_uv(Vector2(0, 1))
    st.add_vertex(Vector3(0, 1, 0))
    st.generate_normals()
    self.built = st.get_vertex_count()
    self.mesh = st.commit()
"#;

/// 🔑 **THE ACCEPTANCE.**
#[test]
fn surface_tool_builds_mesh_from_script() {
    let (ml, node) = scene_with_script(BUILD_QUAD);

    assert_eq!(
        script_int(&ml, node, "built"),
        6,
        "the script must see six vertices. A panic reaching here means \
         `SurfaceTool.new()` returned nothing — ClassDB registration without an \
         interpreter arm"
    );

    let list = instances(&ml);
    assert_eq!(list.len(), 1, "the MeshInstance3D must sync one instance");
    let mesh = list[0]
        .mesh
        .as_ref()
        .expect("the synced instance must carry geometry");

    // ⚠️ THE assertion. A default cube also yields one instance with one
    // surface, so only the vertex count separates "drew the script's quad" from
    // "silently drew a cube".
    assert_eq!(
        mesh.vertices.len(),
        6,
        "the rendered mesh must be the script's two triangles, not the fallback cube"
    );

    // The authored UVs survived to the drawn mesh, in vertex order.
    let uvs: Vec<[f32; 2]> = mesh.uvs.clone();
    assert_eq!(
        uvs,
        vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ],
        "per-vertex UVs must reach the render instance"
    );

    // `generate_normals()` produced the face normal for a CCW quad in XY.
    assert_eq!(mesh.normals.len(), 6);
    for (i, n) in mesh.normals.iter().enumerate() {
        assert!(
            (n.z - 1.0).abs() < 1e-4 && n.x.abs() < 1e-4 && n.y.abs() < 1e-4,
            "vertex {i} should carry the generated +Z face normal, got {n:?}"
        );
    }
}

/// The control: without the handle path, this is what the failure looked like.
///
/// A `MeshInstance3D` with no script and no mesh still renders — the default
/// cube. Asserting its vertex count differs from six is what makes the
/// acceptance's `== 6` meaningful rather than coincidental.
#[test]
fn an_unscripted_mesh_instance_still_falls_back_to_the_placeholder() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let ml = MainLoop::new(tree);

    let list = instances(&ml);
    let mesh = list[0].mesh.as_ref().expect("placeholder geometry");
    assert_ne!(
        mesh.vertices.len(),
        6,
        "the fallback must not coincidentally have six vertices, or the \
         acceptance above proves nothing"
    );
}
