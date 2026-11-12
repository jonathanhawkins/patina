//! pat-mgixk.81 acceptance: a `blend_shape` track authored in a `.tscn` drives
//! the node's morph weight.
//!
//! # What was wrong
//!
//! Two independent faults, so a scene-authored blend_shape track moved nothing:
//!
//! 1. **The keys were never loaded.** Godot stores blend_shape keys as a FLAT
//!    `PackedFloat32Array` of `(time, transition, value)` triples —
//!    `BLEND_SHAPE_TRACK_SIZE = 3` (`upstream/godot/scene/resources/animation.cpp:228-243`).
//!    `animation_tscn.rs`'s track-type match routed only Position3D/Scale3D/
//!    Rotation3D to the flat-array filler; BlendShape fell through to
//!    `fill_dictionary_track`, which returns immediately on anything that is not
//!    a Dictionary. The track ended up with zero keyframes and was dropped by the
//!    `is_empty()` check at the end of the builder — silently.
//! 2. **The property name did not match the consumer.** The track path is
//!    `MeshInstance3D:ShapeName`, so the subname is the bare shape name
//!    (`Smile`), while the renderer reads `blend_shapes/<name>`
//!    (`render_server_3d.rs`). Even a correctly-keyed track would have written to
//!    a property nothing consults.
//!
//! # The path driven
//!
//! ```text
//! .tscn -> add_packed_scene_to_tree -> attach_scene_animation_players
//!   -> MainLoop::step -> SceneTree::process_animations
//!     -> node `blend_shapes/Smile` -> deformed_mesh_for_node (morph)
//! ```
//!
//! The `.tscn` shape, the `instance()` helper and the step cadence are copied
//! from `animation_player_godot46_library_properties_load.rs`, and the morph
//! assertion from `mesh_deformation_morph_then_skin.rs` — both green at HEAD, so
//! a failure here is about blend_shape TRACKS, not about scene loading, the
//! AnimationPlayer, or morphing.
//!
//! # `blend_shapes/<name>` is an established convention, not a new one
//!
//! The glTF animation importer already builds its blend-shape tracks with
//! `format!("blend_shapes/morph_{slot}")` (`animation.rs:2170`) — glTF morph
//! targets are unnamed, hence the index — and `render_server_3d` falls back to
//! the same `morph_<k>` spelling when a mesh's `blend_shape_names` entry is not
//! a String. So the `.tscn` loader prefixing its subname is bringing the one
//! producer that lacked the convention into line with the other, rather than
//! choosing a spelling.
//!
//! That also bounds the blast radius to nothing: a `.tscn` blend_shape track
//! currently loads ZERO keys and is dropped, so no existing test can be
//! asserting its property path. The glTF producer is untouched.
//!
//! # AnimationTree output needs no separate change
//!
//! The bead also asks to "handle AnimationTree output, which uses the same
//! property path". It needs nothing: `animation_tree.rs:1563` clones
//! `track.property_path` verbatim into its sampled triples and writes each
//! `(property_path, value)` onto the node (`:1591`). So prefixing the path once
//! where the track is BUILT carries through the tree path for free — there is no
//! second place that spells the property name. Checked rather than assumed,
//! because "also handle X" reads like a second edit and is not one here.

use gdcore::math::Vector3;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::deformed_mesh_for_node;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdserver3d::mesh::{BlendShape3D, Mesh3D};
use gdvariant::Variant;

/// 🔑 `keys` is a FLAT float array, not the `{"times": .., "values": ..}`
/// dictionary a value track uses. Stride 3, so these six floats are two keys:
///
/// * `(0, 1, 0)` — t=0, transition 1, weight 0
/// * `(1, 1, 1)` — t=1, transition 1, weight 1
///
/// That is the whole reason the old code dropped the track: it handed this array
/// to the dictionary filler.
const ANIMATION_BODY: &str = r#"resource_name = "smile"
length = 1.0
loop_mode = 0
tracks/0/type = "blend_shape"
tracks/0/imported = false
tracks/0/enabled = true
tracks/0/path = NodePath("Model:Smile")
tracks/0/interp = 1
tracks/0/loop_wrap = true
tracks/0/keys = PackedFloat32Array(0, 1, 0, 1, 1, 1)"#;

fn scene() -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Animation" id="Animation_smile"]
{ANIMATION_BODY}

[sub_resource type="AnimationLibrary" id="AnimationLibrary_main"]
_data = {{
&"smile": SubResource("Animation_smile")
}}

