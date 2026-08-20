//! pat-s3rrv.187 — `VisualInstance3D.sorting_offset` reaches the transparent
//! draw order, and a per-surface material override beyond surface 0 survives a
//! `.tscn` round trip.
//!
//! Ground truth: `upstream/godot/scene/3d/visual_instance_3d.{h,cpp}` and
//! `servers/rendering/renderer_rd/forward_clustered/render_forward_clustered.cpp:955`,
//! which is the line that actually defines the property's meaning:
//! `inst->depth = cam.origin.distance_to(center) - inst->sorting_offset`.
//!
//! # What was wrong
//!
//! `sorting_offset` and `sorting_use_aabb_center` were registered in ClassDB and
//! present in the inspector schema, and **no render path read either one** — the
//! value was authorable, round-tripped through a `.tscn`, and changed nothing on
//! screen. Likewise a mesh with two materials kept only
//! `surface_material_override/0`; the second was dropped at load and could never
//! be written back.
//!
//! # Scope — one acceptance clause is NOT covered here
//!
//! The bead also asks that "a two-surface mesh **renders** both
//! `surface_material_override/0` and `/1` materials". That is not implemented and
//! is not asserted below. `gdrender3d` never reads `Mesh3D::surfaces` in either
//! backend — the software rasterizer and the wgpu pipeline both iterate only the
//! top-level `vertices`/`indices`, i.e. the primary surface — so extra surfaces
//! are not drawn at all, with or without a material. Making them render is a
//! renderer change (a per-surface draw loop in both backends, plus a per-surface
//! material list on `Instance3D` and a sync channel to carry it), not a property
//! read. What IS covered is that the second override survives load and save, so
//! the data is there when that work happens.

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::renderer::SoftwareRenderer3D;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::geometry_instance_settings_for_node;
use gdscene::scene_saver::TscnSaver;
use gdscene::scene_tree::SceneTree;
use gdserver3d::instance::GiMode;
use gdserver3d::material::{Material3D, Transparency};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;
use gdvariant::Variant;

/// Two transparent meshes at the SAME depth, so distance cannot break the tie
/// and `sorting_offset` is the only thing that can decide the order.
///
/// ⚠️ `A` is declared first and carries the offset. Without the offset the sort
/// is stable and leaves them in scene order, so "A then B" is the answer a build
/// that ignores the property gives — which is exactly what must NOT happen.
const SORTING_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="A" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, -4)
sorting_offset = 5.0

[node name="B" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, -4)
"#;

/// The same pair with no sorting authored at all — the control.
const PLAIN_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="A" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, -4)

[node name="B" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, -4)
"#;

/// A mesh with two authored surface materials, red on surface 0 and blue on 1.
const TWO_SURFACE_SCENE: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="StandardMaterial3D" id="Mat_0"]
albedo_color = Color(1, 0, 0, 1)
metallic = 0.25

[sub_resource type="StandardMaterial3D" id="Mat_1"]
albedo_color = Color(0, 0, 1, 1)
roughness = 0.75

[node name="Root" type="Node3D"]

[node name="Mesh" type="MeshInstance3D" parent="."]
surface_material_override/0 = SubResource("Mat_0")
surface_material_override/1 = SubResource("Mat_1")
"#;

fn load(src: &str) -> (SceneTree, NodeId) {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    (tree, root)
}

fn find(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == name)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no node named {name}"))
}

