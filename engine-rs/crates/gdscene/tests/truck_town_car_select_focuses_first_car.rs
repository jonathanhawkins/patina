//! truck_town's car-select menu (`games/truck_town`, the official Godot demo
//! `3d/truck_town`, unmodified) boots without a script error and focuses the
//! first car, as real Godot 4.6.1 does.
//!
//! `car_select.gd` `_ready` starts with
//! `car_container.get_child(0).grab_focus.call_deferred()`: `grab_focus`
//! with no call is a `Callable` bound to the Button. Patina answered `Nil` for
//! a native method named as a member of ANOTHER object (only bareword methods
//! of the running script became Callables), so `_ready` died with "cannot
//! call method on Nil" on its first line, nothing was focused, and Enter
//! (ui_accept) on the menu did nothing.
//! Repro: `node scripts/scene-audit.mjs games/truck_town`.

use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture")
}

/// Boots `car_select/car_select.tscn` the way the runner does (same as
/// `platformer_3d_movement_matches_godot.rs`).
fn boot_menu() -> MainLoop {
    boot(&game_dir(), "car_select/car_select.tscn")
}

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

#[test]
fn car_select_ready_runs_clean_and_focuses_the_first_car() {
    let _roots = roots_lock();
    let mut ml = boot_menu();
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }
    let out = ml.tree_mut().drain_script_output();
    assert!(
        out.errors.is_empty(),
        "car_select.gd must run without script errors, got {:?}",
        out.errors
    );
    let mini_van = ml
        .tree()
        .get_node_by_path("/root/CarSelect/CarContainer/MiniVan")
        .expect("MiniVan button");
    assert_eq!(
        ml.tree().focus_owner(),
        Some(mini_van),
        "Godot focuses the first car (car_container.get_child(0).grab_focus.call_deferred())"
    );
}

/// The rest of car_select.gd `_load_scene`, which the real game cannot reach
/// yet (town_scene.gd `setup` then fails on `%WorldEnvironment.environment`,
/// a separate root cause): show the loading panel,
/// `await RenderingServer.frame_post_draw`, instance the town, set
/// `town.mood = town.Mood.DAY`, add it. Two bugs stopped it:
/// `RenderingServer.frame_post_draw` raised "undefined variable:
/// 'RenderingServer'", and a named enum read through ANOTHER object
/// (`town.Mood`) was Nil. Same shape as the game's script, cut to those lines.
#[test]
fn load_scene_awaits_frame_post_draw_and_reads_the_towns_enum() {
    let _roots = roots_lock();
    let dir = tempfile::tempdir().unwrap();
    let w = |name: &str, text: &str| std::fs::write(dir.path().join(name), text).unwrap();
    w(
        "project.godot",
        "config_version=5\n[application]\nrun/main_scene=\"res://menu.tscn\"\n",
    );
    w(
        "town.gd",
        "extends Node3D\n\nenum Mood {\n\tSUNRISE,\n\tDAY,\n\tSUNSET,\n}\n\nvar mood := Mood.SUNRISE\n",
    );
    w(
        "town.tscn",
        "[gd_scene format=3]\n\n[ext_resource type=\"Script\" path=\"res://town.gd\" id=\"1\"]\n\n[node name=\"TownScene\" type=\"Node3D\"]\nscript = ExtResource(\"1\")\n",
    );
    w(
        "menu.gd",
        "extends Control\n\nvar town: Node3D = null\n\nfunc _ready() -> void:\n\t_load_scene()\n\nfunc _load_scene() -> void:\n\tvisible = true\n\tawait RenderingServer.frame_post_draw\n\ttown = preload(\"res://town.tscn\").instantiate()\n\ttown.mood = town.Mood.DAY\n\tget_parent().add_child(town)\n\thide()\n",
    );
    w(
        "menu.tscn",
        "[gd_scene format=3]\n\n[ext_resource type=\"Script\" path=\"res://menu.gd\" id=\"1\"]\n\n[node name=\"CarSelect\" type=\"Control\"]\nscript = ExtResource(\"1\")\n",
    );
    let mut ml = boot(dir.path(), "menu.tscn");
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }
    let out = ml.tree_mut().drain_script_output();
    assert!(
        out.errors.is_empty(),
        "no script errors, got {:?}",
        out.errors
    );
    let town = ml
        .tree()
        .get_node_by_path("/root/TownScene")
        .expect("the town is added once the frame_post_draw await resumes");
    let mood = ml
        .tree()
        .get_script(town)
        .and_then(|s| s.get_property("mood"));
    assert_eq!(
        mood,
        Some(gdvariant::Variant::Int(1)),
        "town.mood = town.Mood.DAY (1)"
    );
}