[node name="World" type="Node3D"]

[node name="Model" type="MeshInstance3D" parent="."]

[node name="Anim" type="AnimationPlayer" parent="."]
libraries/ = SubResource("AnimationLibrary_main")
autoplay = "smile"
"#
    )
}

/// One vertex at the origin with a `Smile` shape that lifts it one unit along
/// +Y, so the morphed position reads back as the weight itself.
///
/// A single vertex on purpose, as in `mesh_deformation_morph_then_skin.rs`: the
/// assertion is about WHETHER and HOW MUCH the morph applied, and one vertex
/// makes that a coordinate rather than something inferred from a shape.
fn base_mesh() -> Mesh3D {
    Mesh3D {
        vertices: vec![Vector3::new(0.0, 0.0, 0.0)],
        normals: vec![Vector3::new(0.0, 0.0, 1.0)],
        blend_shapes: vec![BlendShape3D {
            name: "Smile".into(),
            vertex_deltas: vec![Vector3::new(0.0, 1.0, 0.0)],
        }],
        ..Default::default()
    }
}

/// Instances the scene the way a game does. `add_packed_scene_to_tree` is what
/// calls `attach_scene_animation_players`, so the player comes from the scene
/// load rather than from the test.
fn instance() -> MainLoop {
    let parsed = PackedScene::from_tscn(&scene()).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &parsed).expect("the scene instances");
    MainLoop::new(tree)
}

fn model(ml: &MainLoop) -> NodeId {
    ml.tree()
        .get_node_by_path("/root/World/Model")
        .expect("the fixture has /root/World/Model")
}

/// The node's authored morph weight, or `None` when the property was never
/// written at all — which is the pre-bead state and a different failure from
/// "written with the wrong value".
fn weight(ml: &MainLoop) -> Option<f64> {
    match ml
        .tree()
        .get_node(model(ml))
        .expect("Model is in the tree")
        .get_property("blend_shapes/Smile")
    {
        Variant::Float(f) => Some(f),
        Variant::Int(i) => Some(i as f64),
        Variant::Nil => None,
        other => panic!("blend_shapes/Smile should be a number, got {other:?}"),
    }
}

/// **The acceptance.**
#[test]
fn blend_shape_track_scene_load_morphs() {
    let mut ml = instance();

    // -- half the clip: the keyed midpoint reaches the node -------------------
    //
    // Five frames of 0.1s through a 1.0s clip, the cadence the sibling
    // animation test uses.
    for _ in 0..5 {
        ml.step(0.1);
    }

    let mid = weight(&ml).expect(
        "`blend_shapes/Smile` must exist on the node. None is the pre-bead state \
         twice over: the flat key array went to the dictionary filler so the \
         track was dropped with zero keys, AND the property would have been \
         written as the bare subname `Smile` rather than `blend_shapes/Smile`",
    );
    assert!(
        (mid - 0.5).abs() < 0.05,
        "half-way through a 0->1 clip the weight must be ~0.5, got {mid}. A value \
         of 0 means the track exists but its keys did not load; 1 means the \
         sampler is not interpolating",
    );

    // -- and the morph actually moves geometry -------------------------------
    //
    // 🔑 The weight landing on the node is only half the wiring requirement; the
    // renderer has to read it. `deformed_mesh_for_node` is the shared helper the
    // runtime and the editor both morph through, and it returns `None` when
    // there is nothing to deform — which is exactly what an unwritten weight
    // produces.
    let deformed = deformed_mesh_for_node(ml.tree(), model(&ml), &base_mesh())
        .expect("a node carrying a non-zero morph weight must report a deformed mesh");
    let y = deformed.vertices[0].y;
    assert!(
        (y - mid as f32).abs() < 0.05,
        "the morphed vertex must be lifted by the animated weight: expected \
         y ~= {mid}, got {y}. The shape's delta is exactly +1 on Y, so the \
         vertex's y IS the weight the renderer applied — if it is 0 the weight \
         reached the node property but not the morph",
    );
    assert!(
        y > 0.01,
        "and it must differ from the base vertex at the origin, which is the \
         bead's 'morphed vertices differ from base' clause",
    );

    // -- past the end: the weight rests on the last key ----------------------
    for _ in 0..6 {
        ml.step(0.1);
    }
    let end = weight(&ml).expect("the weight is still written at the end of the clip");
    assert!(
        (end - 1.0).abs() < 0.01,
        "at the end of the clip the weight must rest on the last keyframe (1.0), \
         got {end}",
    );
}
