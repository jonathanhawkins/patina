//! truck_town's minivan (`vehicles/car_base.tscn`) on the imported town drives
//! like real Godot 4.6.1's (pat-qg3nu).
//!
//! Every expected number is Godot's, from a probe run headless at the
//! project's 120 ticks/s (`--fixed-fps 120`, Jolt, physics interpolation off):
//! `town/model/town_model.gltf` at the root, the car under a Node3D carrying
//! town_scene.tscn's `InstancePos` transform, the Body's `vehicle.gd`
//! detached, and `vehicle.gd`'s own control law (throttle
//! `clamp(200 / speed, 0, 100)` under 5 m/s, else 40; steering
//! `move_toward(target, 1.5 * delta)`) driven on a fixed tick schedule. Values
//! are read at the START of a tick, after Godot's `flush_queries` ran
//! `VehicleBody3D::_body_state_changed`: the state a script sees. Tick `t`
//! here is the state after `t` Patina `MainLoop::step`s.
//!
//! Positions are the Body's, local to CarBase (+Z is the car's drive
//! direction). Before pat-qg3nu the car sat on its chassis box at y -0.131
//! (Godot 0.131), never moved under throttle, and the wheel nodes kept their
//! authored pose (Wheel1 y 0.1152; Godot 0.0155).
//!
//! Set `VEHICLE_DUMP=<file>` to write the whole Patina trace.

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const DT: f32 = 1.0 / 120.0;

/// A control schedule, in ticks: settle, throttle, steer to +0.4, steer to
/// -0.4, then steering back to 0.
#[derive(Clone, Copy)]
struct Schedule {
    settle: u32,
    throttle: u32,
    left: u32,
    right: u32,
    coast: u32,
}

impl Schedule {
    fn ticks(&self) -> u32 {
        self.settle + self.throttle + self.left + self.right + self.coast
    }
}

/// Probe 1: full throttle over the ramp at the end of the start road, then
/// the steering sweep while it lands.
const RAMP: Schedule = Schedule {
    settle: 300,
    throttle: 300,
    left: 144,
    right: 144,
    coast: 60,
};
/// Probe 2: a short throttle, then the left turn on the flat road.
const TURN: Schedule = Schedule {
    settle: 300,
    throttle: 120,
    left: 120,
    right: 120,
    coast: 60,
};

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture")
}

struct Car {
    ml: MainLoop,
    body: NodeId,
    wheels: [NodeId; 4],
}

fn named_child(tree: &SceneTree, id: NodeId, name: &str) -> NodeId {
    *tree
        .get_node(id)
        .unwrap()
        .children()
        .iter()
        .find(|&&c| tree.get_node(c).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no child {name}"))
}

fn car_on_town() -> Car {
    gdobject::class_db::register_all_classes();
    gdcore::file_access::set_path_roots(Some(game_dir()), None);
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let town = gdscene::gltf_scene::packed_scene_from_gltf_path("res://town/model/town_model.gltf")
        .expect("town_model.gltf imports");
    gdscene::packed_scene::add_packed_scene_to_tree_with_subscenes(&mut tree, root, &town, &|_| {
        None
    })
    .expect("town instances");

    // town_scene.tscn's InstancePos: `Transform3D(3.57628e-07, 0, -1, 0, 1, 0,
    // 1, 0, 3.57628e-07, ...)` is ROW-major, so its columns are these. The car
    // drives along world -X.
    let pos = tree
        .add_child(root, Node::new("InstancePos", "Node3D"))
        .unwrap();
    let t = Transform3D {
        basis: Basis {
            x: Vector3::new(3.57628e-07, 0.0, 1.0),
            y: Vector3::new(0.0, 1.0, 0.0),
            z: Vector3::new(-1.0, 0.0, 3.57628e-07),
        },
        origin: Vector3::new(35.48, 8.72405, 14.335),
    };
    gdscene::node3d::set_transform_component(
        tree.get_node_mut(pos).unwrap(),
        "transform",
        &Variant::Transform3D(t),
    );

    let car_scene = gdscene::packed_scene::load_scene_from_res("res://vehicles/car_base.tscn")
        .expect("car_base.tscn");
    let car = gdscene::packed_scene::add_packed_scene_to_tree_from_res(&mut tree, pos, &car_scene)
        .expect("car instances");
    let body = named_child(&tree, car, "Body");
    let wheels = ["Wheel1", "Wheel2", "Wheel3", "Wheel4"].map(|n| named_child(&tree, body, n));
    // The probe drives the controls itself, as the Godot one did with the
    // script detached.
    tree.get_node_mut(body)
        .unwrap()
        .set_property("script", Variant::Nil);
    // The Godot probe reported the chassis' contacts the same way; monitoring
    // does not change the simulation.
    tree.get_node_mut(body)
        .unwrap()
        .set_property("contact_monitor", Variant::Bool(true));
    tree.get_node_mut(body)
        .unwrap()
        .set_property("max_contacts_reported", Variant::Int(16));

    let mut ml = MainLoop::new(tree);
    // The project's own settings: 120 ticks/s and `3d/physics_engine="Jolt
    // Physics"`, which selects Jolt's contact model (pat-std5q). Without
    // them the town ran GodotPhysics' solver against Godot's Jolt numbers.
    let text = std::fs::read_to_string(game_dir().join("project.godot")).unwrap();
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).unwrap();
    gdscene::project_runtime::apply_project_settings(&mut ml, &settings);
    assert_eq!(ml.physics_ticks_per_second(), 120);
    ml.register_physics_bodies();
    Car { ml, body, wheels }
}

