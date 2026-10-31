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
use gdeditor::viewport_3d_render::{render_editor_3d_frame, render_game_3d_frame, Render3DKind};
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
/// Carries a `Camera3D` because the colour census renders through
/// [`render_game_3d_frame`], the Play path, which needs the scene's own camera.
///
/// 🔑 THE CENSUS MUST NOT RUN THROUGH THE EDITOR FRAME. The first version of
/// this test rendered `render_editor_3d_frame` and asserted the control had ZERO
/// red-dominant pixels; it failed with **28**. The editor draws a RED X origin
/// axis and alpha-BLENDS it over whatever is behind it, so those composited
/// pixels depend on the mesh colour underneath and are therefore *not* identical
/// between the two variants — they land in the difference set, red, in the
/// control frame. "The chrome is the same in both frames" is false for any
/// overlay blended over the thing under test. `render_game_3d_frame` documents
/// that "the grid and origin axes are off", which removes the confound at the
/// source instead of loosening the assertion.
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

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 4)

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
    // census covers exactly the pixels the material changed — and rendered
    // through the Play frame, which draws no grid and no origin axes. See
    // `shaded_box_tscn` for why the editor frame cannot be used here.
    let shaded = render_game_3d_frame(&tree, W, H).expect("the fixture has a Camera3D");
    let (plain_tree, _) = load_box(false);
    let plain = render_game_3d_frame(&plain_tree, W, H).expect("the fixture has a Camera3D");

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

// ---------------------------------------------------------------------------
// surface_material_override/N
// ---------------------------------------------------------------------------

/// Builds a TWO-surface `ImmediateMesh`: two triangles side by side in the
/// z = 0 plane, each wound counter-clockwise as seen from the default editor
/// camera (which sits at +Z looking toward -Z), so neither is back-face culled.
///
/// 🔑 Two surfaces is the whole point. `ImmediateMesh::to_mesh` documents that
/// "surface 0 becomes the mesh's primary vertex data and the rest become
/// `Mesh3D::surfaces`", and the renderer pairs `surface_materials[i]` with
/// `surfaces[i]` — which Godot spells `surface_material_override/{i + 1}`. So a
/// two-surface mesh is the smallest fixture on which `/1` can mean anything at
/// all, and it is reachable from a script without an imported asset.
const TWO_SURFACE_GD: &str = "\
extends MeshInstance3D
func _ready():
    var m = ImmediateMesh.new()
    m.surface_begin(3)
    m.surface_add_vertex(Vector3(-1.4, -0.6, 0))
    m.surface_add_vertex(Vector3(-0.2, -0.6, 0))
    m.surface_add_vertex(Vector3(-0.8, 0.6, 0))
    m.surface_end()
    m.surface_begin(3)
    m.surface_add_vertex(Vector3(0.2, -0.6, 0))
    m.surface_add_vertex(Vector3(1.4, -0.6, 0))
    m.surface_add_vertex(Vector3(0.8, 0.6, 0))
    m.surface_end()
    self.mesh = m
";

/// The two-surface node, with `surface_material_override/1` green or absent.
///
/// 🔑 The two variants differ ONLY in that override. Without it the second
/// surface still DRAWS — the renderer falls back to the instance material, which
/// is what Godot does for a surface with no override — so differencing the two
/// frames isolates exactly the pixels whose COLOUR the override decided, rather
/// than the pixels of a surface that appeared out of nowhere.
fn two_surface_tscn(overridden: bool) -> String {
    let (subres, line) = if overridden {
        (
            "\n[sub_resource type=\"StandardMaterial3D\" id=\"Green_1\"]\nalbedo_color = Color(0, 1, 0, 1)\n",
            "surface_material_override/1 = SubResource(\"Green_1\")\n",
        )
    } else {
        ("", "")
    };
    format!(
        r#"[gd_scene load_steps=2 format=3]
{subres}
[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 4)

[node name="Surf" type="MeshInstance3D" parent="."]
{line}"#
    )
}

