//! pat-b4zhq — the natives gap batch found by the real-game script audit.
//!
//! Every item here was the "registered in ClassDB with no dispatch arm" trap
//! or a plain missing arm: the call answered `Nil` (or raised) while Godot
//! answers a concrete value. Each test asserts the CONCRETE value, never
//! truthiness:
//!
//! * `PhysicsDirectBodyState2D.get/set_linear_velocity`, `get_transform`,
//!   `is_sleeping`, `get/set_angular_velocity` — only the property spellings
//!   worked; `state.set_linear_velocity(v)` was swallowed.
//! * `CollisionObject2D.get_rid()` — no 2D arm (3D had one), and the 2D ray
//!   `exclude` did not accept a `RID`.
//! * `CanvasItem.get_global_transform_with_canvas()` — no arm at all, and a
//!   Control's `get_global_transform()` answered Nil.
//! * `Input.get_vector(..., deadzone)` — the 5th argument raised a TypeError.
//! * `RenderingServer.SHADOW_QUALITY_*`, `DirectionalLight3D.SKY_MODE_*` /
//!   `SHADOW_*` — UndefinedVariable; `directional_soft_shadow_filter_set_quality`
//!   — UndefinedFunction. The 3D platformer's own `stage.gd` is run under
//!   Compatibility as the end-to-end check.

use gdcore::math::{Transform2D, Vector2};
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::PackedScene;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

