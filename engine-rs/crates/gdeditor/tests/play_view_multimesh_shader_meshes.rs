//! pat-mgixk.50 (partial) — the editor/Play 3D list expands a
//! `MultiMeshInstance3D` through the SAME expansion the render-server adapter
//! uses, honouring `visible_instance_count`.
//!
//! # What was wrong
//!
//! The shipped editor and Play view render through
//! `gdeditor::viewport_3d_render`, and that path had **no MultiMesh arm at all**
//! — `grep -i multimesh` in `viewport_3d_render.rs` and `viewport_3d.rs`
//! returned zero. Every `MultiMeshInstance3D` fell through to the 0.4 marker
//! cube, however many instances it authored. `RenderServer3DAdapter` expanded it
//! correctly and had no production caller.
//!
//! `visible_instance_count` was ignored on BOTH paths: the adapter's loop ran to
//! the full `instance_count`, so a MultiMesh authored with 3 instances and 2
//! visible drew 3.
//!
//! # The path driven
//!
//! ```text
//! .tscn -> SceneTree -> Viewport3D::build_render_list
//!   (what game_3d_render_inputs and render_editor_3d_frame_gpu both consume)
//!     -> render_server_3d::multimesh_instance_draws
//! ```
//!
//! ⚠️ Scope: this file covers the MultiMesh slice only. The ShaderMaterial,
//! `surface_material_override/N` and script-built-mesh halves of the bead are
//! reported as remaining scope, and the full-acceptance function will live here
//! beside this one.

use gdcore::math::Vector3;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{render_editor_3d_frame, Render3DKind};
use gdobject::class_db::{register_3d_classes, register_all_classes};
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;

/// Frame size for the pixel census. Matches
/// `mesh_properties_render_scaled.rs`, which proves a mesh at the origin is
/// visible to the default editor camera at this size.
const W: u32 = 128;
const H: u32 = 96;

/// Three instances spread along X so each is separable, with only the first two
/// visible.
///
/// Buffer layout is row-major 3x4 per instance — 12 floats, origin at indices
/// 3, 7 and 11 — copied from `gdscene/tests/multimesh_subresource_per_instance_transforms.rs`,
/// which pins that decoding.
///
/// 🔑 `visible_instance_count = 2` is a PREFIX count, so the surviving two must
/// be instances 0 and 1 (x = -6 and 0) and the dropped one must be instance 2
/// (x = +6). Asserting only the COUNT would pass on an implementation that kept
/// any two of the three.
const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="MultiMesh" id="MultiMesh_1"]
transform_format = 1
instance_count = 3
visible_instance_count = 2
buffer = PackedFloat32Array(1, 0, 0, -6, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 6, 0, 1, 0, 0, 0, 0, 1, 0)

[node name="Root" type="Node3D"]

[node name="MM" type="MultiMeshInstance3D" parent="."]
multimesh = SubResource("MultiMesh_1")
"#;

/// The same scene with every instance visible, as the control.
const ALL_VISIBLE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="MultiMesh" id="MultiMesh_1"]
transform_format = 1
instance_count = 3
buffer = PackedFloat32Array(1, 0, 0, -6, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 6, 0, 1, 0, 0, 0, 0, 1, 0)

[node name="Root" type="Node3D"]

[node name="MM" type="MultiMeshInstance3D" parent="."]
multimesh = SubResource("MultiMesh_1")
"#;

/// ⚠️ `register_3d_classes` is REQUIRED and its absence fails as an EMPTY list
/// rather than an error: `collect_scene_content` filters on
/// `is_class("Node3D")`, which reads the ClassDB, so without it the node is not
/// spatial, the list comes back empty and every assertion below passes
/// vacuously.
fn load(src: &str) -> (SceneTree, NodeId) {
    register_3d_classes();
    let scene = PackedScene::from_tscn(src).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the scene instances");
    let mm = tree
        .get_node_by_path("/root/Root/MM")
        .expect("the fixture has /root/Root/MM");
    (tree, mm)
}

/// The draw records the Play and editor 3D frames are both built from.
fn multimesh_items(tree: &SceneTree, mm: NodeId) -> Vec<(Vector3, Render3DKind)> {
    Viewport3D::new(320, 180)
        .build_render_list(tree)
        .into_iter()
        .filter(|item| item.node_id == mm.raw())
        .map(|item| (item.global_transform.origin, item.kind))
        .collect()
}

