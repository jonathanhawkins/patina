//! truck_town (`games/truck_town`, the official Godot `3d/truck_town` demo,
//! unmodified): every vehicle script attaches, member initializers that read
//! the node see what real Godot 4.6.1 sees, and failures that used to vanish
//! are reported.
//!
//! - pat-0ywbp: `follow_camera.gd` `var base_fov := fov` and
//!   `var initial_transform := transform`, `vehicle.gd`
//!   `var previous_speed := linear_velocity.length()` raised UndefinedVariable
//!   at script load, so the Camera3D and the VehicleBody3D ran with NO script.
//!   Godot evaluates initializers when the script is set on the node — after
//!   the `.tscn` values listed above `script = …`, before `_init` and before
//!   the values listed after it. Real Godot on the probe in
//!   `initializers_see_the_node_before_init_and_before_exported_overrides`
//!   prints `INIT fov=74.0 base_fov=74.0` and
//!   `READY base_fov=74.0 origin=(0.0, 2.032, -4.394) … height_seen=1.5 height=1.25`.
//! - pat-kkqnm: a script that fails to parse was only a WARN line; it is now
//!   in the drained script errors as `script failed to load`.
//! - pat-c7i5e: an error inside a coroutine resumed after `await` was
//!   discarded (`let _ = …`) and took the other ready coroutines down with it.

use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;
use gdvariant::Variant;

fn truck_town() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture")
}

/// Boots `scene` from project `dir` the way the runner does (same as
/// `truck_town_car_select_focuses_first_car.rs`).
/// `res://` roots are process-global: plain `cargo test` runs this file's
/// tests on parallel threads, and a second test re-pointing the roots between
/// another's boot and its script reads made that script "not found". Each
/// test holds it for its whole body.
static ROOTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn roots_lock() -> std::sync::MutexGuard<'static, ()> {
    ROOTS.lock().unwrap_or_else(|e| e.into_inner())
}

fn boot(dir: &std::path::Path, scene: &str) -> MainLoop {
    gdobject::class_db::register_all_classes();
    let dir = dir.to_path_buf();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let text = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).unwrap();
    gdscene::project_runtime::seed_script_settings_for_this_thread(&settings, &dir);
    let tscn = std::fs::read_to_string(dir.join(scene)).unwrap();
    let packed = PackedScene::from_tscn(&tscn).unwrap();
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
    let mut ml = MainLoop::new(tree);
    gdscene::project_runtime::apply_project_settings(&mut ml, &settings);
    ml.register_physics_bodies();
    ml
}

fn script_prop(ml: &MainLoop, path: &str, prop: &str) -> Variant {
    let id = ml
        .tree()
        .get_node_by_path(path)
        .unwrap_or_else(|| panic!("node {path}"));
    let script = ml
        .tree()
        .get_script(id)
        .unwrap_or_else(|| panic!("{path} must have its script attached"));
    script
        .get_property(prop)
        .unwrap_or_else(|| panic!("{path} script has no `{prop}`"))
}

fn float(v: Variant) -> f64 {
    match v {
        Variant::Float(f) => f,
        other => panic!("expected Float, got {other:?}"),
    }
}

fn temp_project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("project.godot"),
        "config_version=5\n\n[application]\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

#[test]
fn car_base_camera_and_body_scripts_attach_with_node_initializers() {
    let _roots = roots_lock();
    let mut ml = boot(&truck_town(), "vehicles/car_base.tscn");
    ml.step(1.0 / 60.0);
    let cam = "/root/CarBase/Body/CameraBase/Camera3D";
    // car_base.tscn authors `fov = 74.0` above `script = …`.
    assert_eq!(float(script_prop(&ml, cam, "base_fov")), 74.0, "Godot: base_fov=74.0");
    match script_prop(&ml, cam, "initial_transform") {
        Variant::Transform3D(t) => {
            let o = t.origin;
            assert!(
                (o.x - 0.0).abs() < 1e-5 && (o.y - 2.032).abs() < 1e-5 && (o.z + 4.394).abs() < 1e-5,
                "initial_transform must be the authored camera transform, got origin {o:?}"
            );
        }
        other => panic!("initial_transform must be a Transform3D, got {other:?}"),
    }
    // An exported override listed after `script = …` still applies.
    assert_eq!(float(script_prop(&ml, cam, "height")), 1.25);
    // The VehicleBody3D starts at rest: linear_velocity.length() == 0.
    assert_eq!(float(script_prop(&ml, "/root/CarBase/Body", "previous_speed")), 0.0);

    let out = ml.tree_mut().drain_script_output();
    for e in &out.errors {
        assert!(
            !e.contains("script failed to load") && !e.contains("member initializer"),
            "no truck_town script may fail to load: {e}"
        );
    }
}

