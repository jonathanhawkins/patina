//! pat-mgixk.62 acceptance: a scene change replaces only the current scene.
//!
//! # What was wrong
//!
//! All three scene-change entry points — `change_scene_to_packed`,
//! `change_scene_to_node` and `unload_current_scene` — removed **every child of
//! `/root`**. Autoloads *are* root children (`autoload.rs:189/203`), so
//! `get_tree().change_scene_to_file(..)` **deleted every singleton in the
//! project**. Godot frees only `current_scene`
//! (`scene/main/scene_tree.cpp:1647-1740`); surviving scene changes is the whole
//! purpose of an autoload.
//!
//! Two more gaps on the same path:
//!
//! * The deferred apply in `MainLoop::step` swapped the tree and stopped. The
//!   new scene arrived with `_script_path` recorded and no live script, so
//!   nothing in it ran — no `_ready`, no `_process`. The attach loop existed
//!   only at boot.
//! * `SceneTree.scene_changed` is registered in the ClassDB and was never
//!   emitted: `grep '"scene_changed"' crates/gdscene/src` returned 0.
//!
//! # The path driven
//!
//! ```text
//! project.godot (+ autoload) -> EngineBootstrap::run_all
//!   -> A's _ready -> get_tree().change_scene_to_file("res://b.tscn")
//!     -> MainLoop::step -> process_deferred_scene_change
//!       -> attach_subtree_scripts_from_res -> enter_tree_with_scripts -> B's _ready
//! ```

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `set_path_roots` is process-global and `change_scene_to_file` resolves
/// `res://` through it, so these tests must not interleave.
///
/// ⚠️ `EngineBootstrap` does NOT install the path roots — `grep set_path_roots
/// crates/patina-runner/src/bootstrap.rs` is empty, only `boot_project` does it.
/// A test driving a scene change through bootstrap has to set them itself or
/// `res://b.tscn` resolves to a bare relative path and the change fails.
static RES_ROOT: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("patina_mgixk62_{tag}_{}_{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn script_var(
    tree: &gdscene::SceneTree,
    node_id: gdscene::node::NodeId,
    var: &str,
) -> Option<Variant> {
    tree.get_script(node_id)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
}

/// The autoload. Its `_ready` writes `score`, so the value under test is one the
/// autoload's own live script produced — not a value the test poked in, which
/// would survive in the node's property bag even if the script instance were
/// destroyed and the node rebuilt.
const GLOBAL_GD: &str = "\
extends Node
var score = 0
func _ready():
\tscore = 3
";

