//! A `RigidBody3D` turns about its centre of mass, and with Godot's default
//! `center_of_mass_mode` (AUTO) that is its shapes' centroid, not the body
//! origin (pat-zljoy).
//!
//! Godot 4.6.1 numbers, from a probe on truck_town's project settings (Jolt,
//! 120 ticks/s, default damping 0.1): a unit `BoxShape3D` whose
//! `CollisionShape3D` sits at (0, 0, 1), gravity_scale 0, spun at
//! `angular_velocity = (0, 2, 0)` in free flight. Values are read at the
//! start of each tick (`_physics_process`), tick 1 being before any step.
//!
//! | tick | AUTO body origin (spawned at (0, 50, 0)) | CUSTOM (at the origin), spawned at (10, 50, 0) | rotation (both) |
//! |---|---|---|---|
//! | 2 | (-0.0167, 50, 0.0001) | (10, 50, 0) | (0, 0.0083, 0, 1) |
//! | 60 | (-0.8187, 50, 0.4258) | (10, 50, 0) | (0, 0.4614, 0, 0.8872) |
//! | 120 | (-0.9503, 50, 1.3113) | (10, 50, 0) | (0, 0.8097, 0, 0.5868) |
//!
//! Both report `linear_velocity` (0, 0, 0): it is the centre of mass's
//! velocity, and the box spins in place.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture")
}

fn spinning_box(tree: &mut SceneTree, name: &str, at: Vector3, custom: bool) -> NodeId {
    let root = tree.root_id();
    let mut body = Node::new(name, "RigidBody3D");
    body.set_property("gravity_scale", Variant::Float(0.0));
    body.set_property(
        "angular_velocity",
        Variant::Vector3(Vector3::new(0.0, 2.0, 0.0)),
    );
    if custom {
        body.set_property("center_of_mass_mode", Variant::Int(1));
    }
    let body = tree.add_child(root, body).unwrap();
    let mut cs = Node::new("CollisionShape3D", "CollisionShape3D");
    cs.set_property("shape", Variant::String("BoxShape3D".into()));
    cs.set_property("size", Variant::Vector3(Vector3::new(1.0, 1.0, 1.0)));
    let cs = tree.add_child(body, cs).unwrap();
    gdscene::node3d::set_position(tree, cs, Vector3::new(0.0, 0.0, 1.0));
    gdscene::node3d::set_position(tree, body, at);
    body
}

#[test]
fn an_offset_shape_spins_about_its_own_centre_like_godot() {
    gdobject::class_db::register_all_classes();
    gdcore::file_access::set_path_roots(Some(game_dir()), None);
    let mut tree = SceneTree::new();
    let auto = spinning_box(&mut tree, "Auto", Vector3::new(0.0, 50.0, 0.0), false);
    let custom = spinning_box(&mut tree, "Custom", Vector3::new(10.0, 50.0, 0.0), true);
    let mut ml = MainLoop::new(tree);
    let text = std::fs::read_to_string(game_dir().join("project.godot")).unwrap();
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).unwrap();
    gdscene::project_runtime::apply_project_settings(&mut ml, &settings);
    ml.register_physics_bodies();

    let mut at = std::collections::HashMap::new();
    for tick in 1..=120u32 {
        ml.step(1.0 / 120.0);
        let t = ml.tree();
        let pos = |id| gdscene::node3d::get_global_transform(t, id).origin;
        let q = gdscene::node3d::get_global_rotation_quaternion(t, auto);
        let lin = match t.get_node(auto).unwrap().get_property("linear_velocity") {
            Variant::Vector3(v) => v,
            _ => Vector3::ZERO,
        };
        at.insert(tick, (pos(auto), pos(custom), q, lin));
    }
    let near = |what: &str, p: Vector3, w: (f32, f32, f32), tol: f32| {
        let d = (p - Vector3::new(w.0, w.1, w.2)).length();
        assert!(d <= tol, "{what}: Patina {p:?}, Godot {w:?} ({d} apart)");
    };
    for (tick, godot_auto, quat_y) in [
        (2, (-0.0167, 50.0, 0.0001), 0.0083),
        (60, (-0.8187, 50.0, 0.4258), 0.4614),
        (120, (-0.9503, 50.0, 1.3113), 0.8097),
    ] {
        let (a, c, q, lin) = at[&tick];
        near(
            &format!("AUTO body origin at tick {tick}"),
            a,
            godot_auto,
            0.01,
        );
        near(
            &format!("CUSTOM body origin at tick {tick}"),
            c,
            (10.0, 50.0, 0.0),
            1e-4,
        );
        assert!(
            (q.y.abs() - quat_y).abs() < 0.003,
            "tick {tick}: rotation {q:?}; Godot y {quat_y}"
        );
        assert!(
            lin.length() < 1e-4,
            "tick {tick}: linear_velocity {lin:?} is the centre of mass's; Godot 0"
        );
    }
}