fn attach(tree: &mut SceneTree, id: NodeId, src: &str) {
    let script = GDScriptNodeInstance::from_source(src, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
}

fn meta(ml: &MainLoop, id: NodeId, key: &str) -> Variant {
    ml.tree().get_node(id).expect("node").get_meta(key)
}

fn approx(a: Vector2, b: Vector2) -> bool {
    (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
}

// ---------------------------------------------------------------------------
// PhysicsDirectBodyState2D method forms
// ---------------------------------------------------------------------------

/// The METHOD spellings inside `_integrate_forces`, written exactly as the
/// official demos write them. The setter must reach the body (it moves along
/// +X, which only the written velocity can do — gravity is +Y), and each getter
/// must answer the state's own value of the right type.
#[test]
fn direct_body_state_2d_method_forms_read_and_write_the_body() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("custom_integrator", Variant::Bool(true));
    body.set_property("position", Variant::Vector2(Vector2::new(30.0, 40.0)));
    let id = tree.add_child(root, body).unwrap();
    attach(
        &mut tree,
        id,
        "extends RigidBody2D
var ticks = 0
func _integrate_forces(state):
    self.ticks = self.ticks + 1
    if self.ticks == 1:
        set_meta(\"tf0\", state.get_transform())
        set_meta(\"sleeping_type\", typeof(state.is_sleeping()))
        set_meta(\"sleeping\", [state.is_sleeping()])
    if self.ticks == 2:
        set_meta(\"lv\", state.get_linear_velocity())
        set_meta(\"av\", state.get_angular_velocity())
    state.set_linear_velocity(Vector2(120, 0))
    state.set_angular_velocity(2.5)
",
    );
    let mut ml = MainLoop::new(tree);
    for _ in 0..5 {
        ml.step(DT);
    }

    // `get_transform()` on tick 1 is the body's placement before any motion.
    match meta(&ml, id, "tf0") {
        Variant::Transform2D(t) => assert!(
            approx(t.origin, Vector2::new(30.0, 40.0)),
            "get_transform().origin must be the body's position (30, 40), got {:?}",
            t.origin
        ),
        other => panic!("state.get_transform() must be a Transform2D, got {other:?}"),
    }
    assert_eq!(
        meta(&ml, id, "sleeping_type"),
        Variant::Int(1),
        "state.is_sleeping() must be a bool (TYPE_BOOL = 1)"
    );
    assert_eq!(
        meta(&ml, id, "sleeping"),
        Variant::Array(vec![Variant::Bool(false)].into()),
        "an awake body reports is_sleeping() == false"
    );
    // Tick 2 sees what tick 1's setters wrote.
    match meta(&ml, id, "lv") {
        Variant::Vector2(v) => assert!(
            (v.x - 120.0).abs() < 1e-3,
            "get_linear_velocity().x must be the 120 set_linear_velocity wrote, got {v:?}"
        ),
        other => panic!("state.get_linear_velocity() must be a Vector2, got {other:?}"),
    }
    match meta(&ml, id, "av") {
        Variant::Float(f) => assert!(
            (f - 2.5).abs() < 1e-3,
            "get_angular_velocity() must be the 2.5 set_angular_velocity wrote, got {f}"
        ),
        other => panic!("state.get_angular_velocity() must be a float, got {other:?}"),
    }
    let pos = match ml.tree().get_node(id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        other => panic!("position: {other:?}"),
    };
    assert!(
        pos.x > 35.0,
        "set_linear_velocity(Vector2(120, 0)) must move the body along +X; \
         x stayed at {} (started at 30)",
        pos.x
    );
}

// ---------------------------------------------------------------------------
// CollisionObject2D.get_rid
// ---------------------------------------------------------------------------

/// A wall on the +X axis and a probe that fires a ray at it, with or without
/// the wall's own `get_rid()` in `exclude`.
fn rid_probe(exclude_wall: bool) -> (Variant, Variant, Variant) {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(200.0, 0.0)));
    let wall_id = tree.add_child(main, wall).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(Vector2::new(40.0, 400.0)));
    tree.add_child(wall_id, shape).unwrap();
    let probe = tree.add_child(main, Node::new("Probe", "Node2D")).unwrap();
    let exclude = if exclude_wall {
        "    params.exclude = [wall.get_rid()]\n"
    } else {
        ""
    };
    attach(
        &mut tree,
        probe,
        &format!(
            "extends Node2D
var ran = false
func _physics_process(delta):
    if self.ran:
        return
    self.ran = true
    var wall = get_node(\"../Wall\")
    set_meta(\"rid_type\", typeof(wall.get_rid()))
    set_meta(\"has\", wall.has_method(\"get_rid\"))
    var params = PhysicsRayQueryParameters2D.create(Vector2(0, 0), Vector2(400, 0))
{exclude}    var hit = get_world_2d().direct_space_state.intersect_ray(params)
    set_meta(\"hit\", [hit.is_empty()])
"
        ),
    );
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    for _ in 0..3 {
        ml.step(DT);
    }
    (
        meta(&ml, probe, "rid_type"),
        meta(&ml, probe, "has"),
        meta(&ml, probe, "hit"),
    )
}

/// `get_rid()` is a `RID` (TYPE_RID = 23), `has_method` agrees, and the RID in
/// a ray's `exclude` makes the ray pass through that body — the reason a
/// script asks for a RID at all. The control ray without `exclude` must hit,
/// so a ray that never hits anything cannot pass the exclude half.
#[test]
fn collision_object_2d_get_rid_is_a_rid_that_ray_exclude_honours() {
    let (rid_type, has, hit_without) = rid_probe(false);
    assert_eq!(
        rid_type,
        Variant::Int(23),
        "get_rid() must be a RID (TYPE_RID = 23)"
    );
    assert_eq!(
        has,
        Variant::Bool(true),
        "has_method(\"get_rid\") must be true"
    );
    assert_eq!(
        hit_without,
        Variant::Array(vec![Variant::Bool(false)].into()),
        "control: without exclude the ray must hit the wall (non-empty dict)"
    );
    let (_, _, hit_with) = rid_probe(true);
    assert_eq!(
        hit_with,
        Variant::Array(vec![Variant::Bool(true)].into()),
        "with the wall's own get_rid() in exclude the ray must miss (empty dict)"
    );
}

// ---------------------------------------------------------------------------
// CanvasItem.get_global_transform_with_canvas
// ---------------------------------------------------------------------------

/// A Control on a scaled, offset `CanvasLayer`: the canvas transform is the
/// layer's, applied over the Control's own global transform
/// (`canvas_item.cpp:159`). The virtual joystick reads `.get_scale()` of it.
#[test]
fn get_global_transform_with_canvas_carries_the_canvas_layer() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut layer = Node::new("Layer", "CanvasLayer");
    layer.set_property("offset", Variant::Vector2(Vector2::new(10.0, 20.0)));
    layer.set_property("scale", Variant::Vector2(Vector2::new(2.0, 2.0)));
    let layer = tree.add_child(root, layer).unwrap();
    let mut panel = Node::new("Panel", "Control");
    panel.set_property("position", Variant::Vector2(Vector2::new(5.0, 7.0)));
    let panel = tree.add_child(layer, panel).unwrap();
    let mut knob = Node::new("Knob", "Control");
    knob.set_property("position", Variant::Vector2(Vector2::new(1.0, 1.0)));
    let knob = tree.add_child(panel, knob).unwrap();
    attach(
        &mut tree,
        knob,
        "extends Control
func _ready():
    set_meta(\"gt\", get_global_transform())
    set_meta(\"gtc\", get_global_transform_with_canvas())
    set_meta(\"scale\", get_global_transform_with_canvas().get_scale())
    set_meta(\"has\", has_method(\"get_global_transform_with_canvas\"))
",
    );
    gdscene::LifecycleManager::enter_tree(&mut tree, knob);
    let node = tree.get_node(knob).unwrap();

    match node.get_meta("gt") {
        Variant::Transform2D(t) => assert!(
            approx(t.origin, Vector2::new(6.0, 8.0)),
            "a Control's get_global_transform() composes its Control parents: \
             (5,7) + (1,1) = (6,8), got {:?}",
            t.origin
        ),
        other => panic!("Control.get_global_transform() must be a Transform2D, got {other:?}"),
    }
    // Layer: scale 2 then offset (10, 20) -> (6,8)*2 + (10,20) = (22, 36).
    match node.get_meta("gtc") {
        Variant::Transform2D(t) => {
            assert!(
                approx(t.origin, Vector2::new(22.0, 36.0)),
                "with canvas = layer(scale 2, offset (10,20)) * global (6,8) = (22,36), got {:?}",
                t.origin
            );
            assert_ne!(t, Transform2D::IDENTITY);
        }
        other => panic!("get_global_transform_with_canvas() must be a Transform2D, got {other:?}"),
    }
    match node.get_meta("scale") {
        Variant::Vector2(s) => assert!(
            approx(s, Vector2::new(2.0, 2.0)),
            "the joystick's `.get_scale()` of it must be the layer's 2x, got {s:?}"
        ),
        other => panic!("get_scale(): {other:?}"),
    }
    assert_eq!(node.get_meta("has"), Variant::Bool(true));
}

/// On the default canvas the canvas transform is the Camera2D view: a camera
/// at (300, 200) on the default 1152x648 window puts world (300, 200) at the
/// screen centre (576, 324).
#[test]
fn get_global_transform_with_canvas_on_the_default_canvas_is_the_camera_view() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = tree.add_child(root, Node::new("World", "Node2D")).unwrap();
    let mut cam = Node::new("Cam", "Camera2D");
    cam.set_property("position", Variant::Vector2(Vector2::new(300.0, 200.0)));
    cam.set_property("enabled", Variant::Bool(true));
    tree.add_child(world, cam).unwrap();
    let mut item = Node::new("Item", "Node2D");
    item.set_property("position", Variant::Vector2(Vector2::new(300.0, 200.0)));
    let item = tree.add_child(world, item).unwrap();
    attach(
        &mut tree,
        item,
        "extends Node2D\nfunc _ready():\n    set_meta(\"gtc\", get_global_transform_with_canvas())\n",
    );
    gdscene::LifecycleManager::enter_tree(&mut tree, item);
    let size = tree.root_window().visible_size();
    match tree.get_node(item).unwrap().get_meta("gtc") {
        Variant::Transform2D(t) => assert!(
            approx(t.origin, Vector2::new(size.x * 0.5, size.y * 0.5)),
            "an item under the camera centre lands at the screen centre {:?}, got {:?}",
            Vector2::new(size.x * 0.5, size.y * 0.5),
            t.origin
        ),
        other => panic!("get_global_transform_with_canvas(): {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Input.get_vector deadzone
// ---------------------------------------------------------------------------

/// `get_vector(..., deadzone)` follows Godot's circular deadzone exactly
/// (`core/input/input.cpp` `Input::get_vector`) on the RAW strengths: a stick
/// at 0.6 under an explicit 0.3 deadzone remaps its length from (0.3, 1) to
/// (0, 1), giving x = (0.6 - 0.3) / (1 - 0.3) = 0.428571; under 0.7 it is
/// inside the deadzone and reads (0, 0). At HEAD the 5-argument call raised.
#[test]
fn input_get_vector_applies_the_deadzone_argument() {
    use gdplatform::input::{ActionBinding, GamepadAxis, InputEvent, InputMap};
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = tree.add_child(root, Node::new("Probe", "Node")).unwrap();
    attach(
        &mut tree,
        probe,
        "extends Node
func _process(delta):
    set_meta(\"v3\", [Input.get_vector(\"left\", \"right\", \"up\", \"down\", 0.3)])
    set_meta(\"v7\", [Input.get_vector(\"left\", \"right\", \"up\", \"down\", 0.7)])
",
    );
    let mut map = InputMap::new();
    for action in ["left", "up", "down"] {
        map.add_action(action, 0.1);
    }
    map.add_action("right", 0.1);
    map.action_add_event(
        "right",
        ActionBinding::GamepadAxisDirection {
            axis: GamepadAxis::LeftStickX,
            positive: true,
        },
    );
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(map);
    ml.push_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.6,
        gamepad_id: 0,
    });
    ml.step(DT);
    let one = |key: &str| match meta(&ml, probe, key) {
        Variant::Array(a) => match a.to_vec().first() {
            Some(Variant::Vector2(v)) => *v,
            other => panic!("{key}: expected a Vector2, got {other:?}"),
        },
        other => panic!("{key}: the 5-argument get_vector must not raise, got {other:?}"),
    };
    let v3 = one("v3");
    let expected = (0.6 - 0.3) / (1.0 - 0.3);
    assert!(
        (v3.x - expected).abs() < 1e-4 && v3.y.abs() < 1e-6,
        "stick 0.6, deadzone 0.3 -> ({expected:.6}, 0), got {v3:?}"
    );
    assert_eq!(
        one("v7"),
        Vector2::ZERO,
        "stick 0.6 is inside a 0.7 deadzone"
    );
}

// ---------------------------------------------------------------------------
// RenderingServer / DirectionalLight3D constants + stage.gd under Compatibility
// ---------------------------------------------------------------------------

/// The constants' values come from `doc/classes/RenderingServer.xml` and
/// `DirectionalLight3D.xml`.
#[test]
fn shadow_quality_and_directional_light_constants_resolve() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = tree.add_child(root, Node::new("Probe", "Node")).unwrap();
    attach(
        &mut tree,
        probe,
        "extends Node
func _ready():
    set_meta(\"c\", [RenderingServer.SHADOW_QUALITY_HARD, RenderingServer.SHADOW_QUALITY_SOFT_HIGH, RenderingServer.SHADOW_QUALITY_SOFT_ULTRA, DirectionalLight3D.SHADOW_ORTHOGONAL, DirectionalLight3D.SHADOW_PARALLEL_4_SPLITS, DirectionalLight3D.SKY_MODE_LIGHT_AND_SKY, DirectionalLight3D.SKY_MODE_LIGHT_ONLY, DirectionalLight3D.SKY_MODE_SKY_ONLY, RenderingServer.GLOBAL_VAR_TYPE_MAX])
",
    );
    gdscene::LifecycleManager::enter_tree(&mut tree, probe);
    let ints = [0, 4, 5, 0, 2, 0, 1, 2, 29].map(Variant::Int).to_vec();
    assert_eq!(
        tree.get_node(probe).unwrap().get_meta("c"),
        Variant::Array(ints.into()),
        "SHADOW_QUALITY_HARD/SOFT_HIGH/SOFT_ULTRA = 0/4/5, SHADOW_ORTHOGONAL/\
         PARALLEL_4_SPLITS = 0/2, SKY_MODE_* = 0/1/2, and GLOBAL_VAR_TYPE_MAX \
         must keep resolving (29)"
    );
}

fn platformer_3d_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d")
        .canonicalize()
        .expect("games/platformer_3d fixture")
}

/// The demo's own `stage.gd`, booted with `rendering_method = gl_compatibility`
/// so it takes the branch Godot takes under Compatibility: soft-shadow quality
/// set to SOFT_HIGH, the sun switched to SKY_ONLY, and a dimmed LIGHT_ONLY copy
/// added. At HEAD the first line of that branch raised, so none of it ran.
#[test]
fn stage_gd_compatibility_branch_runs_to_completion() {
    gdobject::class_db::register_all_classes();
    let dir = platformer_3d_dir();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let text = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    let mut settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).unwrap();
    settings.set(
        "rendering/renderer/rendering_method",
        gdcore::project_settings::SettingsValue::String("gl_compatibility".into()),
    );
    gdscene::project_runtime::seed_script_visible_settings(&settings, &dir);
    assert_eq!(
        gdcore::project_settings::directional_soft_shadow_quality_override(),
        None,
        "a fresh boot starts with no runtime shadow-quality override"
    );

    let packed = PackedScene::from_tscn(
        r#"[gd_scene format=3]
[ext_resource type="Script" path="res://stage/stage.gd" id="1"]

[node name="Stage" type="Node3D"]
script = ExtResource("1")

[node name="DirectionalLight3D" type="DirectionalLight3D" parent="."]
light_energy = 1.0
"#,
    )
    .unwrap();
    let mut tree = SceneTree::new();
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
    let errs = tree.drain_script_errors();
    assert!(
        errs.is_empty(),
        "stage.gd must run without a script error: {errs:?}"
    );

    assert_eq!(
        gdcore::project_settings::directional_soft_shadow_quality_override(),
        Some(4),
        "directional_soft_shadow_filter_set_quality(SHADOW_QUALITY_SOFT_HIGH) \
         must reach the renderer override as 4"
    );
    let lights: Vec<NodeId> = tree
        .get_node(id)
        .unwrap()
        .children()
        .iter()
        .copied()
        .filter(|c| tree.get_node(*c).unwrap().class_name() == "DirectionalLight3D")
        .collect();
    assert_eq!(
        lights.len(),
        2,
        "the Compatibility branch adds a second sun"
    );
    let prop = |n: NodeId, k: &str| tree.get_node(n).unwrap().get_property(k);
    assert_eq!(
        prop(lights[0], "sky_mode"),
        Variant::Int(2),
        "SKY_MODE_SKY_ONLY = 2"
    );
    assert_eq!(
        prop(lights[1], "sky_mode"),
        Variant::Int(1),
        "SKY_MODE_LIGHT_ONLY = 1"
    );
    assert_eq!(prop(lights[1], "light_energy"), Variant::Float(0.25));
}