#[test]
fn initializers_see_the_node_before_init_and_before_exported_overrides() {
    let _roots = roots_lock();
    // The same probe was run in real Godot 4.6.1; its output is quoted in the
    // module docs. `near` is deliberately not probed: Godot's saver always
    // writes native properties above `script = …`.
    let dir = temp_project(&[
        (
            "cam.gd",
            concat!(
                "extends Camera3D\n",
                "@export var height := 1.5\n",
                "var initial_transform := transform\n",
                "var base_fov := fov\n",
                "var height_seen := height\n",
                "var speed := Vector3(3, 4, 0).length()\n",
                "func _init():\n",
                "\tprint(\"INIT fov=\", fov, \" base_fov=\", base_fov)\n",
                "func _ready():\n",
                "\tprint(\"READY base_fov=\", base_fov, \" origin=\", initial_transform.origin, \" height_seen=\", height_seen, \" height=\", height, \" speed=\", speed)\n",
            ),
        ),
        (
            "main.tscn",
            concat!(
                "[gd_scene load_steps=2 format=3]\n\n",
                "[ext_resource type=\"Script\" path=\"res://cam.gd\" id=\"1\"]\n\n",
                "[node name=\"Root\" type=\"Node3D\"]\n\n",
                "[node name=\"Camera3D\" type=\"Camera3D\" parent=\".\"]\n",
                "transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2.032, -4.394)\n",
                "fov = 74.0\n",
                "script = ExtResource(\"1\")\n",
                "height = 1.25\n",
            ),
        ),
    ]);
    let mut ml = boot(dir.path(), "main.tscn");
    ml.step(1.0 / 60.0);
    let out = ml.tree_mut().drain_script_output();
    assert!(out.errors.is_empty(), "no errors expected, got {:?}", out.errors);
    assert_eq!(
        out.prints,
        vec![
            "INIT fov=74.0 base_fov=74.0".to_string(),
            "READY base_fov=74.0 origin=(0.0, 2.032, -4.394) height_seen=1.5 height=1.25 speed=5.0"
                .to_string(),
        ],
        "must match real Godot 4.6.1's output for the same scene"
    );
}

#[test]
fn a_script_that_fails_to_parse_is_reported_with_its_path_and_line() {
    let _roots = roots_lock();
    let dir = temp_project(&[
        ("broken.gd", "extends Node\n\nfunc _ready():\n\tvar x = = 1\n"),
        (
            "main.tscn",
            concat!(
                "[gd_scene load_steps=2 format=3]\n\n",
                "[ext_resource type=\"Script\" path=\"res://broken.gd\" id=\"1\"]\n\n",
                "[node name=\"Main\" type=\"Node\"]\n",
                "script = ExtResource(\"1\")\n",
            ),
        ),
    ]);
    let mut ml = boot(dir.path(), "main.tscn");
    ml.step(1.0 / 60.0);
    let out = ml.tree_mut().drain_script_output();
    let hit: Vec<&String> = out
        .errors
        .iter()
        .filter(|e| e.contains("script failed to load: res://broken.gd:"))
        .collect();
    assert_eq!(hit.len(), 1, "one load failure for broken.gd, got {:?}", out.errors);
    // Attached while the scene is still detached, so the path is `/Main`.
    assert!(hit[0].contains("Main: script failed to load"), "names the node: {}", hit[0]);
    assert!(hit[0].contains("line 4"), "names the line: {}", hit[0]);
}

