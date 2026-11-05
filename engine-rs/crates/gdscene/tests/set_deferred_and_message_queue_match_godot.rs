//! `set_deferred` and the deferred-call queue against REAL Godot 4.6.1
//! (pat-ivcj2).
//!
//! `set_deferred(&"mood", mood)` never applied: the flush wrote the value
//! into the node's property BAG, which nothing reads for a script variable
//! (it lives on the script instance) or for `name` (a struct field), so the
//! setter never ran; the `&"mood"` StringName spelling did not even queue.
//! truck_town's `town_scene.gd` relies on exactly that to run `set_mood` once
//! the tree exists, so its mood/headlight logic was dead.
//!
//! The order test replays a probe through Godot's MessageQueue rules and
//! compares the WHOLE trace with what `Godot --headless --fixed-fps 60
//! --quit-after 4` printed for the same project (captured 2026-11-05, Godot
//! 4.6.1.stable.official.14d19694e):
//! - FIFO, and a call queued during a flush runs in the same flush;
//! - `add_child.call_deferred(n)` (a bareword NATIVE method as a Callable);
//! - a deferred setter runs with the tree available (`is_inside_tree()`);
//! - a flush right after the `process_frame` signal, BEFORE `_process`;
//! - a node `queue_free`d in `_physics_process` is gone by that frame's
//!   `_process` (Godot's `physics_process` ends with `_flush_delete_queue`).

use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;

/// `res://` roots are process-global; each test holds this for its body.
static ROOTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn roots_lock() -> std::sync::MutexGuard<'static, ()> {
    ROOTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Boots `scene` from `dir` the way patina-runner does (same as
/// `truck_town_car_select_focuses_first_car.rs`).
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

fn write_project(dir: &std::path::Path, script: &str) {
    let w = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    w(
        "project.godot",
        "config_version=5\n\n[application]\nrun/main_scene=\"res://main.tscn\"\n",
    );
    w(
        "main.tscn",
        "[gd_scene format=3]\n\n[ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1\"]\n\n\
         [node name=\"Main\" type=\"Node\"]\nscript = ExtResource(\"1\")\n\n\
         [node name=\"Victim\" type=\"Node\" parent=\".\"]\n",
    );
    w("main.gd", script);
}

const ORDER_PROBE: &str = r#"extends Node

var mood := 0: set = set_mood
var plain := 0
var frame := 0

func L(s):
	print("[f", frame, "] ", s)

func _ready() -> void:
	get_tree().process_frame.connect(_on_pf)
	get_tree().physics_frame.connect(_on_phf)
	L("ready start")
	set_deferred(&"mood", 5)
	set_deferred("plain", 7)
	call_deferred("_a", 1)
	_b.call_deferred(2)
	Callable(self, "_a").call_deferred(3)
	var n := Node.new()
	n.name = "Added"
	add_child.call_deferred(n)
	call_deferred("_nest")
	set_deferred(&"name", "Renamed")
	L("ready end mood=%d plain=%d" % [mood, plain])

func set_mood(v) -> void:
	mood = v
	L("setter %d in_tree=%s" % [v, is_inside_tree()])

func _a(x):
	L("_a %d has_Added=%s name=%s mood=%d plain=%d" % [x, has_node("Added"), name, mood, plain])

func _b(x):
	L("_b %d" % x)

func _nest():
	L("_nest queues _a(9)")
	call_deferred("_a", 9)
	L("_nest after queue")

func _on_pf():
	if frame < 3:
		L("process_frame signal")
	if frame == 1:
		call_deferred("_a", 100)

func _on_phf():
	if frame < 3:
		L("physics_frame signal")
	if frame == 1:
		call_deferred("_a", 200)

func _physics_process(_d):
	if frame < 3:
		L("_physics_process victim=%s" % [get_node_or_null("Victim") != null])
	if frame == 1:
		call_deferred("_a", 300)
		$Victim.queue_free()

func _process(_d):
	if frame < 3:
		L("_process victim=%s" % [get_node_or_null("Victim") != null])
	if frame == 1:
		call_deferred("_a", 400)
	frame += 1
"#;

/// Godot 4.6.1's stdout for [`ORDER_PROBE`], three frames, one physics tick
/// per frame (`--fixed-fps 60`).
const GODOT_ORDER: &[&str] = &[
    "[f0] ready start",
    "[f0] ready end mood=0 plain=0",
    "[f0] physics_frame signal",
    "[f0] _physics_process victim=true",
    "[f0] setter 5 in_tree=true",
    "[f0] _a 1 has_Added=false name=Main mood=5 plain=7",
    "[f0] _b 2",
    "[f0] _a 3 has_Added=false name=Main mood=5 plain=7",
    "[f0] _nest queues _a(9)",
    "[f0] _nest after queue",
    "[f0] _a 9 has_Added=true name=Renamed mood=5 plain=7",
    "[f0] process_frame signal",
    "[f0] _process victim=true",
    "[f1] physics_frame signal",
    "[f1] _physics_process victim=true",
    "[f1] _a 200 has_Added=true name=Renamed mood=5 plain=7",
    "[f1] _a 300 has_Added=true name=Renamed mood=5 plain=7",
    "[f1] process_frame signal",
    "[f1] _a 100 has_Added=true name=Renamed mood=5 plain=7",
    "[f1] _process victim=false",
    "[f2] _a 400 has_Added=true name=Renamed mood=5 plain=7",
    "[f2] physics_frame signal",
    "[f2] _physics_process victim=false",
    "[f2] process_frame signal",
    "[f2] _process victim=false",
];

#[test]
fn deferred_calls_and_set_deferred_run_in_godots_order() {
    let _roots = roots_lock();
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path(), ORDER_PROBE);
    let mut ml = boot(dir.path(), "main.tscn");
    let mut prints = Vec::new();
    let mut errors = Vec::new();
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
        let out = ml.tree_mut().drain_script_output();
        prints.extend(out.prints);
        errors.extend(out.errors);
    }
    // Boot-time prints (`_ready`) are drained with the first frame's.
    assert!(
        errors.is_empty(),
        "the probe raises nothing in Godot, got {errors:?}"
    );
    assert_eq!(
        prints, GODOT_ORDER,
        "trace must match Godot 4.6.1 line for line"
    );
}

