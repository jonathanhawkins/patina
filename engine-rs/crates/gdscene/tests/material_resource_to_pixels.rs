//! pat-0q2kx.42 — an authored `StandardMaterial3D` changes the pixels a real
//! renderer produces, not just the data a struct holds.
//!
//! Godot resolves a MeshInstance3D's material through
//! `material_override > surface_material_override/N > mesh surface material`
//! and feeds the full parameter set to the shader
//! (`scene/resources/material.cpp`).
//!
//! Patina parsed the scene and could render a material, but nothing joined the
//! two: `packed_scene` copied only `albedo_color` off the material sub-resource,
//! so metallic, roughness, emission and every texture reference were gone before
//! anything could draw them. Every authored surface arrived as a flat colour.
//! Nothing errored — the mesh simply rendered as though the material had never
//! been written.
//!
//! This drives the real chain: `.tscn` -> `PackedScene` -> `__material__*` node
//! companions -> `RenderServer3DAdapter::resolve_material` -> `SoftwareRenderer3D`
//! -> pixels, and asserts the frame actually differs.
//!
//! Emission is the discriminator rather than metallic/roughness because it is
//! additive and therefore visible without depending on the scene's lighting rig
//! — a metallic difference needs a light in the right place to show up at all,
//! which would make a failure ambiguous between "material dropped" and "lit
//! badly".

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;

const W: u32 = 64;
const H: u32 = 48;

/// A camera looking down -Z at a box sitting on the origin. `material` is spliced
/// into the StandardMaterial3D sub-resource so both variants are byte-identical
/// apart from the material parameters under test.
fn scene_with_material(material_lines: &str) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="BoxMesh" id="Mesh_1"]

[sub_resource type="StandardMaterial3D" id="Mat_1"]
albedo_color = Color(0.5, 0.5, 0.5, 1)
{material_lines}

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 4)

[node name="Mesh" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)
mesh = SubResource("Mesh_1")
surface_material_override/0 = SubResource("Mat_1")
"#
    )
}

fn render(tscn: &str) -> Vec<gdcore::math::Color> {
    gdobject::class_db::register_3d_classes();

    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let mut adapter = RenderServer3DAdapter::new(W, H);
    let (snapshot, _data) = adapter.render_frame(&tree);
    assert!(
        snapshot.visible_mesh_count > 0,
        "the mesh must reach the renderer at all — 0 visible meshes means the \
         scene setup failed, not that materials are broken"
    );
    adapter
        .last_frame()
        .expect("a rendered frame")
        .pixels
        .clone()
}

/// **The acceptance test.** An emissive material paints different pixels than a
/// non-emissive one built from the identical scene.
#[test]
fn material_resource_to_pixels() {
    let flat = render(&scene_with_material(""));
    let emissive = render(&scene_with_material(
        "emission_enabled = true\nemission = Color(1, 0, 0, 1)",
    ));

    assert_eq!(
        flat.len(),
        emissive.len(),
        "same viewport, same pixel count"
    );

    let differing = flat
        .iter()
        .zip(emissive.iter())
        .filter(|(a, b)| {
            (a.r - b.r).abs() > 1e-3 || (a.g - b.g).abs() > 1e-3 || (a.b - b.b).abs() > 1e-3
        })
        .count();

    // ⚠️ The whole bead in one assertion: the authored material reached the
    // renderer. Zero differing pixels means `emission` was dropped somewhere
    // between the .tscn and the shader — which is exactly the state this bead
    // was filed for.
    assert!(
        differing > 0,
        "an emissive StandardMaterial3D must render differently from a \
         non-emissive one; {differing} of {} pixels differed",
        flat.len()
    );
}

/// The emission gate is honoured through the whole chain, not just in the
/// parser: naming an emission colour WITHOUT `emission_enabled` must render
/// identically to no emission at all, as it does in Godot.
#[test]
fn emission_without_its_enable_flag_changes_nothing() {
    let flat = render(&scene_with_material(""));
    let colour_only = render(&scene_with_material("emission = Color(1, 0, 0, 1)"));

    let differing = flat
        .iter()
        .zip(colour_only.iter())
        .filter(|(a, b)| {
            (a.r - b.r).abs() > 1e-3 || (a.g - b.g).abs() > 1e-3 || (a.b - b.b).abs() > 1e-3
        })
        .count();

    assert_eq!(
        differing, 0,
        "emission without emission_enabled must not light the surface — \
         {differing} pixels changed"
    );
}
