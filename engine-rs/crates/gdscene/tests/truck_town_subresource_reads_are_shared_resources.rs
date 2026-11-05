//! Script-visible inline sub-resources read as real, SHARED Resource objects
//! (pat-nyn3w), on the official truck_town demo (`games/truck_town`,
//! unmodified).
//!
//! A `.tscn` property `environment = SubResource("Environment_dauhq")` is kept
//! on the node as the text `"SubResource:Environment_dauhq"` plus flattened
//! `__subres__<prop>__<key>` companions, which the renderers and the saver
//! read. Scripts were handed the TEXT: `$WorldEnvironment.environment` printed
//! `SubResource:Environment_dauhq`, and the speedometer's
//! `tint_gradient.sample(..)` raised "method not found: 'String.sample'" every
//! frame.
//!
//! Every expected value below was captured from real Godot 4.6.1 running the
//! same scene (`--headless --script`, a probe that instantiates
//! `town/town_scene.tscn` and prints each read with `var_to_str`).

use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::PackedScene;
use gdscene::scripting::GDScriptNodeInstance;
use gdvariant::Variant;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture")
}

/// Boots `scene` the way the runner does (same as
/// `truck_town_car_select_focuses_first_car.rs`).
fn boot(scene: &str) -> MainLoop {
    gdobject::class_db::register_all_classes();
    let dir = game_dir();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let text = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).unwrap();
    gdscene::project_runtime::seed_script_settings_for_this_thread(&settings, &dir);
    let tscn = std::fs::read_to_string(dir.join(scene)).unwrap();
    let packed = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::begin_subscene_session();
    let id = gdscene::packed_scene::instance_packed_scene_detached(
        &mut tree,
        &packed,
        &gdscene::packed_scene::load_scene_from_res,
    )
    .unwrap();
    gdscene::scripting::attach_subtree_scripts_from_res(&mut tree, id);
    let node = tree.take_node(id).unwrap();
    tree.add_child(root, node).unwrap();
    gdscene::packed_scene::finish_attached_scene(&mut tree, id);
    let mut ml = MainLoop::new(tree);
    gdscene::project_runtime::apply_project_settings(&mut ml, &settings);
    ml.register_physics_bodies();
    ml
}

/// A probe script, attached to a fresh node under the town, that reads the
/// scene the way the game's own scripts do.
const PROBE_GD: &str = r#"extends Node

func speedometer() -> Node:
	return get_node("/root/TownScene/CarUI/Speedometer")

func world_env() -> Node:
	return get_node("/root/TownScene/WorldEnvironment")

func gradient_samples(speeds: Array) -> Array:
	var g: Gradient = speedometer().tint_gradient
	var out := []
	for s in speeds:
		out.append(g.sample(remap(s, 0.0, 30.0, 0.0, 1.0)))
	return out

func gradient_identity() -> Array:
	var g = speedometer().tint_gradient
	return [g.get_class(), g is Resource, g.resource_local_to_scene, g.get_point_count(), g.offsets, g.interpolation_mode, g.interpolation_color_space]

func env_reads() -> Array:
	var we := world_env()
	var e: Environment = we.environment
	return [e.get_class(), e is Resource, is_same(e, we.environment), e == we.environment,
		e.fog_light_color, e.fog_density, e.background_mode, e.tonemap_mode, e.glow_intensity,
		e.sdfgi_enabled, e.ambient_light_energy, e.sky.get_class(), e.sky.sky_material.resource_path]

func env_write_through_var(v: float) -> float:
	var e = world_env().environment
	e.fog_density = v
	return world_env().environment.fog_density

func env_after_mood() -> Array:
	var e = world_env().environment
	return [e.fog_light_color, e.sky.sky_material.resource_path]

func shapes() -> Array:
	var a = get_node("/root/TownScene/TownModel/Trees/Tree/StaticBody3D/CollisionShape3D")
	var b = get_node("/root/TownScene/TownModel/Trees/Tree2/StaticBody3D/CollisionShape3D")
	var before := [a.shape.get_class(), is_same(a.shape, b.shape), a.shape.height, a.shape.radius]
	a.shape.height = 3.5
	before.append(b.shape.height)
	return before

