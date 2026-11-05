//! pat-v3co1 — the official Godot 3D platformer demo's OWN scripts, run
//! unmodified from `games/platformer_3d`.
//!
//! Every test here loads the real `.gd` file off disk (`res://` rooted at the
//! game), so a regression in any native the demo touches shows up as the demo
//! misbehaving, not as a synthetic probe drifting from what games write.
//!
//! The player and enemy scenes instance a `.glb` (another workstream), so those
//! two are rebuilt here as plain `.tscn` text with the SAME node paths the
//! scripts address (`$Player/Skeleton`, `%CoinCount`, `$Target/Camera3D`, ...).
//! The touch-screen UI is pure `.tscn` and is loaded as shipped.
//!
//! What each assertion would have caught at the parent commit:
//!
//! * `stage.gd` — `RenderingServer.get_current_rendering_method()` raised.
//! * `touch_screen_ui.gd` / `virtual_joystick.gd` —
//!   `DisplayServer.is_touchscreen_available()` raised, so the joystick's
//!   `hide()` never ran and it stayed VISIBLE on desktop.
//! * `player.gd` failed to PARSE (`%CoinCount` was not an expression), and once
//!   it parsed: `%CoinCount` found nothing (the `.tscn` spells the flag
//!   `unique_name_in_owner = true`), `get_gravity()` was missing for 3D bodies,
//!   `Input.get_vector(&"..")` rejected StringNames, and
//!   `_animation_tree[&"parameters/.."] = x` was an "invalid index assignment".
//! * `follow_camera.gd` — `get_rid()`, `World3D.get_space()`,
//!   `PhysicsServer3D.space_get_direct_state()` and
//!   `PhysicsRayQueryParameters3D.create()` were all missing, the ray ignored
//!   `exclude`, and `look_at_from_position` on its `top_level` node landed it
//!   offset by the parent — so the camera never followed the player.
//! * `enemy.gd` — `state.get_step()` answered Nil inside `_integrate_forces`.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d")
        .canonicalize()
        .expect("games/platformer_3d fixture")
}

fn project_settings() -> gdcore::project_settings::ProjectSettings {
    let text = std::fs::read_to_string(game_dir().join("project.godot")).expect("project.godot");
    gdcore::project_settings::ProjectSettings::from_godot_format(&text).expect("project parses")
}

/// Boots `tscn` the way the runner does: res:// at the game, the project's
/// settings published to scripts, the scene built detached with its scripts
/// attached, then ONE `add_child` (enter + ready), then physics registered.
///
/// `tweak` edits the settings before they are published — the hook the
/// emulate-touch and rendering-method variants use.
fn boot_with(
    tscn: &str,
    tweak: impl FnOnce(&mut gdcore::project_settings::ProjectSettings),
) -> MainLoop {
    gdobject::class_db::register_all_classes();
    let dir = game_dir();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let mut settings = project_settings();
    tweak(&mut settings);
    gdscene::project_runtime::seed_script_settings_for_this_thread(&settings, &dir);

    let packed = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::begin_subscene_session();
    let id = gdscene::packed_scene::instance_packed_scene_detached(
        &mut tree,
        &packed,
        &gdscene::packed_scene::load_scene_from_res,
    )
    .expect("scene instances");
    gdscene::scripting::attach_subtree_scripts_from_res(&mut tree, id);
    let node = tree.take_node(id).expect("detached root");
    tree.add_child(root, node).expect("add_child");
    gdscene::packed_scene::finish_attached_scene(&mut tree, id);
    let mut ml = MainLoop::new(tree);
    gdscene::project_runtime::apply_project_settings(&mut ml, &settings);
    ml.register_physics_bodies();
    ml
}

fn boot(tscn: &str) -> MainLoop {
    boot_with(tscn, |_| {})
}

fn node(ml: &MainLoop, path: &str) -> NodeId {
    ml.tree()
        .get_node_by_path(path)
        .unwrap_or_else(|| panic!("scene must contain {path}"))
}

fn errors(ml: &mut MainLoop) -> Vec<String> {
    ml.tree_mut().drain_script_errors()
}