fn f(tree: &SceneTree, id: NodeId, key: &str) -> f32 {
    match tree.get_node(id).unwrap().get_property(key) {
        Variant::Float(v) => v as f32,
        Variant::Int(v) => v as f32,
        _ => 0.0,
    }
}

fn v3(tree: &SceneTree, id: NodeId, key: &str) -> Vector3 {
    match tree.get_node(id).unwrap().get_property(key) {
        Variant::Vector3(v) => v,
        _ => Vector3::ZERO,
    }
}

/// The state at the start of one tick.
#[derive(Clone, Debug)]
struct Rec {
    pos: Vector3,
    /// The Body's local basis (heading = its Z column).
    basis: Basis,
    speed: f32,
    /// World angular velocity, as the node publishes it.
    ang: Vector3,
    wheel_local: [Transform3D; 4],
    wheel_contact: [bool; 4],
    wheel_rpm: [f32; 4],
    /// The chassis' contacts from the last step: (world position, normal).
    contacts: Vec<(Vector3, Vector3)>,
}

fn body_contacts(tree: &SceneTree, id: NodeId) -> Vec<(Vector3, Vector3)> {
    let Variant::Array(list) = tree.get_node(id).unwrap().get_property("_body_contacts") else {
        return Vec::new();
    };
    list.to_vec()
        .into_iter()
        .filter_map(|c| match c {
            Variant::Dictionary(d) => match (d.get("position"), d.get("normal")) {
                (Some(Variant::Vector3(p)), Some(Variant::Vector3(n))) => Some((p, n)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Runs `s` and returns the record at the start of each tick (index 0 = tick 1).
fn run(s: Schedule) -> Vec<Rec> {
    let mut car = car_on_town();
    let mut out: Vec<Rec> = Vec::new();
    let mut dump = std::env::var("VEHICLE_DUMP")
        .ok()
        .map(|p| std::fs::File::create(p).unwrap());
    let mut steering = 0.0f32;
    for tick in 1..=s.ticks() {
        car.ml.step(DT as f64);
        let tree = car.ml.tree();
        let rec = Rec {
            pos: gdscene::node3d::get_position(tree, car.body),
            basis: gdscene::node3d::get_local_transform(tree, car.body).basis,
            speed: v3(tree, car.body, "linear_velocity").length(),
            ang: v3(tree, car.body, "angular_velocity"),
            wheel_local: car
                .wheels
                .map(|w| gdscene::node3d::get_local_transform(tree, w)),
            wheel_contact: car.wheels.map(|w| {
                tree.get_node(w).unwrap().get_property("_wheel_in_contact") == Variant::Bool(true)
            }),
            wheel_rpm: car.wheels.map(|w| f(tree, w, "_wheel_rpm")),
            contacts: body_contacts(tree, car.body),
        };
        if let Some(d) = dump.as_mut() {
            use std::io::Write;
            let w = &rec.wheel_local[0];
            let g = gdscene::node3d::get_global_transform(tree, car.body).origin;
            writeln!(
                d,
                "{tick} p=({:.4},{:.4},{:.4}) g=({:.4},{:.4},{:.4}) sp={:.3} w=({:.4},{:.4},{:.4}) bz=({:.3},{:.3},{:.3}) w1=({:.4},{:.4},{:.4}) c={:?}",
                rec.pos.x, rec.pos.y, rec.pos.z, g.x, g.y, g.z, rec.speed, rec.ang.x, rec.ang.y, rec.ang.z,
                rec.basis.z.x, rec.basis.z.y, rec.basis.z.z,
                w.origin.x, w.origin.y, w.origin.z, rec.wheel_contact,
            )
            .unwrap();
            writeln!(d, "{tick} contacts {:?}", rec.contacts).unwrap();
        }
        let speed = rec.speed;
        out.push(rec);

        // vehicle.gd `_physics_process`, on the schedule.
        let accel = tick > s.settle && tick <= s.settle + s.throttle;
        let left_end = s.settle + s.throttle + s.left;
        let target = if tick > s.settle + s.throttle && tick <= left_end {
            0.4
        } else if tick > left_end && tick <= left_end + s.right {
            -0.4
        } else {
            0.0
        };
        let engine = if !accel {
            0.0
        } else if speed < 5.0 && speed.abs() > 1e-5 {
            (40.0 * 5.0 / speed).clamp(0.0, 100.0)
        } else {
            40.0
        };
        let step = 1.5 * DT;
        steering = if (target - steering).abs() <= step {
            target
        } else {
            steering + step * (target - steering).signum()
        };
        let node = car.ml.tree_mut().get_node_mut(car.body).unwrap();
        node.set_property("engine_force", Variant::Float(engine as f64));
        node.set_property("steering", Variant::Float(steering as f64));
    }
    out
}

fn at(recs: &[Rec], tick: u32) -> &Rec {
    &recs[(tick - 1) as usize]
}

fn assert_near(what: &str, got: f32, godot: f32, tol: f32) {
    assert!(
        (got - godot).abs() <= tol,
        "{what}: Patina {got}, Godot {godot} (tolerance {tol})"
    );
}

fn assert_vec_near(what: &str, got: Vector3, godot: (f32, f32, f32), tol: f32) {
    let d = Vector3::new(got.x - godot.0, got.y - godot.1, got.z - godot.2).length();
    assert!(
        d <= tol,
        "{what}: Patina {got:?}, Godot {godot:?} ({d} apart, tolerance {tol})"
    );
}

/// The car settles on its four springs at Godot's ride height, rolling slowly
/// down the start road, and every wheel node is posed at its suspension drop.
///
/// | tick | Godot Body position | Godot Wheel1 local y |
/// |---|---|---|
/// | 3 | | -0.0336 (one tick after the first contact; tick 2 still reads the authored 0.1152) |
/// | 160 | (-0.0017, 0.1480, 0.1136) | 0.0152 |
/// | 300 | (-0.0017, 0.1307, 0.6184), speed 0.550 | 0.0155, rpm 21.1 |
#[test]
fn minivan_settles_on_its_springs_at_godots_height() {
    let recs = run(Schedule {
        settle: 300,
        throttle: 0,
        left: 0,
        right: 0,
        coast: 0,
    });
    assert_near(
        "Wheel1 y at tick 2 (pose trails the cast)",
        at(&recs, 2).wheel_local[0].origin.y,
        0.1152,
        1e-3,
    );
    assert_near(
        "Wheel1 y at tick 3",
        at(&recs, 3).wheel_local[0].origin.y,
        -0.0336,
        2e-3,
    );
    assert_vec_near(
        "Body at tick 160",
        at(&recs, 160).pos,
        (-0.0017, 0.1480, 0.1136),
        0.01,
    );
    let r = at(&recs, 300);
    assert_vec_near("Body at tick 300", r.pos, (-0.0017, 0.1307, 0.6184), 0.01);
    assert_near("speed at tick 300", r.speed, 0.550, 0.02);
    assert!(
        r.wheel_contact.iter().all(|&c| c),
        "every wheel on the road at tick 300: {:?}",
        r.wheel_contact
    );
    assert_near(
        "Wheel1 y at tick 300",
        r.wheel_local[0].origin.y,
        0.0155,
        0.003,
    );
    // The wheel keeps its authored x/z: only the suspension drop moves it.
    assert_near("Wheel1 x", r.wheel_local[0].origin.x, 0.573678, 1e-4);
    assert_near("Wheel1 z", r.wheel_local[0].origin.z, 1.10416, 1e-4);
    assert_near("Wheel1 rpm at tick 300", r.wheel_rpm[0], 21.1, 1.5);
}

/// Full throttle down the start road: Godot's acceleration curve, including
/// the 0.1/s default damping, then the ramp at its end throws the car.
///
/// | tick | Godot speed | Godot Body position |
/// |---|---|---|
/// | 360 | 4.165 | z 1.9266 |
/// | 420 | 5.995 | z 4.4933 |
/// | 540 | 9.229 | z 12.1454 |
/// | 576 | | the chassis' front edge meets the ramp: two contacts, z 13.8563 and 14.8103 (world), either side of the 14.3333 centre line |
/// | 600 | 8.228 (on the ramp lip; Wheel1 already off it) | (-0.0017, 0.8185, 16.6831) |
/// | 629 | | the rear wheels have left the lip; from 630 all four hang at the rest length (y -0.0348) |
/// | 630 | | (-0.0017, 1.9320, 18.2081), pitching at 0.9108 rad/s |
/// | 660 | | y 2.5023 |
/// | 700 | | x -0.0015 |
///
/// Godot's angular velocity about the world X and Y axes stays under 0.002
/// rad/s from tick 570 to 700: the car pitches over the ramp and neither rolls
/// nor yaws. Before pat-std5q Patina's single off-centre contact gave it up to
/// 0.57 rad/s of roll/yaw, and its chassis box sat 0.077 m to the side of
/// where the car authors it. The speed is checked to 0.2%: it is the
/// velocity Godot publishes BEFORE the wheels' impulses (pat-zljoy), which
/// `vehicle.gd`'s throttle law reads.
#[test]
fn throttle_and_ramp_jump_match_godot() {
    let recs = run(RAMP);
    for (tick, speed, z) in [
        (360, 4.165, 1.9266),
        (420, 5.995, 4.4933),
        (540, 9.229, 12.1454),
    ] {
        let r = at(&recs, tick);
        assert_near(
            &format!("speed at tick {tick}"),
            r.speed,
            speed,
            0.002 * speed,
        );
        assert_near(&format!("z at tick {tick}"), r.pos.z, z, 0.002 * z.max(1.0));
        assert!(
            r.wheel_contact.iter().all(|&c| c),
            "tick {tick}: all wheels on the road"
        );
        assert_near(
            &format!("x at tick {tick} (drives straight)"),
            r.pos.x,
            -0.0017,
            0.005,
        );
    }
    let rpm = at(&recs, 540).wheel_rpm[0];
    assert_near("Wheel1 rpm at tick 540", rpm, 353.8, 0.03 * 353.8);

    // The ramp: the chassis box's front edge meets the ramp face through a
    // TWO-point manifold, one contact at each end of the edge. Godot reports
    // the pair from tick 576 at x 19.3152, 19.2442 at tick 577: Jolt finds a
    // contact from the positions BEFORE its step's motion (0.075 m at this
    // speed), so Patina's first contact, found after the motion, sits where
    // Godot's tick-577 one does.
    let c = &at(&recs, 576).contacts;
    assert_eq!(
        c.len(),
        2,
        "tick 576: the chassis touches the ramp at {c:?}; Godot reports 2 contacts"
    );
    let mut zs: Vec<f32> = c.iter().map(|(p, _)| p.z).collect();
    zs.sort_by(f32::total_cmp);
    assert_near("near contact z (world)", zs[0], 13.8563, 0.005);
    assert_near("far contact z (world)", zs[1], 14.8103, 0.005);
    for (p, n) in c {
        assert_near("contact x (world, Godot's tick 577)", p.x, 19.2442, 0.01);
        assert_vec_near(
            "contact normal (the ramp face)",
            *n,
            (0.4926, 0.8702, 0.0),
            0.002,
        );
    }
    for tick in 570..=700 {
        let w = at(&recs, tick).ang;
        assert!(
            w.x.abs() < 0.01 && w.y.abs() < 0.01,
            "tick {tick}: the car rolls/yaws at {w:?}; Godot's stays under 0.002 rad/s about X and Y"
        );
    }
    let r = at(&recs, 600);
    assert_vec_near("Body at tick 600", r.pos, (-0.0017, 0.8185, 16.6831), 0.04);
    assert!(
        !r.wheel_contact[0],
        "Wheel1 has left the ramp at tick 600 (Godot)"
    );
    assert!(
        at(&recs, 628).wheel_contact[1],
        "the rear wheels are still on the lip at tick 628 (Godot)"
    );
    let r = at(&recs, 630);
    assert_vec_near(
        "Body at tick 630 (airborne)",
        r.pos,
        (-0.0017, 1.9320, 18.2081),
        0.05,
    );
    assert_near("pitch rate at tick 630", r.ang.z, 0.9108, 0.06);
    assert_near(
        "y at tick 660 (top of the jump)",
        at(&recs, 660).pos.y,
        2.5023,
        0.08,
    );
    assert_near(
        "x at tick 700 (still on the centre line)",
        at(&recs, 700).pos.x,
        -0.0015,
        0.01,
    );
    assert!(
        !at(&recs, 629).wheel_contact.iter().any(|&c| c),
        "every wheel airborne at tick 629 (Godot)"
    );
    for tick in [630, 640] {
        let r = at(&recs, tick);
        for (i, w) in r.wheel_local.iter().enumerate() {
            assert!(
                !r.wheel_contact[i],
                "wheel {} airborne at tick {tick}",
                i + 1
            );
            assert_near(
                &format!("wheel {} hangs at its rest length", i + 1),
                w.origin.y,
                -0.0348,
                1e-3,
            );
        }
    }
}

/// The left turn on the flat road: heading and path follow Godot's, and the
/// steering wheels are posed at the steering angle.
///
/// Godot: steering reaches 0.4 at tick 453; at tick 480 the front wheels'
/// local X axis is (0.921, 0, -0.389) (`Basis(up, 0.4)`), the rear wheels'
/// stays (1, 0, 0).
///
/// | tick | Godot Body position | Godot heading (Body basis Z) | speed |
/// |---|---|---|---|
/// | 480 | (0.4622, 0.1252, 7.3117) | (0.3831, 0.0162, 0.9236) | 5.396 |
/// | 540 | (2.1388, 0.1231, 9.1530) | (0.8292, 0.0163, 0.5587) | 4.756 |
#[test]
fn steering_turns_like_godot() {
    let recs = run(TURN);
    let r = at(&recs, 480);
    assert_vec_near("Body at tick 480", r.pos, (0.4622, 0.1252, 7.3117), 0.02);
    assert_vec_near(
        "heading at tick 480",
        r.basis.z,
        (0.3831, 0.0162, 0.9236),
        0.01,
    );
    assert_near("speed at tick 480", r.speed, 5.396, 0.03);
    for (i, steers) in [(0, true), (1, false), (2, true), (3, false)] {
        let x = r.wheel_local[i].basis.x;
        let want = if steers {
            (0.921, 0.0, -0.389)
        } else {
            (1.0, 0.0, 0.0)
        };
        assert_vec_near(
            &format!("wheel {} axle at steering 0.4", i + 1),
            x,
            want,
            0.01,
        );
    }
    let r = at(&recs, 540);
    assert_vec_near("Body at tick 540", r.pos, (2.1388, 0.1231, 9.1530), 0.03);
    assert_vec_near(
        "heading at tick 540",
        r.basis.z,
        (0.8292, 0.0163, 0.5587),
        0.015,
    );
    assert_near("speed at tick 540", r.speed, 4.756, 0.03);
    // pat-std5q: the right wheels drop off the road edge here and the
    // chassis box lands on it; Godot's car is at (3.5563, -0.0061, 9.7741).
    assert_vec_near(
        "Body at tick 580",
        at(&recs, 580).pos,
        (3.5563, -0.0061, 9.7741),
        0.04,
    );
}

/// The wheels SPIN: the pose's Y axis turns about the axle by the integrated
/// `m_deltaRotation`, as Godot's `Basis(right, m_rotation)` does. Godot's
/// Wheel1 at tick 420 (straight-line throttle, no steering): basis Y
/// (0, 0.8798, -0.4753) after 3.9 m of travel. Spin accumulates over the whole
/// run, so a small speed difference shows up here as an angle.
#[test]
fn wheels_spin_like_godot() {
    let recs = run(Schedule {
        settle: 300,
        throttle: 120,
        left: 0,
        right: 0,
        coast: 0,
    });
    let y = at(&recs, 420).wheel_local[0].basis.y;
    let godot = Vector3::new(0.0, 0.8798, -0.4753);
    let angle = y.normalized().dot(godot).clamp(-1.0, 1.0).acos();
    assert!(
        angle < 0.2,
        "Wheel1 spin pose {y:?} is {angle} rad from Godot's {godot:?}"
    );
    // And it is not still at rest: the authored pose has Y = (0, 1, 0).
    assert!(y.z.abs() > 0.1, "Wheel1 never spun: basis Y {y:?}");
}
