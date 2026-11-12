//! pat-9s7yp — `await RenderingServer.frame_post_draw` resumes AFTER the frame
//! is drawn, between it and the next frame, as in Godot.
//!
//! truck_town's `car_select.gd` (games/truck_town, the official demo):
//!
//! ```gdscript
//! func _load_scene(car_scene: PackedScene) -> void:
//!     loading_screen.visible = true
//!     await RenderingServer.frame_post_draw
//!     var car: Node3D = car_scene.instantiate()
//!     ...
//!     get_parent().add_child(town)
//! ```
//!
//! Godot's `Main::iteration` emits `frame_post_draw` from
//! `RenderingServer::draw`, after the process phase and the deferred calls (and
//! before `_process_frames++`). Patina made it a one-frame await resumed in the
//! coroutine phase right after `_process` of the SAME frame, so the car was in
//! the tree one frame early: f109 against Godot's f110.
//!
//! The timeline below is Godot 4.6.1's own, recorded with the reference
//! capture (`node scripts/godot-reference.mjs <this probe as a project> --keys
//! "wait:100,Enter:50,wait:100" --watch /root/Main:<each var>`), whose frame
//! numbering and input timing are the MCP `play` tool's: Enter is pressed
//! before frame 7 steps, and frame f is the state after f steps.
//!
//! | frame | awaited_at | resumed_at | spawned | processed_since_resume | ready_resumed_at |
//! |-------|-----------:|-----------:|--------:|-----------------------:|-----------------:|
//! | f1    | -1         | -1         | 0       | 0                      | -1               |
//! | f2    | -1         | -1         | 0       | 0                      | 0                |
//! | f7    | 6          | -1         | 0       | 0                      | 0                |
//! | f8    | 6          | 6          | 1       | 1                      | 0                |
//! | f9    | 6          | 6          | 1       | 2                      | 0                |

use gdplatform::input::{InputEvent, Key};
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;
use gdvariant::Variant;

const SCRIPT: &str = "extends Node

var awaited_at := -1
var resumed_at := -1
var spawned := 0
var processed_since_resume := 0
var ready_resumed_at := -1


func _ready() -> void:
\t_await_in_ready()


func _await_in_ready() -> void:
\tawait RenderingServer.frame_post_draw
\tready_resumed_at = Engine.get_process_frames()


func _unhandled_input(event: InputEvent) -> void:
\tif event is InputEventKey and event.pressed and not event.echo and event.keycode == KEY_ENTER:
\t\t_load_scene()


func _process(_delta: float) -> void:
\tif spawned:
\t\tprocessed_since_resume += 1


func _load_scene() -> void:
\tawaited_at = Engine.get_process_frames()
\tawait RenderingServer.frame_post_draw
\tresumed_at = Engine.get_process_frames()
\tvar n := Node.new()
\tn.name = \"Spawned\"
\tadd_child(n)
\tspawned = 1
";

static ROOTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn boot(dir: &std::path::Path) -> MainLoop {
    gdobject::class_db::register_all_classes();
    gdcore::file_access::set_path_roots(Some(dir.to_path_buf()), None);
    let packed =
        PackedScene::from_tscn(&std::fs::read_to_string(dir.join("main.tscn")).unwrap()).unwrap();
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
    MainLoop::new(tree)
}

fn key(pressed: bool) -> InputEvent {
    InputEvent::Key {
        key: Key::Enter,
        pressed,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

#[test]
fn frame_post_draw_resumes_between_frames_on_godots_timeline() {
    let _roots = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.godot"),
        "config_version=5\n[application]\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.gd"), SCRIPT).unwrap();
    std::fs::write(
        dir.path().join("main.tscn"),
        "[gd_scene format=3]\n\n[ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1\"]\n\n[node name=\"Main\" type=\"Node\"]\nscript = ExtResource(\"1\")\n",
    )
    .unwrap();
    let mut ml = boot(dir.path());
    let main = ml.tree().get_node_by_path("/root/Main").expect("Main");

    let names = [
        "awaited_at",
        "resumed_at",
        "spawned",
        "processed_since_resume",
        "ready_resumed_at",
    ];
    // Godot 4.6.1's record (see the module docs), frames 1..=9.
    let godot: [[i64; 5]; 9] = [
        [-1, -1, 0, 0, -1],
        [-1, -1, 0, 0, 0],
        [-1, -1, 0, 0, 0],
        [-1, -1, 0, 0, 0],
        [-1, -1, 0, 0, 0],
        [-1, -1, 0, 0, 0],
        [6, -1, 0, 0, 0],
        [6, 6, 1, 1, 0],
        [6, 6, 1, 2, 0],
    ];
    for f in 1..=9usize {
        // MCP `play`: an input {start: 7, end: 9} is pressed before frame 7
        // steps and released after frame 9.
        if f == 7 {
            ml.push_event(key(true));
        }
        ml.step(1.0 / 60.0);
        let script = ml.tree().get_script(main).expect("main.gd");
        let got: Vec<i64> = names
            .iter()
            .map(|n| match script.get_property(n) {
                Some(Variant::Int(i)) => i,
                other => panic!("{n} = {other:?}"),
            })
            .collect();
        assert_eq!(
            got,
            godot[f - 1],
            "f{f}: {names:?} must match Godot's record — a resume visible one frame EARLY \
             (spawned at f7) means frame_post_draw resumed inside the process phase"
        );
        if f == 9 {
            ml.push_event(key(false));
        }
    }
    assert!(ml.tree().get_node_by_path("/root/Main/Spawned").is_some());
    let out = ml.tree_mut().drain_script_output();
    assert!(
        out.errors.is_empty(),
        "no script errors, got {:?}",
        out.errors
    );
}
