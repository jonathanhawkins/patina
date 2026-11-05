//! pat-29zst — a rigid body's solved state reaches its NODE one tick after
//! the step that produced it, as in Godot 4.6.1.
//!
//! Godot's `Main::iteration` runs, per physics tick: `PhysicsServer::sync()`,
//! `flush_queries()` (which runs `_integrate_forces` and copies each body's
//! state onto its node), the scene's `_physics_process`, and only then
//! `step()`. The state a step computes is therefore published at the NEXT
//! tick's flush, and a read at the end of a frame sees the body one step old.
//! Godot has no step before the first tick, so after one tick a body still
//! sits where it was authored.
//!
//! The 2D numbers below were measured on Godot 4.6.1 (Godot Physics 2D) with
//! the scene in `SCENE_2D`, 60 ticks/s, `--fixed-fps 60`, reading positions
//! from a `call_deferred` at the end of each frame:
//!
//! ```text
//! END f1 ball=(0.0, 0.0)       faller=(0.0, 100.0)
//! END f2 ball=(0.998333, 0.0)  faller=(0.0, 100.2722)  fv=(0.0, 16.33333)
//! END f3 ball=(1.995003, 0.0)  faller=(0.0, 100.8162)  fv=(0.0, 32.63945)
//! END f4 ball=(2.990011, 0.0)  faller=(0.0, 101.6315)  fv=(0.0, 48.91838)
//! ```
//!
//! The 3D numbers are the official 3D platformer's Enemy1 (Jolt), see
//! `platformer_3d_movement_matches_godot.rs`.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;
use gdvariant::Variant;

const SCENE_2D: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="CircleShape2D" id="c1"]
radius = 4.0

[node name="Main" type="Node2D"]

[node name="Ball" type="RigidBody2D" parent="."]
gravity_scale = 0.0
linear_velocity = Vector2(60, 0)

[node name="Shape" type="CollisionShape2D" parent="Ball"]
shape = SubResource("c1")

[node name="Faller" type="RigidBody2D" parent="."]
position = Vector2(0, 100)

[node name="Shape" type="CollisionShape2D" parent="Faller"]
shape = SubResource("c1")
"#;

fn boot(tscn: &str) -> MainLoop {
    gdobject::class_db::register_all_classes();
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = gdscene::packed_scene::instance_packed_scene_detached(
        &mut tree,
        &packed,
        &gdscene::packed_scene::load_scene_from_res,
    )
    .unwrap();
    let node = tree.take_node(id).unwrap();
    tree.add_child(root, node).unwrap();
    let mut ml = MainLoop::new(tree);
    // Godot's implicit 2D space damping, which Patina applies only when a
    // project states it (see `apply_project_settings`).
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(
        "[physics]\n\n2d/default_linear_damp=0.1\n2d/default_angular_damp=1.0\n",
    )
    .unwrap();
    gdscene::project_runtime::apply_project_settings(&mut ml, &settings);
    ml.set_physics_ticks_per_second(60);
    ml.register_physics_bodies();
    ml
}

fn pos2(ml: &MainLoop, path: &str) -> Vector2 {
    let id = ml.tree().get_node_by_path(path).unwrap();
    gdscene::node2d::get_position(ml.tree(), id)
}

fn vel2(ml: &MainLoop, path: &str) -> Vector2 {
    let id = ml.tree().get_node_by_path(path).unwrap();
    match ml
        .tree()
        .get_node(id)
        .unwrap()
        .get_property("linear_velocity")
    {
        Variant::Vector2(v) => v,
        other => panic!("{path} linear_velocity: {other:?}"),
    }
}

fn near(a: Vector2, g: [f32; 2], tol: f32, what: &str) {
    assert!(
        (a.x - g[0]).abs() <= tol && (a.y - g[1]).abs() <= tol,
        "{what}: Patina {a:?}, Godot {g:?}"
    );
}

#[test]
fn rigid_body_2d_reads_one_step_old_at_frame_end() {
    let mut ml = boot(SCENE_2D);
    let godot = [
        // (ball, faller, faller velocity)
        ([0.0, 0.0], [0.0, 100.0], None),
        ([0.998333, 0.0], [0.0, 100.2722], Some([0.0, 16.33333])),
        ([1.995003, 0.0], [0.0, 100.8162], Some([0.0, 32.63945])),
        ([2.990011, 0.0], [0.0, 101.6315], Some([0.0, 48.91838])),
    ];
    for (i, (ball, faller, fv)) in godot.iter().enumerate() {
        ml.step(1.0 / 60.0);
        let f = i + 1;
        near(
            pos2(&ml, "/root/Main/Ball"),
            *ball,
            1e-3,
            &format!("f{f} ball"),
        );
        near(
            pos2(&ml, "/root/Main/Faller"),
            *faller,
            1e-3,
            &format!("f{f} faller"),
        );
        if let Some(v) = fv {
            near(
                vel2(&ml, "/root/Main/Faller"),
                *v,
                1e-3,
                &format!("f{f} faller velocity"),
            );
        }
    }
}