#[test]
fn play_view_renders_multimesh_visible_instances() {
    let (tree, mm) = load(SCENE);
    let items = multimesh_items(&tree, mm);

    // -- one draw per VISIBLE instance, not one marker for the node ----------
    assert_eq!(
        items.len(),
        2,
        "a MultiMeshInstance3D with instance_count 3 and visible_instance_count \
         2 must contribute exactly 2 draws. 1 means it fell through to the \
         marker-cube arm, which is what this path did before pat-mgixk.50; 3 \
         means visible_instance_count was ignored. Got {items:?}",
    );

    // -- real geometry, not the 0.4 placeholder cube -------------------------
    for (origin, kind) in &items {
        assert!(
            matches!(kind, Render3DKind::Geometry { .. }),
            "each instance must carry its own resolved mesh; a Primitive kind is \
             the marker fallthrough (instance at {origin:?} had {kind:?})",
        );
    }

    // 🔑 PREFIX semantics: the two survivors are instances 0 and 1.
    let xs: Vec<f32> = items.iter().map(|(o, _)| o.x).collect();
    assert!(
        xs.iter().any(|x| (*x + 6.0).abs() < 1e-4),
        "instance 0 (x = -6) must be drawn; got {xs:?}",
    );
    assert!(
        xs.iter().any(|x| x.abs() < 1e-4),
        "instance 1 (x = 0) must be drawn; got {xs:?}",
    );
    assert!(
        !xs.iter().any(|x| (*x - 6.0).abs() < 1e-4),
        "instance 2 (x = +6) is beyond visible_instance_count and must NOT be \
         drawn — visible_instance_count is a PREFIX count, so keeping any two \
         is not enough; got {xs:?}",
    );

    // -- the control: without the cap, all three draw ------------------------
    //
    // Proves the assertions above measure `visible_instance_count` and not some
    // unrelated reason the third instance went missing.
    let (all_tree, all_mm) = load(ALL_VISIBLE);
    let all = multimesh_items(&all_tree, all_mm);
    assert_eq!(
        all.len(),
        3,
        "with visible_instance_count unset, Godot's -1 sentinel means draw ALL, \
         so the same buffer must contribute 3 draws. Got {all:?}",
    );
}

// ---------------------------------------------------------------------------
// Script-built meshes (ImmediateMesh / SurfaceTool)
// ---------------------------------------------------------------------------

/// A `MeshInstance3D` whose mesh the script assigns at runtime.
const SCRIPT_MESH_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Mesh" type="MeshInstance3D" parent="."]
"#;

/// Builds an `ImmediateMesh` triangle and assigns it, copied from
/// `gdscene/tests/immediate_mesh_built_from_script_renders.rs` which pins this
/// API (Godot's `Mesh.PRIMITIVE_TRIANGLES` is 3).
const BUILDER_GD: &str = "\
extends MeshInstance3D
func _ready():
    var m = ImmediateMesh.new()
    m.surface_begin(3)
    m.surface_add_vertex(Vector3(0, 0, 0))
    m.surface_add_vertex(Vector3(1, 0, 0))
    m.surface_add_vertex(Vector3(0, 1, 0))
    m.surface_end()
    self.mesh = m
";