/// The real game: `town_scene.gd` `_ready` calls `set_deferred(&"mood",
/// mood)` so `set_mood` runs once the tree exists. Godot runs it at the end
/// of the first frame: `$AmbientSound.stream = ambient_sound[p_mood]` (line
/// 105) assigns mood_day.ogg, and — the scene being run standalone, with no
/// car — line 114 `get_nodes_in_group(&"car")[0]` raises "Out of bounds get
/// index '0'" inside `set_mood` (scene-audit's Godot capture).
#[test]
fn truck_town_set_deferred_mood_runs_set_mood() {
    let _roots = roots_lock();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture");
    let mut ml = boot(&dir, "town/town_scene.tscn");
    let sound = ml
        .tree()
        .get_node_by_path("/root/TownScene/AmbientSound")
        .expect("AmbientSound");
    // The .tscn authors no stream: only `set_mood` assigns one.
    let before = format!(
        "{:?}",
        ml.tree().get_node(sound).unwrap().get_property("stream")
    );
    assert!(
        !before.contains(".ogg"),
        "no stream before the deferred setter runs, got {before}"
    );
    ml.step(1.0 / 60.0);
    let stream = ml.tree().get_node(sound).unwrap().get_property("stream");
    let text = format!("{stream:?}");
    assert!(
        text.contains("mood_day.ogg"),
        "set_mood(Mood.DAY) assigns mood_day.ogg as Godot does, got {text}"
    );
    let errors = ml.tree_mut().drain_script_output().errors;
    assert!(
        errors
            .iter()
            .any(|e| e.contains("set_mood()") && e.contains("index out of bounds: 0")),
        "set_mood reaches line 114 and raises the out-of-bounds Godot raises, got {errors:?}"
    );
}