func gradient_texture() -> Array:
	var tex = get_node("/root/TownScene/TouchTurnLeft/TurnLeft").texture_normal
	return [tex.get_class(), tex.width, tex.gradient.get_class(), tex.gradient.sample(0.5)]

func drive_speedometer(speed: float) -> void:
	var body := VehicleBody3D.new()
	body.linear_velocity = Vector3(speed, 0, 0)
	speedometer().car_body = body
"#;

fn attach_probe(ml: &mut MainLoop) -> NodeId {
    let tree = ml.tree_mut();
    let town = tree
        .get_node_by_path("/root/TownScene")
        .expect("the town scene root");
    let probe = tree.add_child(town, Node::new("Probe", "Node")).unwrap();
    let script = GDScriptNodeInstance::from_source(PROBE_GD, probe).expect("probe parses");
    tree.attach_script(probe, Box::new(script));
    probe
}

fn call(ml: &mut MainLoop, node: NodeId, method: &str, args: &[Variant]) -> Variant {
    let v = ml
        .tree_mut()
        .call_script_method(node, method, args)
        .unwrap_or_else(|| panic!("{method} returned nothing"));
    let out = ml.tree_mut().drain_script_output();
    assert!(
        out.errors.is_empty(),
        "{method} must run without script errors, got {:?}",
        out.errors
    );
    v
}

fn items(v: Variant) -> Vec<Variant> {
    match v {
        Variant::Array(a) => a.to_vec(),
        other => panic!("expected an Array, got {other:?}"),
    }
}

fn color(v: &Variant) -> [f32; 4] {
    match v {
        Variant::Color(c) => [c.r, c.g, c.b, c.a],
        other => panic!("expected a Color, got {other:?}"),
    }
}

fn assert_color(got: &Variant, want: [f32; 4], what: &str) {
    let got = color(got);
    for i in 0..4 {
        assert!(
            (got[i] - want[i]).abs() < 1e-6,
            "{what}: got {got:?}, Godot 4.6.1 gives {want:?}"
        );
    }
}

