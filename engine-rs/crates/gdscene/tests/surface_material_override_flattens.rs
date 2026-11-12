//! pat-s3rrv.30 — a plain `StandardMaterial3D` on `surface_material_override/0`
//! flattens onto the node.
//!
//! # Why this path specifically
//!
//! The bead's acceptance test (`material_transparency_modes_render`) opens with
//! a Case 0 that asserts the authored material reaches
//! `Render3DItem.material`. Its recorded failure is `left None, right
//! Some(Alpha)` — and **`None` is ambiguous**: `item_transparency` does
//! `items.iter().find_map(|i| i.material.as_ref())`, which yields `None` both
//! when the items carry no material *and* when there are no items at all.
//!
//! A sibling in-file test now covers the `material_override` slot (the gap
//! MagentaBrook closed). The acceptance fixture attaches its material through
//! `surface_material_override/0` instead, and that slot had no equivalent
//! regression test. So this pins the flattening half on its own: if it passes
//! while the acceptance still reports `None`, the fault is downstream — in
//! `material3d_for_node` or in item collection — and not in the `.tscn` → node
//! chain.
//!
//! ⚠️ Asserting `__material__class` and a render-mode key, not `albedo_color`.
//! The colour is special-cased and surfaced on its own, so a resolver that got
//! far enough to copy the colour but not the property bag would pass a
//! colour-only assertion while every render mode, metallic and roughness value
//! stayed dropped — which is the symptom this bead exists for.

use gdscene::packed_scene::PackedScene;
use gdvariant::Variant;

const TSCN: &str = r#"[gd_scene format=3]

[sub_resource type="BoxMesh" id="Box_1"]

[sub_resource type="StandardMaterial3D" id="Mat_1"]
albedo_color = Color(0, 0, 1, 1)
transparency = 1
cull_mode = 1

[node name="World" type="Node3D"]

[node name="Glass" type="MeshInstance3D" parent="."]
mesh = SubResource("Box_1")
surface_material_override/0 = SubResource("Mat_1")
"#;

#[test]
fn surface_material_override_flattens_a_plain_standard_material() {
    let scene = PackedScene::from_tscn(TSCN).expect("scene parses");
    let nodes = scene.instance().expect("scene instances");
    let node = nodes
        .iter()
        .find(|n| n.name() == "Glass")
        .expect("the MeshInstance3D is in the instanced scene");

    assert_eq!(
        node.get_property("__material__class"),
        Variant::String("StandardMaterial3D".into()),
        "surface_material_override/0 must be resolved by the __material__ pass. \
         Nil here means the node reached the tree with no material companions, \
         so `material3d_for_node` answers None and the mesh renders unmaterialed"
    );
}

#[test]
fn the_render_modes_travel_with_it_not_just_the_colour() {
    let scene = PackedScene::from_tscn(TSCN).expect("scene parses");
    let nodes = scene.instance().expect("scene instances");
    let node = nodes.iter().find(|n| n.name() == "Glass").unwrap();

    // transparency = 1 is BaseMaterial3D.TRANSPARENCY_ALPHA, which is exactly
    // what the acceptance's Case 0 expects to survive the trip.
    assert_eq!(
        node.get_property("__material__transparency"),
        Variant::Int(1),
        "the authored transparency must travel on the node; by render time the \
         PackedScene and its sub-resource table are gone, so anything the \
         renderer needs has to be flattened here"
    );
    assert_eq!(
        node.get_property("__material__cull_mode"),
        Variant::Int(1),
        "...and so must the other render modes — the full property bag is \
         copied, not a hand-picked subset"
    );
}
