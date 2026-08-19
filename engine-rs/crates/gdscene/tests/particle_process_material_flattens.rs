//! pat-s3rrv.39 (slice 1): a `ParticleProcessMaterial` sub-resource reaches the
//! emitter.
//!
//! Godot keeps a particle system's motion on a `ParticleProcessMaterial`
//! assigned to `process_material` — `gravity`, `direction`, `spread`,
//! `initial_velocity_min/max` and `color` all live on the resource, not the node.
//!
//! `MainLoop`'s emitter setup already reads every one of those as a PLAIN node
//! property; its own comment says the flattener "lands [them] on the node ... so
//! they are read the same way as every other material parameter". Nothing ever
//! produced them. `packed_scene` had flatteners for `material_override`,
//! `surface_material_override/0`, `physics_material_override`, SpriteFrames and
//! TileSet — but none for `process_material` — so a Godot-authored particle
//! scene parsed cleanly and then emitted with default motion: no gravity, no
//! aimed direction.
//!
//! These pin the flattening. The scene text is exactly the shape Godot writes.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// The `.tscn` the acceptance criteria describe, verbatim in shape.
const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="ParticleProcessMaterial" id="ParticleProcessMaterial_1"]
gravity = Vector3(0, 400, 0)
initial_velocity_min = 120.0
initial_velocity_max = 120.0
direction = Vector3(0, 1, 0)
spread = 0.0

[node name="Emitter" type="GPUParticles2D"]
process_material = SubResource("ParticleProcessMaterial_1")
amount = 16
"#;

fn emitter_props(tscn: &str) -> Vec<(String, Variant)> {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node_id = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let node = tree.get_node(node_id).expect("emitter exists");
    [
        "gravity",
        "direction",
        "spread",
        "initial_velocity_min",
        "initial_velocity_max",
    ]
    .iter()
    .map(|k| (k.to_string(), node.get_property(k)))
    .collect()
}

fn get<'a>(props: &'a [(String, Variant)], key: &str) -> &'a Variant {
    &props.iter().find(|(k, _)| k == key).expect("key present").1
}

/// **The core of the bead.** The material's `gravity` lands on the node, where
/// the emitter reads it. Before this the property was `Nil` and the emitter fell
/// back to its default.
#[test]
fn gravity_from_the_material_reaches_the_node() {
    let props = emitter_props(SCENE);
    match get(&props, "gravity") {
        Variant::Vector3(g) => {
            assert!(
                (g.y - 400.0).abs() < 0.001,
                "gravity.y must be the material's 400, got {g:?}"
            );
            assert!(g.x.abs() < 0.001 && g.z.abs() < 0.001, "got {g:?}");
        }
        other => panic!(
            "gravity must be flattened onto the emitter as a Vector3; got \
             {other:?}. Nil means `process_material` was parsed and dropped — \
             the bug this fixes."
        ),
    }
}

/// The rest of the motion parameters land too, not just gravity.
#[test]
fn direction_spread_and_velocity_reach_the_node() {
    let props = emitter_props(SCENE);

    match get(&props, "direction") {
        Variant::Vector3(d) => assert!(
            (d.y - 1.0).abs() < 0.001,
            "direction must be the material's (0,1,0), got {d:?}"
        ),
        other => panic!("direction must be flattened; got {other:?}"),
    }
    match get(&props, "spread") {
        Variant::Float(f) => assert!(f.abs() < 0.001, "spread must be 0.0, got {f}"),
        Variant::Int(i) => assert_eq!(*i, 0),
        other => panic!("spread must be flattened; got {other:?}"),
    }
    for key in ["initial_velocity_min", "initial_velocity_max"] {
        match get(&props, key) {
            Variant::Float(f) => assert!(
                (f - 120.0).abs() < 0.001,
                "{key} must be the material's 120.0, got {f}"
            ),
            other => panic!("{key} must be flattened; got {other:?}"),
        }
    }
}

/// A value authored directly on the NODE is not clobbered by the material.
///
/// This is the guard on the flattener's precedence: it must fill gaps, not
/// overwrite. Without it, every existing hand-built particle test that sets
/// `gravity` on the node would silently start reading the material instead.
#[test]
fn a_node_property_is_not_overwritten_by_the_material() {
    const OVERRIDDEN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="ParticleProcessMaterial" id="ParticleProcessMaterial_1"]
gravity = Vector3(0, 400, 0)

[node name="Emitter" type="GPUParticles2D"]
process_material = SubResource("ParticleProcessMaterial_1")
gravity = Vector3(0, -9, 0)
"#;
    let props = emitter_props(OVERRIDDEN);
    match get(&props, "gravity") {
        Variant::Vector3(g) => assert!(
            (g.y + 9.0).abs() < 0.001,
            "the node's own gravity must win over the material's, got {g:?}"
        ),
        other => panic!("expected the node's Vector3, got {other:?}"),
    }
}

/// An emitter with no `process_material` is untouched — the flattener must not
/// invent properties for scenes that never asked for them.
#[test]
fn an_emitter_without_a_material_is_unchanged() {
    const BARE: &str = r#"[gd_scene format=3]

[node name="Emitter" type="GPUParticles2D"]
amount = 8
"#;
    let props = emitter_props(BARE);
    assert!(
        matches!(get(&props, "gravity"), Variant::Nil),
        "no process_material means no flattened gravity"
    );
}