fn script_var(ml: &MainLoop, id: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(id)
        .and_then(|s| s.get_property(name))
        .unwrap_or(Variant::Nil)
}

fn global_origin(ml: &MainLoop, id: NodeId) -> Vector3 {
    gdscene::node3d::get_global_transform(ml.tree(), id).origin
}

// ---------------------------------------------------------------------------
// stage.gd
// ---------------------------------------------------------------------------

/// The real stage script on a root with the one child it touches.
const STAGE: &str = r#"[gd_scene format=3]
[ext_resource type="Script" path="res://stage/stage.gd" id="1"]

[node name="Stage" type="Node3D"]
script = ExtResource("1")

[node name="DirectionalLight3D" type="DirectionalLight3D" parent="."]
light_energy = 1.0
"#;

fn light_count(ml: &MainLoop, root: NodeId) -> usize {
    ml.tree()
        .get_node(root)
        .unwrap()
        .children()
        .iter()
        .filter(|c| {
            ml.tree()
                .get_node(**c)
                .is_some_and(|n| n.class_name() == "DirectionalLight3D")
        })
        .count()
}

/// The demo declares no rendering method, so Godot runs it on `forward_plus`
/// and `stage.gd` takes NO Compatibility branch: one sun, untouched.
#[test]
fn stage_ready_runs_on_forward_plus_and_keeps_its_single_sun() {
    let mut ml = boot(STAGE);
    let errs = errors(&mut ml);
    assert!(
        errs.is_empty(),
        "stage.gd's _ready must run without a script error, got {errs:?}"
    );
    let stage = node(&ml, "/root/Stage");
    assert_eq!(
        light_count(&ml, stage),
        1,
        "forward_plus must not take the gl_compatibility branch (which duplicates the sun)"
    );
}

// ---------------------------------------------------------------------------
// touch_screen_ui.gd + virtual_joystick.gd (the shipped .tscn)
// ---------------------------------------------------------------------------

const TOUCH_UI: &str = "touch_screen_ui/touch_screen_ui.tscn";

fn visible(ml: &MainLoop, id: NodeId) -> Variant {
    ml.tree().get_node(id).unwrap().get_property("visible")
}

fn touch_ui_text() -> String {
    std::fs::read_to_string(game_dir().join(TOUCH_UI)).expect("touch UI scene")
}

/// 🔑 On a desktop (no touch emulation) BOTH scripts hide their node. The
/// joystick is the discriminating one: before the fix its
/// `if not DisplayServer.is_touchscreen_available(): hide()` raised before
/// `hide()`, so it stayed visible over the game.
#[test]
fn touch_ui_and_joystick_hide_on_a_desktop_without_touch() {
    let mut ml = boot(&touch_ui_text());
    let errs = errors(&mut ml);
    assert!(errs.is_empty(), "touch UI _ready must not raise: {errs:?}");
    let ui = node(&ml, "/root/TouchScreenUI");
    let stick = node(&ml, "/root/TouchScreenUI/Virtual Joystick");
    assert_eq!(visible(&ml, ui), Variant::Bool(false), "TouchScreenUI hides");
    assert_eq!(
        visible(&ml, stick),
        Variant::Bool(false),
        "the virtual joystick hides itself when there is no touchscreen"
    );
}

/// Godot's base `is_touchscreen_available()` IS mouse-to-touch emulation, so a
/// project that turns it on shows the touch UI — the other half of the branch.
#[test]
fn touch_ui_shows_when_the_project_emulates_touch_from_mouse() {
    let mut ml = boot_with(&touch_ui_text(), |s| {
        s.set(
            "input_devices/pointing/emulate_touch_from_mouse",
            gdcore::project_settings::SettingsValue::Bool(true),
        );
    });
    let errs = errors(&mut ml);
    assert!(errs.is_empty(), "touch UI _ready must not raise: {errs:?}");
    let ui = node(&ml, "/root/TouchScreenUI");
    let stick = node(&ml, "/root/TouchScreenUI/Virtual Joystick");
    assert_eq!(visible(&ml, ui), Variant::Bool(true), "TouchScreenUI shows");
    assert_eq!(visible(&ml, stick), Variant::Bool(true), "the joystick stays");
}