/// Instances the two-surface fixture and runs its script's `_ready`.
fn load_two_surface(overridden: bool) -> (MainLoop, NodeId) {
    register_all_classes();
    let src = two_surface_tscn(overridden);
    let scene = PackedScene::from_tscn(&src).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the scene instances");
    let surf = tree
        .get_node_by_path("/root/Root/Surf")
        .expect("the fixture has /root/Root/Surf");
    let script = GDScriptNodeInstance::from_source(TWO_SURFACE_GD, surf).expect("script parses");
    tree.attach_script(surf, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(surf);
    (ml, surf)
}

fn is_green(c: &gdcore::math::Color) -> bool {
    c.g - c.r > 0.05 && c.g - c.b > 0.05
}

/// **The `surface_material_override/N` slice of pat-mgixk.50.**
///
/// A mesh's SECOND surface draws with its own authored material on the
/// editor/Play 3D path.
///
/// # What was wrong
///
/// `surface_materials_for_node` (`render_server_3d.rs:3012`) already read the
/// flattened `__surfmat<N>__*` companions, and `gdrender3d` already drew each
/// extra surface with its own override (`renderer.rs:1053`). But
/// `viewport_3d_render` never called either: `grep -rn "surface_materials"
/// crates/gdeditor/src` was empty, and `Render3DItem` carried ONE material, so
/// every per-surface override was dropped on the path the editor and Play use.
#[test]
fn play_view_renders_a_per_surface_material_override() {
    let (ml, surf) = load_two_surface(true);

    // -- exact: the draw item carries the per-surface override ---------------
    let items: Vec<_> = Viewport3D::new(W, H)
        .build_render_list(ml.tree())
        .into_iter()
        .filter(|item| item.node_id == surf.raw())
        .collect();
    assert_eq!(items.len(), 1, "the MeshInstance3D contributes one draw");

    // The mesh must really have a second surface, or the override below pairs
    // with nothing and the pixel census that follows would be vacuous.
    let Render3DKind::Geometry { mesh } = &items[0].kind else {
        panic!(
            "the script-built mesh must become inline Geometry (got {:?})",
            items[0].kind
        )
    };
    assert_eq!(
        mesh.surfaces.len(),
        1,
        "surface 0 is the mesh's primary vertex data and surface 1 lands in \
         `surfaces`, so a two-surface ImmediateMesh must report exactly 1 extra \
         surface — 0 means the second `surface_begin`/`surface_end` pair was \
         lost and `surface_material_override/1` has nothing to apply to",
    );

    // 🔑 ONE entry, not two: `surface_materials_for_node` returns a vec indexed
    // so that entry 0 is `surface_material_override/1` (`out[index - 1]`,
    // because `/0` is the whole-mesh material and goes through the
    // `__material__*` bag instead). An off-by-one here would paint the wrong
    // surface, so the LENGTH is asserted, not just that something is present.
    assert_eq!(
        items[0].surface_materials.len(),
        1,
        "surface_material_override/1 must resolve to exactly one entry, at index \
         0; got {:?}",
        items[0].surface_materials,
    );
    let green = items[0].surface_materials[0]
        .as_ref()
        .expect("entry 0 must be the authored green material, not None");
    assert!(
        green.albedo.g > 0.9 && green.albedo.r < 0.1 && green.albedo.b < 0.1,
        "the authored albedo must survive the trip; got {:?}",
        green.albedo,
    );

    // -- observable: the second surface is green, and only with the override --
    // The Play frame, for the same reason as the shader census: the editor's
    // blended origin axes would contaminate a colour count.
    let shaded = render_game_3d_frame(ml.tree(), W, H).expect("the fixture has a Camera3D");
    let (plain_ml, _) = load_two_surface(false);
    let plain = render_game_3d_frame(plain_ml.tree(), W, H).expect("the fixture has a Camera3D");

    let differing: Vec<usize> = (0..shaded.pixels.len())
        .filter(|&i| {
            let (a, b) = (&shaded.pixels[i], &plain.pixels[i]);
            (a.r - b.r).abs() > 1e-3 || (a.g - b.g).abs() > 1e-3 || (a.b - b.b).abs() > 1e-3
        })
        .collect();
    assert!(
        !differing.is_empty(),
        "the authored per-surface material must change the frame; zero differing \
         pixels means `surface_material_override/1` never reached the renderer, \
         which is the defect this slice fixes",
    );

    let green_shaded = differing
        .iter()
        .filter(|&&i| is_green(&shaded.pixels[i]))
        .count();
    let green_plain = differing
        .iter()
        .filter(|&&i| is_green(&plain.pixels[i]))
        .count();
    assert_eq!(
        green_plain,
        0,
        "without the override the second surface falls back to the achromatic \
         instance material, so none of the {} changed pixels may be \
         green-dominant; a non-zero count means the difference is not the \
         material",
        differing.len(),
    );
    assert!(
        green_shaded * 2 > differing.len(),
        "most of the pixels the override changed must be green-dominant: \
         {green_shaded} of {}. A near-zero count means the material arrived but \
         was applied to no surface",
        differing.len(),
    );
}

// ---------------------------------------------------------------------------
// The bead's named acceptance: all four features in ONE scene
// ---------------------------------------------------------------------------

/// A one-surface triangle, for the script-built-mesh corner of the combined
/// fixture.
const TRI_GD: &str = "\
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

/// One scene carrying all four features the bead names, laid out inside the
/// default editor camera's view (focus at the origin, distance 5).
const COMBINED: &str = r#"[gd_scene load_steps=5 format=3]

[sub_resource type="MultiMesh" id="MultiMesh_1"]
transform_format = 1
instance_count = 3
visible_instance_count = 2
buffer = PackedFloat32Array(1, 0, 0, -1, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0)

[sub_resource type="BoxMesh" id="Box_1"]

[sub_resource type="Shader" id="Shader_1"]
code = "shader_type spatial; void fragment() { ALBEDO = vec3(1.0, 0.0, 0.0); }"

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = SubResource("Shader_1")

[sub_resource type="StandardMaterial3D" id="Green_1"]
albedo_color = Color(0, 1, 0, 1)

[node name="Root" type="Node3D"]

[node name="MM" type="MultiMeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, -1.8, 0)
multimesh = SubResource("MultiMesh_1")

[node name="Shaded" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, -1.6, -0.6, 0)
mesh = SubResource("Box_1")
material_override = SubResource("ShaderMaterial_1")

[node name="Surf" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1.4, 0)
surface_material_override/1 = SubResource("Green_1")

[node name="Tri" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 1.4, -0.9, 0)
"#;

/// A bare root, as the control that proves the fixture reaches pixels at all.
const EMPTY: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]
"#;

/// **pat-mgixk.50 acceptance.** One Play/editor 3D scene-sync produces correct
/// draw records for all four node kinds the bead names, from one `.tscn`.
///
/// # Why one fixture matters
///
/// Each feature has its own focused test above, including the pixel-level
/// observables (red shader mesh, green second surface) which are measured there
/// by differencing a two-variant scene — the clean way to isolate one node's
/// contribution. What those cannot show is that the four resolve **together**:
/// the MultiMesh arm returns early from `build_render_list`'s `flat_map`, the
/// script-mesh and shader resolutions happen before it, and the per-surface
/// materials after. A reordering or a misplaced early return breaks a
/// combination while every single-feature test stays green. That is the claim
/// here.
///
/// Both entry points are covered by construction: the materials are applied in
/// `populate_editor_scene_3d`, the one scene-population function behind both
/// `render_editor_3d_frame` (software, driven here) and
/// `render_editor_3d_frame_gpu`.
#[test]
fn play_view_renders_multimesh_shader_and_script_meshes() {
    register_all_classes();
    let scene = PackedScene::from_tscn(COMBINED).expect("the combined .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the scene instances");

    let node = |t: &SceneTree, path: &str| t.get_node_by_path(path).expect("fixture node exists");
    let mm = node(&tree, "/root/Root/MM");
    let shaded = node(&tree, "/root/Root/Shaded");
    let surf = node(&tree, "/root/Root/Surf");
    let tri = node(&tree, "/root/Root/Tri");

    for (nid, src) in [(surf, TWO_SURFACE_GD), (tri, TRI_GD)] {
        let script = GDScriptNodeInstance::from_source(src, nid).expect("script parses");
        tree.attach_script(nid, Box::new(script));
    }
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(surf);
    ml.tree_mut().process_script_ready(tri);

    let list = Viewport3D::new(W, H).build_render_list(ml.tree());
    let of = |nid: NodeId| -> Vec<&gdeditor::viewport_3d_render::Render3DItem> {
        list.iter().filter(|i| i.node_id == nid.raw()).collect()
    };

    // -- 1. MultiMesh: 2 of 3 instances, prefix semantics -------------------
    let mm_items = of(mm);
    assert_eq!(
        mm_items.len(),
        2,
        "exactly 2 multimesh blobs (instance_count 3, visible_instance_count 2); \
         1 is the marker-cube fallthrough and 3 ignores the cap",
    );
    let mut mm_xs: Vec<f32> = mm_items
        .iter()
        .map(|i| i.global_transform.origin.x)
        .collect();
    mm_xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert!(
        (mm_xs[0] + 1.0).abs() < 1e-4 && mm_xs[1].abs() < 1e-4,
        "the surviving instances must be the PREFIX (x = -1 and 0), not any two \
         of the three; got {mm_xs:?}",
    );
    for item in &mm_items {
        assert!(
            (item.global_transform.origin.y + 1.8).abs() < 1e-4,
            "each instance must compose with the MultiMeshInstance3D's own \
             transform (y = -1.8); got {:?}",
            item.global_transform.origin,
        );
    }

    // -- 2. ShaderMaterial on material_override -----------------------------
    let shaded_items = of(shaded);
    assert_eq!(shaded_items.len(), 1, "the shaded box contributes one draw");
    let sm = shaded_items[0]
        .shader_material
        .as_ref()
        .expect("the shaded box must carry its ShaderMaterial");
    assert!(
        sm.shader.is_some(),
        "the ShaderMaterial must carry its Shader, or it renders unshaded",
    );

    // -- 3. surface_material_override/1 on a 2-surface script mesh ----------
    let surf_items = of(surf);
    assert_eq!(
        surf_items.len(),
        1,
        "the two-surface mesh contributes one draw"
    );
    let Render3DKind::Geometry { mesh } = &surf_items[0].kind else {
        panic!("the two-surface script mesh must be inline Geometry")
    };
    assert_eq!(
        mesh.surfaces.len(),
        1,
        "the second ImmediateMesh surface must survive as `surfaces[0]`, or the \
         override pairs with nothing",
    );
    assert_eq!(
        surf_items[0].surface_materials.len(),
        1,
        "surface_material_override/1 resolves to exactly one entry at index 0; \
         got {:?}",
        surf_items[0].surface_materials,
    );
    let green = surf_items[0].surface_materials[0]
        .as_ref()
        .expect("entry 0 is the authored green material");
    assert!(
        green.albedo.g > 0.9 && green.albedo.r < 0.1 && green.albedo.b < 0.1,
        "the authored green must survive; got {:?}",
        green.albedo,
    );

    // -- 4. GDScript-built ImmediateMesh triangle ---------------------------
    let tri_items = of(tri);
    assert_eq!(tri_items.len(), 1, "the triangle contributes one draw");
    let Render3DKind::Geometry { mesh } = &tri_items[0].kind else {
        panic!(
            "a script-built mesh must become inline Geometry, not the marker \
             cube (got {:?})",
            tri_items[0].kind
        )
    };
    assert_eq!(
        mesh.vertices.len(),
        3,
        "the drawn mesh must be the script's TRIANGLE; `resolve_mesh` falls back \
         to a cube, which is also single-surface Geometry, so the vertex count \
         is what discriminates",
    );

    // -- and the whole fixture reaches pixels --------------------------------
    //
    // The per-feature COLOUR claims are proved by differencing in the focused
    // tests above; what this adds is that the combined scene is actually drawn,
    // so none of the four assertions above describes a draw record that the
    // render pass then discards.
    let frame = render_editor_3d_frame(&Viewport3D::new(W, H), ml.tree());
    let empty_scene = PackedScene::from_tscn(EMPTY).expect("the empty .tscn parses");
    let mut empty_tree = SceneTree::new();
    let empty_root = empty_tree.root_id();
    add_packed_scene_to_tree(&mut empty_tree, empty_root, &empty_scene).expect("instances");
    let empty = render_editor_3d_frame(&Viewport3D::new(W, H), &empty_tree);

    let differing = (0..frame.pixels.len())
        .filter(|&i| {
            let (a, b) = (&frame.pixels[i], &empty.pixels[i]);
            (a.r - b.r).abs() > 1e-3 || (a.g - b.g).abs() > 1e-3 || (a.b - b.b).abs() > 1e-3
        })
        .count();
    assert!(
        differing > 0,
        "the four-node fixture must change the frame against an empty scene; 0 \
         differing pixels means every draw record above was built and then never \
         rasterized",
    );
}

// ---------------------------------------------------------------------------
// The MultiMesh's own mesh sub-resource
// ---------------------------------------------------------------------------

/// A one-instance MultiMesh whose `mesh` is a sub-resource of class `class`, or
/// with no `mesh` at all when `class` is empty.
fn multimesh_mesh_tscn(class: &str) -> String {
    let (subres, line) = if class.is_empty() {
        (String::new(), String::new())
    } else {
        (
            format!("\n[sub_resource type=\"{class}\" id=\"NestedMesh_1\"]\n"),
            "mesh = SubResource(\"NestedMesh_1\")\n".to_string(),
        )
    };
    // The nested mesh is declared BEFORE the MultiMesh that references it, which
    // is the dependency order Godot writes.
    format!(
        r#"[gd_scene load_steps=3 format=3]
{subres}
[sub_resource type="MultiMesh" id="MultiMesh_1"]
transform_format = 1
instance_count = 1
buffer = PackedFloat32Array(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0)
{line}
[node name="Root" type="Node3D"]

[node name="MM" type="MultiMeshInstance3D" parent="."]
multimesh = SubResource("MultiMesh_1")
"#
    )
}

/// The mesh the single expanded instance actually draws.
fn multimesh_drawn_mesh(class: &str) -> gdserver3d::mesh::Mesh3D {
    register_3d_classes();
    let src = multimesh_mesh_tscn(class);
    let scene = PackedScene::from_tscn(&src).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the scene instances");
    let mm = tree
        .get_node_by_path("/root/Root/MM")
        .expect("the fixture has /root/Root/MM");
    let items: Vec<_> = Viewport3D::new(W, H)
        .build_render_list(&tree)
        .into_iter()
        .filter(|i| i.node_id == mm.raw())
        .collect();
    assert_eq!(items.len(), 1, "one instance, one draw");
    match &items[0].kind {
        Render3DKind::Geometry { mesh } => mesh.clone(),
        other => panic!("a multimesh instance must draw inline Geometry, got {other:?}"),
    }
}

/// **The `MultiMesh.mesh` slice of pat-mgixk.50.** A MultiMesh draws the mesh
/// resource it authored, not a stand-in cube.
///
/// # What was wrong
///
/// `packed_scene` copies the MultiMesh body onto the node verbatim, so
/// `__multimesh__mesh` arrived as the bare string `SubResource:<id>` — an id
/// with no class and no properties. Nothing downstream could turn that into
/// geometry, so `multimesh_mesh_for_node` fell through to `Mesh3D::cube(1.0)`
/// however the mesh was authored.
///
/// 🔑 The oracle is DERIVED, not guessed: the fallback's own vertex count is
/// read from `Mesh3D::cube(1.0)` in-test. Asserting a hard-coded number would
/// silently pass if the fallback and a real primitive happened to agree, which is
/// exactly the trap here — a `BoxMesh` sub-resource would render
/// indistinguishably from the cube fallback, so this fixture authors a
/// `SphereMesh` instead.
#[test]
fn play_view_renders_the_multimesh_s_own_mesh_resource() {
    let fallback_vertices = gdserver3d::mesh::Mesh3D::cube(1.0).vertices.len();

    // Control: no `mesh` authored at all -> Godot draws nothing, we draw the
    // documented stand-in. This pins what "fell through" looks like.
    let control = multimesh_drawn_mesh("");
    assert_eq!(
        control.vertices.len(),
        fallback_vertices,
        "with no authored mesh the instance still draws the stand-in cube, so \
         this must match `Mesh3D::cube(1.0)` — if it does not, the comparison \
         below is measuring something else",
    );

    // The authored SphereMesh must actually be built.
    let sphere = multimesh_drawn_mesh("SphereMesh");
    assert_ne!(
        sphere.vertices.len(),
        fallback_vertices,
        "an authored `mesh = SubResource(SphereMesh)` must build that primitive; \
         matching the cube's vertex count means `__multimesh__mesh` was still \
         the unresolved `SubResource:<id>` string and the fallback won",
    );
    assert!(
        sphere.vertices.len() > fallback_vertices,
        "a sphere is tessellated into many more vertices than the stand-in cube \
         ({} vs {fallback_vertices}); a smaller count means some other mesh was \
         substituted",
        sphere.vertices.len(),
    );
}