fn float(v: &Variant) -> f64 {
    match v {
        Variant::Float(f) => *f,
        Variant::Int(i) => *i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn text(v: &Variant) -> String {
    match v {
        Variant::String(s) => s.clone(),
        Variant::StringName(s) => s.as_str().to_string(),
        other => panic!("expected a string, got {other:?}"),
    }
}

fn town() -> (MainLoop, NodeId) {
    let mut ml = boot("town/town_scene.tscn");
    // `_ready` ends in Godot's own error (no car in the "car" group yet), which
    // real Godot raises too; it is not under test here.
    let _ = ml.tree_mut().drain_script_output();
    let probe = attach_probe(&mut ml);
    (ml, probe)
}

/// `tint_gradient.sample(remap(speed, 0, 30, 0, 1))` at the speeds the demo
/// reaches, against Godot's own numbers (LINEAR, sRGB, three points).
#[test]
fn speedometer_gradient_samples_match_godot() {
    let (mut ml, probe) = town();
    let speeds = [0.0, 3.0, 6.0, 9.0, 15.0, 21.0, 30.0, 45.0, -2.0];
    let arg = Variant::Array(gdvariant::SharedArray::from_vec(
        speeds.iter().map(|s| Variant::Float(*s)).collect(),
    ));
    let got = items(call(&mut ml, probe, "gradient_samples", &[arg]));
    let godot: [[f32; 4]; 9] = [
        [0.7019608, 0.9019608, 1.0, 1.0],
        [0.8509804, 0.9509804, 1.0, 1.0],
        [1.0, 1.0, 1.0, 1.0],
        [1.0, 0.9127451, 0.8877451, 1.0],
        [1.0, 0.7382353, 0.6632353, 1.0],
        [1.0, 0.56372553, 0.43872547, 1.0],
        [1.0, 0.3019608, 0.101960786, 1.0],
        [1.0, 0.3019608, 0.101960786, 1.0],
        [0.7019608, 0.9019608, 1.0, 1.0],
    ];
    for ((s, want), got) in speeds.iter().zip(godot).zip(&got) {
        assert_color(got, want, &format!("tint_gradient at {s} m/s"));
    }

    let id = items(call(&mut ml, probe, "gradient_identity", &[]));
    assert_eq!(text(&id[0]), "Gradient", "get_class()");
    assert_eq!(id[1], Variant::Bool(true), "is Resource");
    assert_eq!(id[2], Variant::Bool(false), "resource_local_to_scene");
    assert_eq!(id[3], Variant::Int(3), "get_point_count()");
    assert_eq!(
        id[4],
        Variant::PackedFloat32Array(vec![0.0, 0.2, 1.0]),
        "offsets"
    );
    assert_eq!(float(&id[5]), 0.0, "interpolation_mode (LINEAR)");
    assert_eq!(float(&id[6]), 0.0, "interpolation_color_space (sRGB)");
}

/// The speedometer's own `_process`, end to end: the label is tinted with
/// the gradient colour for the car's speed and no `String.sample` error.
#[test]
fn speedometer_process_tints_the_label_like_godot() {
    let (mut ml, probe) = town();
    call(&mut ml, probe, "drive_speedometer", &[Variant::Float(15.0)]);
    let sp = ml
        .tree()
        .get_node_by_path("/root/TownScene/CarUI/Speedometer")
        .unwrap();
    ml.tree_mut()
        .call_script_method(sp, "_process", &[Variant::Float(1.0 / 60.0)]);
    let out = ml.tree_mut().drain_script_output();
    assert!(
        out.errors.is_empty(),
        "speedometer.gd _process must run clean, got {:?}",
        out.errors
    );
    let node = ml.tree().get_node(sp).unwrap();
    assert_color(
        &node.get_property("theme_override_colors/font_color"),
        [1.0, 0.7382353, 0.6632353, 1.0],
        "font_color override at 15 m/s",
    );
    assert_eq!(
        node.get_property("text"),
        Variant::String("Speed: 15.0 m/s".into())
    );
}

/// `$WorldEnvironment.environment` is an `Environment` with Godot's values, the
/// SAME object on every read, and a write through a variable holding it is
/// visible through the node.
#[test]
fn world_environment_reads_as_one_shared_environment() {
    let (mut ml, probe) = town();
    let r = items(call(&mut ml, probe, "env_reads", &[]));
    assert_eq!(text(&r[0]), "Environment", "get_class()");
    assert_eq!(r[1], Variant::Bool(true), "is Resource");
    assert_eq!(r[2], Variant::Bool(true), "is_same(e, we.environment)");
    assert_eq!(r[3], Variant::Bool(true), "e == we.environment");
    assert_color(&r[4], [0.7254902, 0.91764706, 1.0, 1.0], "fog_light_color");
    assert!(
        (float(&r[5]) - 0.0015).abs() < 1e-7,
        "fog_density {:?}",
        r[5]
    );
    assert_eq!(float(&r[6]), 2.0, "background_mode (BG_SKY)");
    assert_eq!(float(&r[7]), 4.0, "tonemap_mode (AgX)");
    assert_eq!(float(&r[8]), 0.5, "glow_intensity");
    assert_eq!(r[9], Variant::Bool(false), "sdfgi_enabled (default)");
    assert_eq!(float(&r[10]), 1.0, "ambient_light_energy (default)");
    assert_eq!(text(&r[11]), "Sky", "environment.sky.get_class()");
    assert_eq!(text(&r[12]), "res://town/sky_day.tres", "sky.sky_material");

    let after = call(
        &mut ml,
        probe,
        "env_write_through_var",
        &[Variant::Float(0.01)],
    );
    assert!(
        (float(&after) - 0.01).abs() < 1e-7,
        "write through a var: {after:?}"
    );
    // ... and it is on the companion the renderer and the saver read.
    let we = ml
        .tree()
        .get_node_by_path("/root/TownScene/WorldEnvironment")
        .unwrap();
    assert_eq!(
        ml.tree()
            .get_node(we)
            .unwrap()
            .get_property("__subres__environment__fog_density"),
        Variant::Float(0.01)
    );
}

/// town_scene.gd `set_mood`: every mood writes `fog_light_color` and swaps
/// `environment.sky.sky_material`; the reads afterwards are Godot's, and the
/// writes are on the node companions the renderer and saver read.
#[test]
fn set_mood_reads_and_writes_the_environment_like_godot() {
    let (mut ml, probe) = town();
    let town = ml.tree().get_node_by_path("/root/TownScene").unwrap();
    let we = ml
        .tree()
        .get_node_by_path("/root/TownScene/WorldEnvironment")
        .unwrap();
    let godot = [
        ([0.686, 0.6, 0.467, 1.0], "res://town/sky_morning.tres"),
        ([0.725, 0.918, 1.0, 1.0], "res://town/sky_day.tres"),
        ([0.776, 0.549, 0.502, 1.0], "res://town/sky_sunset.tres"),
        ([0.2, 0.149, 0.125, 1.0], "res://town/sky_night.tres"),
    ];
    for (mood, (fog, sky)) in godot.iter().enumerate() {
        ml.tree_mut()
            .call_script_method(town, "set_mood", &[Variant::Int(mood as i64)]);
        let out = ml.tree_mut().drain_script_output();
        // `set_mood` ends with `get_nodes_in_group(&"car")[0]`, an
        // out-of-bounds error in Godot too when no car was set up. Nothing
        // before it may fail.
        assert!(
            out.errors.iter().all(|e| !e.contains("SubResource")
                && !e.contains("on String")
                && !e.contains("String.")),
            "mood {mood}: the environment writes must not fail, got {:?}",
            out.errors
        );
        let r = items(call(&mut ml, probe, "env_after_mood", &[]));
        assert_color(&r[0], *fog, &format!("mood {mood} fog_light_color"));
        assert_eq!(text(&r[1]), *sky, "mood {mood} sky.sky_material");

        let node = ml.tree().get_node(we).unwrap();
        assert_color(
            &node.get_property("__subres__environment__fog_light_color"),
            *fog,
            &format!("mood {mood}: the companion the renderer reads"),
        );
        assert_eq!(
            node.get_property("__extres____subres__environment:sky__sky_material"),
            Variant::String(sky.to_string()),
            "mood {mood}: the nested sky's resolved sky_material path"
        );
    }
}

/// Five tree colliders share `CapsuleShape3D_pxdqy`: one object in Godot, so a
/// write through one is read through another — and lands on the `__shape__*`
/// mirror the physics and the saver read, on every holder.
#[test]
fn a_shared_sub_resource_is_one_object_across_nodes() {
    let (mut ml, probe) = town();
    let r = items(call(&mut ml, probe, "shapes", &[]));
    assert_eq!(text(&r[0]), "CapsuleShape3D");
    assert_eq!(r[1], Variant::Bool(true), "is_same(a.shape, b.shape)");
    assert!(
        (float(&r[2]) - 7.26806640625).abs() < 1e-6,
        "height {:?}",
        r[2]
    );
    assert_eq!(float(&r[3]), 0.5, "radius (default)");
    assert_eq!(
        float(&r[4]),
        3.5,
        "b.shape.height after a.shape.height = 3.5"
    );
    for tree_name in ["Tree", "Tree2", "Tree5"] {
        let id = ml
            .tree()
            .get_node_by_path(&format!(
                "/root/TownScene/TownModel/Trees/{tree_name}/StaticBody3D/CollisionShape3D"
            ))
            .unwrap();
        let node = ml.tree().get_node(id).unwrap();
        assert_eq!(
            node.get_property("__subres__shape__height"),
            Variant::Float(3.5),
            "{tree_name}: verbatim body"
        );
        if node.has_property("__shape__class") {
            assert_eq!(
                node.get_property("__shape__height"),
                Variant::Float(3.5),
                "{tree_name}: the __shape__ mirror"
            );
        }
    }
}

/// A sub-resource nested in another (`GradientTexture2D.gradient`) reads as
/// its own resource with working methods.
#[test]
fn a_nested_sub_resource_reads_as_its_own_resource() {
    let (mut ml, probe) = town();
    let r = items(call(&mut ml, probe, "gradient_texture", &[]));
    assert_eq!(text(&r[0]), "GradientTexture2D");
    assert_eq!(float(&r[1]), 128.0, "width");
    assert_eq!(text(&r[2]), "Gradient");
    assert_color(
        &r[3],
        [1.0, 1.0, 1.0, 0.25098],
        "a one-point gradient at 0.5",
    );
}

/// The real flow: car select -> Mini Van -> town. `town.setup` writes
/// `%WorldEnvironment.environment.sdfgi_enabled` and the speedometer samples
/// its gradient every frame; neither may raise.
#[test]
fn the_town_runs_without_sub_resource_errors() {
    let mut ml = boot("car_select/car_select.tscn");
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }
    let menu = ml.tree().get_node_by_path("/root/CarSelect").unwrap();
    ml.tree_mut()
        .call_script_method(menu, "_on_mini_van_pressed", &[]);
    let mut errors = Vec::new();
    for _ in 0..20 {
        ml.step(1.0 / 60.0);
        errors.extend(ml.tree_mut().drain_script_output().errors);
    }
    assert!(
        ml.tree().get_node_by_path("/root/TownScene").is_some(),
        "the town loads"
    );
    let bad: Vec<&String> = errors
        .iter()
        .filter(|e| {
            e.contains("String.sample") || e.contains("SubResource") || e.contains("on String")
        })
        .collect();
    assert!(
        bad.is_empty(),
        "sub-resource errors in the running town: {bad:?}"
    );
    let sp = ml
        .tree()
        .get_node_by_path("/root/TownScene/CarUI/Speedometer")
        .unwrap();
    assert!(
        matches!(
            ml.tree()
                .get_node(sp)
                .unwrap()
                .get_property("theme_override_colors/font_color"),
            Variant::Color(_)
        ),
        "the speedometer tints its label every frame"
    );
}

fn sky_top(env: &gdserver3d::environment::Environment3D) -> [f32; 4] {
    match env.sky.as_ref().map(|s| &s.material) {
        Some(gdserver3d::sky::SkyMaterial::Procedural(m)) => {
            let c = m.sky_top_color;
            [c.r, c.g, c.b, c.a]
        }
        other => panic!("expected a procedural sky, got {other:?}"),
    }
}

/// The renderer reads the SAME store the script writes: after `set_mood`, the
/// environment the game renders with has the mood's fog colour and the mood's
/// sky material (`town/sky_*.tres`), as Godot renders it. Before, the inline
/// environment was re-read from the scene FILE, so every mood rendered the
/// authored one — and its `sky_material = ExtResource(..)` fell to the default
/// procedural sky.
#[test]
fn the_renderer_sees_what_set_mood_wrote() {
    let (mut ml, _probe) = town();
    let env = gdscene::world_environment::runtime_environment_3d(ml.tree(), None)
        .expect("the town authors an environment");
    // `sky_day.tres`: sky_top_color = Color(7.123351e-06, 0.6897872, 0.89266914, 1)
    let top = sky_top(&env);
    assert!(
        (top[1] - 0.6897872).abs() < 1e-6 && (top[2] - 0.89266914).abs() < 1e-6,
        "the authored sky_day.tres sky, got {top:?}"
    );

    let town = ml.tree().get_node_by_path("/root/TownScene").unwrap();
    ml.tree_mut()
        .call_script_method(town, "set_mood", &[Variant::Int(3)]);
    let _ = ml.tree_mut().drain_script_output();
    let env = gdscene::world_environment::runtime_environment_3d(ml.tree(), None).unwrap();
    let f = env.fog_light_color;
    assert!(
        (f.r - 0.2).abs() < 1e-6 && (f.g - 0.149).abs() < 1e-6 && (f.b - 0.125).abs() < 1e-6,
        "NIGHT fog_light_color, got {f:?}"
    );
    // `sky_night.tres`: sky_top_color = Color(0.0964875, 0.114707, 0.155705, 1)
    let top = sky_top(&env);
    assert!(
        (top[0] - 0.0964875).abs() < 1e-6 && (top[2] - 0.155705).abs() < 1e-6,
        "NIGHT sky_night.tres sky, got {top:?}"
    );
}