// ---------------------------------------------------------------------------
// player.gd + follow_camera.gd + enemy.gd
// ---------------------------------------------------------------------------

/// `player.tscn` / `enemy.tscn` minus their `.glb` instances: the same node
/// paths, a floor to stand on, and `unique_name_in_owner = true` spelled the
/// way the shipped `player.tscn` spells it.
const WORLD: &str = r#"[gd_scene format=3]
[ext_resource type="Script" path="res://player/player.gd" id="1"]
[ext_resource type="Script" path="res://player/follow_camera.gd" id="2"]
[ext_resource type="Script" path="res://enemy/enemy.gd" id="3"]

[sub_resource type="CapsuleShape3D" id="cap"]
[sub_resource type="BoxShape3D" id="floor"]
size = Vector3(60, 1, 60)
[sub_resource type="SphereShape3D" id="sph"]

[node name="World" type="Node3D"]

[node name="Floor" type="StaticBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, -0.5, 0)
[node name="Shape" type="CollisionShape3D" parent="Floor"]
shape = SubResource("floor")

[node name="Player" type="CharacterBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0)
script = ExtResource("1")
[node name="CollisionCapsule" type="CollisionShape3D" parent="Player"]
shape = SubResource("cap")
[node name="Target" type="Node3D" parent="Player"]
[node name="Camera3D" type="Camera3D" parent="Player/Target"]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 3)
script = ExtResource("2")
[node name="Player" type="Node3D" parent="Player"]
[node name="Skeleton" type="Node3D" parent="Player/Player"]
[node name="Bullet" type="Marker3D" parent="Player/Player/Skeleton"]
[node name="CoinCount" type="Label3D" parent="Player/Player/Skeleton"]
unique_name_in_owner = true
text = "123"
[node name="Parallax" type="Label3D" parent="Player/Player/Skeleton/CoinCount"]
[node name="Parallax2" type="Label3D" parent="Player/Player/Skeleton/CoinCount"]
[node name="Parallax3" type="Label3D" parent="Player/Player/Skeleton/CoinCount"]
[node name="Parallax4" type="Label3D" parent="Player/Player/Skeleton/CoinCount"]
[node name="AnimationTree" type="AnimationTree" parent="Player"]
[node name="SoundShoot" type="AudioStreamPlayer" parent="Player"]
[node name="SoundJump" type="AudioStreamPlayer" parent="Player"]

[node name="Enemy" type="RigidBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 8, 1, 0)
custom_integrator = true
contact_monitor = true
max_contacts_reported = 5
script = ExtResource("3")
[node name="Shape" type="CollisionShape3D" parent="Enemy"]
shape = SubResource("sph")
[node name="Enemy" type="Node3D" parent="Enemy"]
[node name="Skeleton" type="Node3D" parent="Enemy/Enemy"]
[node name="RayFloor" type="RayCast3D" parent="Enemy/Enemy/Skeleton"]
[node name="RayWall" type="RayCast3D" parent="Enemy/Enemy/Skeleton"]
[node name="AnimationPlayer" type="AnimationPlayer" parent="Enemy/Enemy"]
[node name="SoundWalkLoop" type="AudioStreamPlayer3D" parent="Enemy"]
[node name="SoundHit" type="AudioStreamPlayer3D" parent="Enemy"]
"#;

fn vec3(v: Variant, what: &str) -> Vector3 {
    match v {
        Variant::Vector3(v) => v,
        other => panic!("{what} should be a Vector3, got {other:?}"),
    }
}

fn float(v: Variant, what: &str) -> f64 {
    match v {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("{what} should be a number, got {other:?}"),
    }
}

/// 🔑 The whole world runs a second of physics with no script error — every
/// native the three scripts call each frame is dispatched — and the first
/// airborne frame's fall is exactly the PROJECT's gravity (22, not Godot's
/// built-in 9.8), which only a real `get_gravity()` can produce.
#[test]
fn player_camera_and_enemy_scripts_run_without_errors_and_fall_at_project_gravity() {
    let mut ml = boot(WORLD);
    let player = node(&ml, "/root/World/Player");
    assert!(
        ml.tree().get_script(player).is_some(),
        "player.gd must PARSE and attach (it uses `%CoinCount`)"
    );

    ml.step(DT);
    let vy = vec3(ml.tree().get_node(player).unwrap().get_property("velocity"), "velocity").y;
    // `velocity += get_gravity() * delta` from rest, before any contact. The
    // 1/60 frame is two 1/120 physics ticks, so the sum is 22 * (1/60) either
    // way; the player starts 1 m above the floor, out of reach in 1/60 s.
    let want = -22.0 * DT as f32;
    assert!(
        (vy - want).abs() < 1e-3,
        "first-frame fall must be the project's 22 m/s² * dt = {want}, got {vy}"
    );

    let mut errs = errors(&mut ml);
    for _ in 0..60 {
        ml.step(DT);
        errs.extend(errors(&mut ml));
    }
    errs.sort();
    errs.dedup();
    assert!(errs.is_empty(), "no script may raise while the world runs: {errs:#?}");

    // `%CoinCount.text = str(coins)` reached the Label3D the `.tscn` marks
    // unique with `unique_name_in_owner = true` (authored "123").
    let label = node(&ml, "/root/World/Player/Player/Skeleton/CoinCount");
    assert_eq!(
        ml.tree().get_node(label).unwrap().get_property("text"),
        Variant::String("0".into()),
        "%CoinCount must resolve and receive str(coins)"
    );
}

/// Godot 4 saves "Access as Unique Name" as the property
/// `unique_name_in_owner = true` (the shipped `player.tscn` does), not as a
/// `%` in the node's name. Either spelling must make `%Name` resolve.
#[test]
fn unique_name_in_owner_property_makes_percent_lookup_resolve() {
    const SCENE: &str = r#"[gd_scene format=3]
[node name="Root" type="Node3D"]
[node name="Deep" type="Node3D" parent="."]
[node name="ByProperty" type="Label3D" parent="Deep"]
unique_name_in_owner = true
[node name="%ByName" type="Label3D" parent="Deep"]
[node name="Plain" type="Label3D" parent="Deep"]
"#;
    let ml = boot(SCENE);
    let root = node(&ml, "/root/Root");
    let by_property = node(&ml, "/root/Root/Deep/ByProperty");
    let by_name = node(&ml, "/root/Root/Deep/ByName");
    let tree = ml.tree();
    assert_eq!(tree.get_node_relative(root, "%ByProperty"), Some(by_property));
    assert_eq!(tree.get_node_relative(root, "%ByName"), Some(by_name));
    assert_eq!(
        tree.get_node_relative(root, "%Plain"),
        None,
        "CONTROL: a node without the flag is not reachable as %Name"
    );
}

/// Holding `move_forward` moves the player away from the camera (−z), the
/// AnimationTree receives the run blend through `tree[&"parameters/.."] = x`,
/// and the `top_level` follow camera trails it within `max_distance` (3.5).
#[test]
fn player_runs_forward_and_the_follow_camera_keeps_up() {
    let mut ml = boot(WORLD);
    let player = node(&ml, "/root/World/Player");
    let camera = node(&ml, "/root/World/Player/Target/Camera3D");
    let anim_tree = node(&ml, "/root/World/Player/AnimationTree");

    // Land first.
    for _ in 0..30 {
        ml.step(DT);
    }
    let start = global_origin(&ml, player);
    for _ in 0..120 {
        ml.input_state_mut().action_press("move_forward", 1.0);
        ml.step(DT);
    }
    let errs = errors(&mut ml);
    assert!(errs.is_empty(), "running must not raise: {errs:#?}");

    let end = global_origin(&ml, player);
    assert!(
        end.z < start.z - 5.0,
        "two seconds of move_forward at up to 6 m/s must carry the player well \
         along −z: {start:?} -> {end:?}"
    );

    // `_animation_tree[&"parameters/run/blend_amount"] = horizontal_speed / MAX_SPEED`
    let blend = ml
        .tree()
        .get_node(anim_tree)
        .unwrap()
        .get_property("parameters/run/blend_amount");
    let blend = float(blend, "parameters/run/blend_amount");
    assert!(
        (blend - 1.0).abs() < 0.05,
        "at full speed the run blend is speed / MAX_SPEED ≈ 1.0, got {blend}"
    );

    // The camera's script clamps its distance to the TARGET (the player's
    // origin) into [min_distance, max_distance] every frame.
    let cam = global_origin(&ml, camera);
    let target = global_origin(&ml, node(&ml, "/root/World/Player/Target"));
    let dist = (cam - target).length();
    assert!(
        dist <= 3.5 + 0.05,
        "the follow camera must stay within max_distance 3.5 of the player; it is \
         {dist} away (camera {cam:?}, target {target:?})"
    );
    assert!(
        cam.z > target.z,
        "the camera trails BEHIND the player (larger z), got camera {cam:?} target {target:?}"
    );
}

/// The enemy's `_integrate_forces` reads `state.get_step()` into `delta` and
/// writes `state.set_linear_velocity(..)`. The step must be the physics tick.
#[test]
fn enemy_integrate_forces_reads_the_physics_step() {
    // A probe beside the real enemy: the same calls, recorded.
    const PROBE: &str = r#"[gd_scene format=3]
[sub_resource type="SphereShape3D" id="sph"]

[node name="World" type="Node3D"]
[node name="Probe" type="RigidBody3D" parent="."]
custom_integrator = true
[node name="Shape" type="CollisionShape3D" parent="Probe"]
shape = SubResource("sph")
"#;
    let mut ml = boot(PROBE);
    let probe = node(&ml, "/root/World/Probe");
    let src = "extends RigidBody3D\n\
               var step = -1.0\n\
               var vel_read = Vector3()\n\
               func _integrate_forces(state):\n\
               \x20   step = state.get_step()\n\
               \x20   state.set_linear_velocity(Vector3(2, 0, 0))\n\
               \x20   vel_read = state.get_linear_velocity()\n";
    ml.tree_mut().attach_script(
        probe,
        Box::new(gdscene::GDScriptNodeInstance::from_source(src, probe).unwrap()),
    );
    ml.step(DT);
    let errs = errors(&mut ml);
    assert!(errs.is_empty(), "_integrate_forces must not raise: {errs:?}");
    // The demo runs physics at `common/physics_ticks_per_second=120`, so the
    // step is 1/120 — not the 1/60 frame this test advances by.
    let step = float(script_var(&ml, probe, "step"), "step");
    assert!(
        (step - 1.0 / 120.0).abs() < 1e-6,
        "state.get_step() is the project's physics tick 1/120, got {step}"
    );
    assert_eq!(
        script_var(&ml, probe, "vel_read"),
        Variant::Vector3(Vector3::new(2.0, 0.0, 0.0)),
        "set_linear_velocity must write the state get_linear_velocity reads back"
    );
}

// ---------------------------------------------------------------------------
// The follow camera's ray: create(), the server path, and `exclude`
// ---------------------------------------------------------------------------

/// A small body ("Own", top face y = 1) above a floor (top face y = −5). A
/// downward ray from y = 3 hits Own first unless Own is excluded by its RID —
/// exactly how `follow_camera.gd` keeps its rays off the player.
const RAY_SCENE: &str = r#"[gd_scene format=3]
[sub_resource type="BoxShape3D" id="floor"]
size = Vector3(20, 2, 20)
[sub_resource type="BoxShape3D" id="own"]
size = Vector3(2, 2, 2)

[node name="World" type="Node3D"]
[node name="Floor" type="StaticBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, -6, 0)
[node name="Shape" type="CollisionShape3D" parent="Floor"]
shape = SubResource("floor")
[node name="Own" type="StaticBody3D" parent="."]
[node name="Shape" type="CollisionShape3D" parent="Own"]
shape = SubResource("own")
[node name="Probe" type="Node3D" parent="."]
"#;

fn ray_probe(exclude_own: bool) -> (Vec<String>, Variant) {
    let mut ml = boot(RAY_SCENE);
    let probe = node(&ml, "/root/World/Probe");
    let exclude = if exclude_own { "[$\"../Own\".get_rid()]" } else { "[]" };
    let src = format!(
        "extends Node3D\n\
         var hit_y = 999.0\n\
         func _physics_process(_d):\n\
         \x20   var ds := PhysicsServer3D.space_get_direct_state(get_world_3d().get_space())\n\
         \x20   var col := ds.intersect_ray(PhysicsRayQueryParameters3D.create(\n\
         \x20           Vector3(0, 3, 0), Vector3(0, -20, 0), 0xffffffff, {exclude}))\n\
         \x20   if not col.is_empty():\n\
         \x20       hit_y = col.position.y\n"
    );
    ml.tree_mut().attach_script(
        probe,
        Box::new(gdscene::GDScriptNodeInstance::from_source(&src, probe).unwrap()),
    );
    for _ in 0..2 {
        ml.step(DT);
    }
    let errs = errors(&mut ml);
    (errs, script_var(&ml, probe, "hit_y"))
}

/// 🔑 `exclude` is honoured: with Own excluded the ray reaches the floor's top
/// face (y = −5). The control below proves Own IS in the ray's way, so this
/// cannot pass by Own simply not being hit.
#[test]
fn ray_create_with_excluded_rid_passes_through_its_own_body() {
    let (errs, hit_y) = ray_probe(true);
    assert!(errs.is_empty(), "the camera's ray idiom must not raise: {errs:?}");
    let y = float(hit_y, "hit_y");
    assert!(
        (y + 5.0).abs() < 1e-3,
        "with its own body excluded the ray must land on the floor at y = -5, got {y}"
    );
}

/// CONTROL: without the exclusion the same ray stops on Own's top face (y = 1).
#[test]
fn ray_without_exclude_stops_on_the_nearer_body() {
    let (errs, hit_y) = ray_probe(false);
    assert!(errs.is_empty(), "the ray idiom must not raise: {errs:?}");
    let y = float(hit_y, "hit_y");
    assert!(
        (y - 1.0).abs() < 1e-3,
        "without exclude the ray must stop on Own's top face at y = 1, got {y}"
    );
}

// ---------------------------------------------------------------------------
// top_level + look_at_from_position
// ---------------------------------------------------------------------------

/// A `top_level` node's global position is what `look_at_from_position` was
/// given, however its parent is placed. It used to be offset by the parent.
#[test]
fn look_at_from_position_on_a_top_level_node_lands_at_the_given_point() {
    const SCENE: &str = r#"[gd_scene format=3]
[node name="World" type="Node3D"]
[node name="Parent" type="Node3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, -10)
[node name="Cam" type="Camera3D" parent="Parent"]
"#;
    let mut ml = boot(SCENE);
    let cam = node(&ml, "/root/World/Parent/Cam");
    let src = "extends Camera3D\n\
               func _ready():\n\
               \x20   set_as_top_level(true)\n\
               \x20   look_at_from_position(Vector3(5, 1, 0), Vector3(0, 1, -20), Vector3.UP)\n";
    ml.tree_mut().attach_script(
        cam,
        Box::new(gdscene::GDScriptNodeInstance::from_source(src, cam).unwrap()),
    );
    ml.tree_mut().process_script_ready(cam);
    let errs = errors(&mut ml);
    assert!(errs.is_empty(), "{errs:?}");
    let at = global_origin(&ml, cam);
    assert!(
        (at - Vector3::new(5.0, 1.0, 0.0)).length() < 1e-4,
        "a top_level node must land exactly where look_at_from_position put it, got {at:?}"
    );
    // And it faces the target: −Z (Godot's forward) points from eye to target.
    let basis = gdscene::node3d::get_global_transform(ml.tree(), cam).basis;
    let forward = Vector3::new(-basis.z.x, -basis.z.y, -basis.z.z);
    let want = (Vector3::new(0.0, 1.0, -20.0) - Vector3::new(5.0, 1.0, 0.0)).normalized();
    assert!(
        forward.dot(want) > 0.999,
        "the camera must look at the target; forward {forward:?} vs {want:?}"
    );
}