/// A GDScript-built mesh reaches the editor/Play draw list, not a marker cube.
///
/// `collect_scene_content` reads only `Variant::String` mesh paths, so a mesh
/// assigned from a script — which arrives as a tagged `ObjectId` handle — left
/// `mesh_path` empty and the node fell through to the 0.4 marker.
/// `RenderServer3DAdapter::resolve_mesh` already decoded those handles and had
/// no caller outside the adapter (pat-mgixk.50).
#[test]
fn play_view_renders_a_script_built_mesh() {
    // `register_all_classes`, not just the 3D set: the script calls
    // `ImmediateMesh.new()`, which needs the resource classes in the ClassDB as
    // well as `is_class("Node3D")` for the render list.
    register_all_classes();

    let scene = PackedScene::from_tscn(SCRIPT_MESH_SCENE).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the scene instances");
    let mesh_node = tree
        .get_node_by_path("/root/Root/Mesh")
        .expect("the fixture has /root/Root/Mesh");

    let script = GDScriptNodeInstance::from_source(BUILDER_GD, mesh_node).expect("script parses");
    tree.attach_script(mesh_node, Box::new(script));
    let mut ml = MainLoop::new(tree);
    // ⚠️ Explicit: `MainLoop::step` does not fire `_ready` for a script attached
    // after construction, and the whole test is what `_ready` builds.
    ml.tree_mut().process_script_ready(mesh_node);

    let items: Vec<_> = Viewport3D::new(320, 180)
        .build_render_list(ml.tree())
        .into_iter()
        .filter(|item| item.node_id == mesh_node.raw())
        .collect();
    assert_eq!(items.len(), 1, "the MeshInstance3D contributes one draw");

    let Render3DKind::Geometry { mesh } = &items[0].kind else {
        panic!(
            "a script-built mesh must become inline Geometry; a Primitive kind \
             is the marker-cube fallthrough this bead fixes (got {:?})",
            items[0].kind
        )
    };

    // 🔑 The VERTEX COUNT is the assertion that matters. `resolve_mesh` falls
    // back to `Mesh3D::cube(1.0)` for anything it cannot resolve, and a cube is
    // also a single-surface Geometry item — so "it is Geometry" alone passes
    // against a mesh that ignored the script entirely. A triangle has 3; a cube
    // has 8 or more.
    assert_eq!(
        mesh.vertices.len(),
        3,
        "the drawn mesh must be the script's TRIANGLE, not the fallback cube",
    );
}

// ---------------------------------------------------------------------------
// ShaderMaterial on the 3D editor/Play path
// ---------------------------------------------------------------------------

/// A box at the origin, optionally carrying a `material_override` that is a
/// ShaderMaterial writing a constant red `ALBEDO`.
///
/// The `.tscn` shape (BoxMesh sub-resource + `MeshInstance3D`) is copied from
/// `gdscene/tests/material_resource_to_pixels.rs`, and the
/// Shader/ShaderMaterial sub-resource chain from
/// `gdeditor/tests/shader_language_frontend_drives_scene_pixels.rs` — both
/// green at HEAD, so a failure here is about the 3D editor path carrying the
/// material, not about `.tscn` parsing or the shader front end.
///
/// ⚠️ No `Camera3D` node: `render_editor_3d_frame` renders through the editor
/// viewport's own camera, and a `Camera3D` would only add a gizmo to the frame.
///
/// 🔑 The two variants differ ONLY in the override line and the sub-resources it
/// needs. The grid, the sky gradient and every gizmo are identical in both, which
/// is what makes differencing them isolate the mesh's own pixels.
fn shaded_box_tscn(shaded: bool) -> String {
    let (subresources, override_line) = if shaded {
        (
            r#"
[sub_resource type="Shader" id="Shader_1"]
code = "shader_type spatial; void fragment() { ALBEDO = vec3(1.0, 0.0, 0.0); }"

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = SubResource("Shader_1")
"#,
            "material_override = SubResource(\"ShaderMaterial_1\")\n",
        )
    } else {
        ("", "")
    };
    format!(
        r#"[gd_scene load_steps=4 format=3]

[sub_resource type="BoxMesh" id="Mesh_1"]
{subresources}
[node name="Root" type="Node3D"]

[node name="Mesh" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)
mesh = SubResource("Mesh_1")
{override_line}"#
    )
}

fn load_box(shaded: bool) -> (SceneTree, NodeId) {
    register_3d_classes();
    let src = shaded_box_tscn(shaded);
    let scene = PackedScene::from_tscn(&src).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the scene instances");
    let mesh = tree
        .get_node_by_path("/root/Root/Mesh")
        .expect("the fixture has /root/Root/Mesh");
    (tree, mesh)
}

/// Red-DOMINANT rather than "bright red".
///
/// 🔑 The shader's output is still lit — ambient and any light term multiply it —
/// so the rendered red is some unknown fraction of `vec3(1, 0, 0)` and a
/// magnitude test like `r > 0.5` can fail on a correct frame. Channel dominance
/// is invariant under that scaling, and is still specific to RED: a green or
/// blue result fails it.
fn is_red(c: &gdcore::math::Color) -> bool {
    c.r - c.g > 0.05 && c.r - c.b > 0.05
}