#[test]
fn an_error_after_await_is_reported_and_does_not_drop_the_other_coroutines() {
    let _roots = roots_lock();
    let dir = temp_project(&[
        (
            "main.gd",
            concat!(
                "extends Node\n",
                "signal go\n",
                "func _ready():\n",
                "\tfails()\n",
                "\tsurvives()\n",
                "func fails():\n",
                "\tawait go\n",
                "\tvar thing = null\n",
                "\tthing.value = 1\n",
                "func survives():\n",
                "\tawait go\n",
                "\tprint(\"survivor resumed\")\n",
            ),
        ),
        (
            "main.tscn",
            concat!(
                "[gd_scene load_steps=2 format=3]\n\n",
                "[ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1\"]\n\n",
                "[node name=\"Main\" type=\"Node\"]\n",
                "script = ExtResource(\"1\")\n",
            ),
        ),
    ]);
    let mut ml = boot(dir.path(), "main.tscn");
    ml.step(1.0 / 60.0);
    let before = ml.tree_mut().drain_script_output();
    assert!(before.errors.is_empty(), "nothing fails before the await: {:?}", before.errors);
    let main = ml.tree().get_node_by_path("/root/Main").unwrap();
    ml.tree_mut().emit_signal(main, "go", &[]);
    ml.step(1.0 / 60.0);
    let out = ml.tree_mut().drain_script_output();
    let hit: Vec<&String> = out
        .errors
        .iter()
        .filter(|e| e.starts_with("/root/Main: fails(): "))
        .collect();
    assert_eq!(hit.len(), 1, "the resumed error is reported once, got {:?}", out.errors);
    assert!(hit[0].contains("line 9"), "names the failing line: {}", hit[0]);
    assert!(
        out.prints.iter().any(|p| p == "survivor resumed"),
        "the coroutine resumed after the failing one must still run, prints {:?}",
        out.prints
    );
}

#[test]
fn a_callee_that_raises_returns_null_and_the_caller_keeps_running() {
    // truck_town's car_select.gd `_load_scene` calls `town.setup(car, …)`; one
    // line inside `setup` failed and the CALLER died too ("Native method
    // 'Node3D.setup' is registered in ClassDB but not implemented"), so the
    // town was never added to the tree. Godot aborts only the callee and hands
    // the caller null.
    let _roots = roots_lock();
    let dir = temp_project(&[
        (
            "main.gd",
            concat!(
                "extends Node3D\n",
                "func _ready():\n",
                "\tvar r = $Child.setup()\n",
                "\tprint(\"caller continued \", r)\n",
            ),
        ),
        (
            "child.gd",
            concat!(
                "extends Node3D\n",
                "func setup():\n",
                "\tprint(\"in setup\")\n",
                "\tvar thing = null\n",
                "\tthing.value = 1\n",
                "\tprint(\"never\")\n",
            ),
        ),
        (
            "main.tscn",
            concat!(
                "[gd_scene load_steps=3 format=3]\n\n",
                "[ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1\"]\n",
                "[ext_resource type=\"Script\" path=\"res://child.gd\" id=\"2\"]\n\n",
                "[node name=\"Main\" type=\"Node3D\"]\n",
                "script = ExtResource(\"1\")\n\n",
                "[node name=\"Child\" type=\"Node3D\" parent=\".\"]\n",
                "script = ExtResource(\"2\")\n",
            ),
        ),
    ]);
    let mut ml = boot(dir.path(), "main.tscn");
    ml.step(1.0 / 60.0);
    let out = ml.tree_mut().drain_script_output();
    assert_eq!(
        out.prints,
        vec!["in setup".to_string(), "caller continued <null>".to_string()],
        "errors: {:?}",
        out.errors
    );
    assert!(
        out.errors.iter().any(|e| e.starts_with("/root/Main/Child: setup(): ")),
        "the callee's error is reported against the callee, got {:?}",
        out.errors
    );
    assert!(
        !out.errors.iter().any(|e| e.contains("not implemented")),
        "the caller must not fail on the native fallback, got {:?}",
        out.errors
    );
}