/// Builds a renderer holding one translucent cube per named scene node, each
/// carrying that node's real `GeometryInstanceSettings`, and returns the order
/// the transparent pass would draw them in.
///
/// ⚠️ Drives the settings through `geometry_instance_settings_for_node` — the
/// function the editor viewport and the runtime adapter SHARE — rather than
/// hand-filling the struct. Hand-filling would assert that the renderer honours
/// a field, and prove nothing about a `.tscn` reaching it, which is the half
/// that was missing.
fn draw_order(tree: &SceneTree, names: &[&str]) -> Vec<String> {
    let mut renderer = SoftwareRenderer3D::new();
    let mut ids = Vec::new();
    for name in names {
        let nid = find(tree, name);
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));
        renderer.set_material(
            id,
            Material3D {
                transparency: Transparency::Alpha,
                ..Default::default()
            },
        );
        // ⚠️ Both meshes are placed at the SAME point on purpose, and the
        // origin is fixed here rather than read back off the node: identical
        // depths mean `camera.distance_to(..)` is equal for both, so the sort
        // has nothing to go on EXCEPT `sorting_offset`. Reading a transform back
        // would let a decomposition difference decide the order instead.
        renderer.set_transform(
            id,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 0.0, -4.0),
            },
        );
        renderer.set_geometry_instance_settings(id, geometry_instance_settings_for_node(tree, nid));
        ids.push((id, name.to_string()));
    }

    let vp = Viewport3D::new(32, 32);
    let order = SoftwareRenderer3D::transparency_draw_order(renderer.instances(), &vp);
    order
        .iter()
        .map(|inst| {
            ids.iter()
                .find(|(id, _)| *id == inst.id)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| "?".to_string())
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The bead's acceptance
// ---------------------------------------------------------------------------

/// 🔑 **THE ACCEPTANCE.** An authored `sorting_offset` swaps the draw order of
/// two overlapping transparent meshes, and a `surface_material_override/1`
/// survives a `.tscn` round trip.
#[test]
fn visual_instance3d_sorting_and_multi_surface_overrides() {
    // -- sorting ------------------------------------------------------------
    let (plain, _) = load(PLAIN_SCENE);
    assert_eq!(
        draw_order(&plain, &["A", "B"]),
        vec!["A".to_string(), "B".to_string()],
        "with nothing authored the two overlapping meshes keep scene order — \
         this is the answer a build that ignores sorting_offset gives, so the \
         swap below is the whole test"
    );

    let (sorted, _) = load(SORTING_SCENE);
    assert_eq!(
        draw_order(&sorted, &["A", "B"]),
        vec!["B".to_string(), "A".to_string()],
        "A authored sorting_offset = 5.0, so it must be drawn LAST — on top. \
         Godot subtracts the offset from the sort depth \
         (render_forward_clustered.cpp:955), and the transparent pass draws \
         farthest-first, so a larger offset means a later draw"
    );

    // -- per-surface overrides ----------------------------------------------
    let (two, root) = load(TWO_SURFACE_SCENE);
    let mesh = find(&two, "Mesh");

    // Surface 1's material body reached the node.
    match two
        .get_node(mesh)
        .map(|n| n.get_property("__surfmat1__albedo_color"))
    {
        Some(Variant::Color(c)) => assert!(
            c.b > 0.9 && c.r < 0.1,
            "surface 1's material must be the BLUE one, got {c:?}"
        ),
        other => panic!("surface 1's material body must reach the node, got {other:?}"),
    }

    let saved = TscnSaver::save_tree(&two, root);
    assert!(
        saved.contains("surface_material_override/1"),
        "the second override must survive the save:\n{saved}"
    );
    // ⚠️ The reference alone is not enough — a reference to a sub-resource whose
    // body was dropped restores as an empty material. The authored `roughness`
    // is unique to surface 1's material, so finding it proves the right BODY was
    // rebuilt and not merely the pointer.
    assert!(
        saved.contains("roughness"),
        "surface 1's material BODY must be rebuilt, not just its reference:\n{saved}"
    );
}

/// ⚠️ The control for the sorting half: the offset must not reorder OPAQUE
/// meshes.
///
/// `sorting_offset` biases the transparent depth sort only — depth testing
/// decides opaque order, and Godot's opaque pass is not sorted by this. A build
/// that applied the offset everywhere would silently perturb every opaque scene.
#[test]
fn sorting_offset_does_not_reorder_opaque_meshes() {
    let (sorted, _) = load(SORTING_SCENE);
    let mut renderer = SoftwareRenderer3D::new();
    for name in ["A", "B"] {
        let nid = find(&sorted, name);
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));
        // Opaque: the default transparency.
        renderer.set_material(id, Material3D::default());
        renderer.set_transform(
            id,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 0.0, -4.0),
            },
        );
        renderer
            .set_geometry_instance_settings(id, geometry_instance_settings_for_node(&sorted, nid));
    }
    let vp = Viewport3D::new(32, 32);
    let order = SoftwareRenderer3D::transparency_draw_order(renderer.instances(), &vp);
    let ids: Vec<u64> = order.iter().map(|i| i.id.0).collect();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort_unstable();
    assert_eq!(
        ids, sorted_ids,
        "an all-opaque scene must come back in creation order regardless of \
         sorting_offset"
    );
}

