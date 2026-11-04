//! A GPUParticles2D's `scale_curve` shapes each particle's size over its life.
//!
//! Found playing the official Godot demo "Dodge the Creeps": the player's
//! `Trail` emits the ship's own texture with
//! `scale_curve = CurveTexture(Curve(0.5 -> 0.324))`. Only the 3D emitter read
//! the curve, so every 2D particle drew at its spawn scale of 1 — a copy of the
//! ship twice its size (the ship's sprite is scaled 0.5) sat behind it.
//!
//! The emitter below is the demo's, verbatim apart from the texture.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

const TRAIL: &str = r#"[gd_scene format=3]

[sub_resource type="Gradient" id="3"]
colors = PackedColorArray(1, 1, 1, 0.501961, 1, 1, 1, 0)

[sub_resource type="GradientTexture1D" id="4"]
gradient = SubResource("3")

[sub_resource type="Curve" id="5"]
_data = [Vector2(0.00501098, 0.5), 0.0, 0.0, 0, 0, Vector2(0.994989, 0.324), 0.0, 0.0, 0, 0]
point_count = 2

[sub_resource type="CurveTexture" id="6"]
curve = SubResource("5")

[sub_resource type="ParticleProcessMaterial" id="7"]
gravity = Vector3(0, 0, 0)
scale_curve = SubResource("6")
color_ramp = SubResource("4")

[node name="Trail" type="GPUParticles2D"]
amount = 10
speed_scale = 2.0
process_material = SubResource("7")
"#;

fn scales(ml: &MainLoop, id: gdscene::node::NodeId) -> Vec<f32> {
    match ml.tree().get_node(id).unwrap().get_property("_particle_scales") {
        Variant::PackedFloat32Array(v) => v,
        other => panic!("the emitter should publish particle scales, got {other:?}"),
    }
}

#[test]
fn a_2d_emitter_shrinks_its_particles_along_the_scale_curve() {
    let scene = PackedScene::from_tscn(TRAIL).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let trail = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let mut ml = MainLoop::new(tree);
    for _ in 0..40 {
        ml.step(1.0 / 60.0);
    }
    let s = scales(&ml, trail);
    assert!(!s.is_empty(), "the trail is emitting");
    for v in &s {
        assert!(
            (0.30..=0.51).contains(v),
            "every particle is on the 0.5 -> 0.324 curve, not its spawn scale of 1: {s:?}"
        );
    }
    // Particles of different ages have different sizes: the curve is sampled
    // per particle, not once.
    let (min, max) = s
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    assert!(max - min > 0.05, "older particles are smaller: {s:?}");
}