/// Scene A changes to B from `_ready`, which is where a real game does it.
const A_GD: &str = "\
extends Node2D
func _ready():
\tget_tree().change_scene_to_file(\"res://b.tscn\")
";

/// Scene B counts its own `_ready` calls, so "ran" and "ran exactly once" are
/// different observations.
const B_GD: &str = "\
extends Node2D
var ready_count = 0
func _ready():
\tready_count = ready_count + 1
";

fn scene_tscn(root_name: &str, script: &str) -> String {
    format!(
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://{script}\" id=\"1_s\"]\n\n\
         [node name=\"{root_name}\" type=\"Node2D\"]\n\
         script = ExtResource(\"1_s\")\n"
    )
}

fn write_fixture(dir: &std::path::Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\n\
         config/name=\"SceneChangeFixture\"\n\
         run/main_scene=\"res://a.tscn\"\n\
         \n\
         [autoload]\n\
         Global=\"*res://global.gd\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("global.gd"), GLOBAL_GD).unwrap();
    std::fs::write(dir.join("a.gd"), A_GD).unwrap();
    std::fs::write(dir.join("b.gd"), B_GD).unwrap();
    std::fs::write(dir.join("a.tscn"), scene_tscn("A", "a.gd")).unwrap();
    std::fs::write(dir.join("b.tscn"), scene_tscn("B", "b.gd")).unwrap();
}

/// A connection that counts how many times its signal fired. Copied from
/// `main_loop.rs`'s own `scene_tree_and_node_signals_fire_via_main_loop`, which
/// is how every other `SceneTree` signal is observed in this codebase.
fn recorder(cell: &Arc<AtomicUsize>) -> gdobject::signal::Connection {
    let c = cell.clone();
    gdobject::signal::Connection::with_callback(
        gdcore::id::ObjectId::from_raw(1),
        "on_scene_changed",
        move |_args| {
            c.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        },
    )
}

/// **The acceptance.**
#[test]
fn change_scene_keeps_autoloads_and_runs_new_scripts() {
    let _guard = RES_ROOT.lock().unwrap_or_else(|p| p.into_inner());
    let dir = unique_project_dir("change");
    write_fixture(&dir);
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);

    let mut boot = EngineBootstrap::new(BootConfig {
        main_scene: Some(dir.join("a.tscn")),
        project_dir: dir.clone(),
        headless: true,
        ..BootConfig::default()
    });
    boot.run_all().expect("the fixture project boots");

    // A's `_ready` already ran during boot (`init_lifecycle` calls
    // `LifecycleManager::enter_tree` on the main scene), so the change is queued
    // before the first step.
    let ml = boot.main_loop_mut().expect("main loop built");
    assert!(
        ml.tree().has_pending_scene_change(),
        "A's _ready must have queued the change; without it every assertion \
         below would be testing a scene that never changed",
    );
    let a_id = ml
        .tree()
        .get_node_by_path("/root/A")
        .expect("scene A is instanced at boot");
    let global_id = ml
        .tree()
        .get_node_by_path("/root/Global")
        .expect("the autoload is instanced at boot");
    assert_eq!(
        script_var(ml.tree(), global_id, "score"),
        Some(Variant::Int(3)),
        "precondition: the autoload's own _ready ran and set score",
    );

    // Connected BEFORE the swap, on the root — the store `MainLoop` already
    // emits `process_frame` on.
    let changed = Arc::new(AtomicUsize::new(0));
    let root = ml.tree().root_id();
    ml.tree_mut()
        .connect_signal(root, "scene_changed", recorder(&changed));

    // Two frames, as the acceptance specifies. The swap lands on the first; the
    // second is what makes "ran once" distinguishable from "runs every frame".
    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);

    // -- 1. the autoload SURVIVED, with its state ----------------------------
    //
    // 🔑 The whole P0. Before this bead the scene change removed every child of
    // /root, so /root/Global was gone and every autoload in the project died
    // with it.
    let global_after = ml
        .tree()
        .get_node_by_path("/root/Global")
        .expect("/root/Global must survive a scene change — autoloads are root children, and the change used to delete every one of them");
    assert_eq!(
        global_after, global_id,
        "it must be the SAME node, not a fresh one: a rebuilt autoload would \
         lose every bit of state the game put in it, which is the same bug \
         wearing a different shape",
    );
    assert_eq!(
        script_var(ml.tree(), global_after, "score"),
        Some(Variant::Int(3)),
        "and its live script must still hold score = 3",
    );

    // -- 2. the old scene is gone, the new one is current --------------------
    assert!(
        ml.tree().get_node_by_path("/root/A").is_none(),
        "the outgoing scene must be freed — 'keeps autoloads' must not become \
         'keeps everything', which would stack scenes on top of each other",
    );
    assert!(
        ml.tree().get_node(a_id).is_none(),
        "and freed by id, not merely detached from the path",
    );
    let b_id = ml
        .tree()
        .get_node_by_path("/root/B")
        .expect("scene B must be instanced");
    assert_eq!(
        ml.tree().current_scene(),
        Some(b_id),
        "get_tree().current_scene must be B",
    );

    // -- 3. B's scripts are live and _ready ran exactly once -----------------
    //
    // 🔑 This is the half the deferred apply skipped entirely: it swapped the
    // tree and attached nothing, so B arrived with `_script_path` set and no
    // script instance. `None` here means no script was attached at all.
    assert_eq!(
        script_var(ml.tree(), b_id, "ready_count"),
        Some(Variant::Int(1)),
        "B's script must be attached and its _ready must have run EXACTLY once \
         across two frames. None = no script attached (the pre-bead state); 0 = \
         attached but never entered the tree; 2+ = the lifecycle is re-run every \
         frame",
    );

    // -- 4. scene_changed fired once ----------------------------------------
    assert_eq!(
        changed.load(Ordering::SeqCst),
        1,
        "SceneTree.scene_changed must fire once for one scene change. 0 is the \
         pre-bead state (registered in the ClassDB, emitted nowhere); 2 would \
         mean it is emitted per frame rather than per change",
    );

    // -- 5. reload rebuilds B ------------------------------------------------
    let reloaded = ml
        .tree_mut()
        .reload_current_scene()
        .expect("reload_current_scene must work from B — the change recorded B's packed source");
    assert_ne!(
        reloaded, b_id,
        "a reload must build a NEW instance, not hand back the old node",
    );
    assert_eq!(
        ml.tree().get_node(reloaded).map(|n| n.name().to_owned()),
        Some("B".to_owned()),
        "and the rebuilt scene must still be B",
    );
    assert!(
        ml.tree().get_node_by_path("/root/Global").is_some(),
        "the autoload must survive the reload too — reload goes through the same \
         change path",
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `unload_current_scene` frees the current scene and leaves the autoloads.
///
/// Separate from the acceptance because it is the third entry point that had the
/// same wipe-everything bug, and nothing in the acceptance's path reaches it.
#[test]
fn unload_current_scene_keeps_autoloads() {
    let _guard = RES_ROOT.lock().unwrap_or_else(|p| p.into_inner());
    let dir = unique_project_dir("unload");
    write_fixture(&dir);
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);

    let mut boot = EngineBootstrap::new(BootConfig {
        // Scene B, whose script does NOT change scenes — this test is about
        // unload alone.
        main_scene: Some(dir.join("b.tscn")),
        project_dir: dir.clone(),
        headless: true,
        ..BootConfig::default()
    });
    boot.run_all().expect("the fixture project boots");

    let ml = boot.main_loop_mut().expect("main loop built");
    let b_id = ml
        .tree()
        .get_node_by_path("/root/B")
        .expect("scene B is instanced at boot");
    assert_eq!(
        ml.tree().current_scene(),
        Some(b_id),
        "precondition: boot records the main scene as current, or unload has \
         nothing to free and this test would pass vacuously",
    );

    ml.tree_mut().unload_current_scene();

    assert!(
        ml.tree().get_node(b_id).is_none(),
        "unload must free the current scene",
    );
    assert_eq!(
        ml.tree().current_scene(),
        None,
        "and leave no current scene",
    );
    assert!(
        ml.tree().get_node_by_path("/root/Global").is_some(),
        "but the autoload must remain — unload_current_scene used to remove \
         every child of /root",
    );

    let _ = std::fs::remove_dir_all(&dir);
}