/// The five properties this bead adds all reach `GeometryInstanceSettings`, and
/// a node that authors none of them keeps Godot's defaults.
///
/// ⚠️ `sorting_use_aabb_center` defaults to **true** and `gi_mode` to
/// **`Static`** (`gi_mode` is documented `default="1"`, not 0). Both are values
/// that a reasonable guess gets wrong in the direction that fails silently: an
/// absent `sorting_use_aabb_center` read as `false` would move the sort point of
/// every mesh in every existing scene, and `gi_mode` read as `Disabled` would
/// drop every mesh out of a bake.
#[test]
fn the_new_properties_default_to_godots_values() {
    let (plain, _) = load(PLAIN_SCENE);
    let g = geometry_instance_settings_for_node(&plain, find(&plain, "B"));

    assert_eq!(g.sorting_offset, 0.0);
    assert!(
        g.sorting_use_aabb_center,
        "defaults to TRUE (visual_instance_3d.h:44) — the doc XML lists no \
         default for this member at all"
    );
    assert_eq!(
        g.gi_mode,
        GiMode::Static,
        "gi_mode is documented default=\"1\""
    );
    assert_eq!(g.gi_lightmap_texel_scale, 1.0);
    assert!(!g.ignore_occlusion_culling);
}

/// An authored value reaches the settings for each of the five.
#[test]
fn the_new_properties_are_read_off_the_node() {
    let src = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="M" type="MeshInstance3D" parent="."]
sorting_offset = -2.5
sorting_use_aabb_center = false
gi_mode = 2
gi_lightmap_texel_scale = 4.0
ignore_occlusion_culling = true
"#;
    let (tree, _) = load(src);
    let g = geometry_instance_settings_for_node(&tree, find(&tree, "M"));

    assert_eq!(g.sorting_offset, -2.5);
    assert!(!g.sorting_use_aabb_center);
    assert_eq!(g.gi_mode, GiMode::Dynamic, "gi_mode = 2 is DYNAMIC");
    assert_eq!(g.gi_lightmap_texel_scale, 4.0);
    assert!(g.ignore_occlusion_culling);
}

/// ⚠️ Surface 0 must keep using the `__material__*` companion it always used.
///
/// The indexed companions are a SEPARATE prefix on purpose: surface 0's body is
/// rebuilt with `strip_prefix("__material__")`, so storing surface 1 as
/// `__material__1__albedo_color` would be harvested into surface 0's
/// sub-resource as a property named `1__albedo_color` — corrupting the material
/// that already worked in order to add the one that did not.
#[test]
fn the_indexed_companions_do_not_leak_into_surface_zero() {
    let (two, _) = load(TWO_SURFACE_SCENE);
    let mesh = find(&two, "Mesh");
    let node = two.get_node(mesh).expect("mesh node");

    // Surface 0 still carries its own red material under the original prefix.
    match node.get_property("__material__albedo_color") {
        Variant::Color(c) => assert!(
            c.r > 0.9 && c.b < 0.1,
            "surface 0 must still be the RED material, got {c:?}"
        ),
        other => panic!("surface 0 keeps the __material__ companion, got {other:?}"),
    }

    // And nothing under `__material__` is an indexed key.
    let leaked: Vec<String> = node
        .properties()
        .filter_map(|(k, _)| {
            let rest = k.strip_prefix("__material__")?;
            rest.starts_with(|c: char| c.is_ascii_digit())
                .then(|| k.clone())
        })
        .collect();
    assert!(
        leaked.is_empty(),
        "no indexed key may live under __material__, found {leaked:?}"
    );
}