/// **The ShaderMaterial slice of pat-mgixk.50.**
///
/// A `MeshInstance3D` whose `material_override` is a ShaderMaterial setting
/// `ALBEDO` red draws RED on the editor/Play 3D path.
///
/// # What was wrong
///
/// `RenderServer3DAdapter::resolve_shader_material` already read the flattened
/// `shader_code` / `shader_type` / `shader_parameter/*` properties off the node,
/// and `gdrender3d`'s software renderer already evaluated `inst.shader_material`
/// per fragment (`renderer.rs:1102`) — but `viewport_3d_render`, which is what
/// the editor and Play actually render through, never resolved one and never
/// called `set_shader_material`. `Render3DItem` had no field to carry it. So
/// every authored 3D shader rendered as the default lit material.
///
/// # Both entry points
///
/// The material is applied in `populate_editor_scene_3d`, which is the single
/// scene-population function behind BOTH `render_editor_3d_frame` (software,
/// driven below) and `render_editor_3d_frame_gpu`.
#[test]
fn play_view_applies_a_shader_material_to_a_mesh() {
    let (tree, mesh) = load_box(true);

    // -- exact: the draw item carries the resolved material ------------------
    let items: Vec<_> = Viewport3D::new(W, H)
        .build_render_list(&tree)
        .into_iter()
        .filter(|item| item.node_id == mesh.raw())
        .collect();
    assert_eq!(items.len(), 1, "the MeshInstance3D contributes one draw");
    let shader_mat = items[0]
        .shader_material
        .as_ref()
        .expect("the draw item must carry the authored ShaderMaterial; None is the pre-bead behaviour, where viewport_3d_render resolved no shader at all");
    assert!(
        shader_mat.shader.is_some(),
        "the ShaderMaterial must carry its Shader — a Some material with a None \
         shader renders unshaded, so this is the half that actually reaches the \
         fragment stage",
    );

    // -- observable: the mesh's own pixels are red ---------------------------
    //
    // 🔑 Differenced against the identical scene WITHOUT the override, so the
    // census covers exactly the pixels the material changed. The editor frame
    // also draws a red X grid line and axis chrome; those are byte-identical in
    // both frames and therefore cannot be mistaken for the shaded mesh.
    let shaded = render_editor_3d_frame(&Viewport3D::new(W, H), &tree);
    let (plain_tree, _) = load_box(false);
    let plain = render_editor_3d_frame(&Viewport3D::new(W, H), &plain_tree);

    assert_eq!(
        shaded.pixels.len(),
        plain.pixels.len(),
        "same viewport, same pixel count",
    );

    let differing: Vec<usize> = (0..shaded.pixels.len())
        .filter(|&i| {
            let (a, b) = (&shaded.pixels[i], &plain.pixels[i]);
            (a.r - b.r).abs() > 1e-3 || (a.g - b.g).abs() > 1e-3 || (a.b - b.b).abs() > 1e-3
        })
        .collect();
    assert!(
        !differing.is_empty(),
        "the authored ShaderMaterial must change the frame at all; zero differing \
         pixels means it never reached the renderer, which is exactly the defect \
         this bead fixes",
    );

    let red_shaded = differing
        .iter()
        .filter(|&&i| is_red(&shaded.pixels[i]))
        .count();
    let red_plain = differing
        .iter()
        .filter(|&&i| is_red(&plain.pixels[i]))
        .count();

    assert_eq!(
        red_plain,
        0,
        "the control's default material is achromatic, so none of the {} changed \
         pixels may be red-dominant WITHOUT the shader. A non-zero count means \
         the difference is not the material and the assertion below proves \
         nothing",
        differing.len(),
    );
    // A strict MAJORITY rather than a near-total share: the discriminating gap
    // here is between ~0% (the colour never arrived) and ~100% (it did), so a
    // majority separates those two outcomes just as decisively while leaving no
    // room for a false failure from partially-covered silhouette edge pixels.
    assert!(
        red_shaded * 2 > differing.len(),
        "the shader writes a CONSTANT red ALBEDO, so most of the pixels it \
         changed must be red-dominant: {red_shaded} of {} were. A near-zero \
         count with a non-empty difference means the material arrived but its \
         colour did not — e.g. the fragment body parsed to zero instructions",
        differing.len(),
    );
}
