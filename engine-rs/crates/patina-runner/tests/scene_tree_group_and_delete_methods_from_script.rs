//! pat-mgixk.59 (remaining scope) — the `get_tree()` method surface beyond the
//! handle itself.
//!
//! The bead's first pass made `get_tree()` a real object: properties, signals
//! and the methods `try_scene_tree_method` already knew. It was deliberately
//! left open because roughly a dozen `SceneTree` methods still had no binding
//! at all, so a script calling them raised `UndefinedFunction`.
//!
//! This covers the ones with real engine backing:
//!
//! | method                 | forwards to                          |
//! |------------------------|--------------------------------------|
//! | `queue_delete(obj)`    | `SceneTree::queue_free`              |
//! | `notify_group_flags()` | `SceneTree::notify_group_flags`      |
//! | `set_current_scene(n)` | `SceneTree::set_current_scene`       |
//!
//! # Why a new test file rather than extending the first one
//!
//! ⚠️ `get_tree_and_get_viewport_are_live_objects` already passes at HEAD. This
//! work is purely additive, so it cannot break that test — reporting it as the
//! verification would make the red check `NotDetected` and the bead would be
//! rejected for proving nothing. A method surface that did not exist needs its
//! own test, which fails at HEAD because the first call raises and aborts
//! `_process`.
//!
//! # Why every effect is asserted from Rust
//!
//! ⚠️ The script sets one flag (`ran`) and otherwise reports nothing. Each
//! method's effect is read out of the tree instead: a node that is gone, the
//! tree's `current_scene`, a notification recorded on the target. A script that
//! asserted its own success could only tell us the call returned, not that it
//! did anything — and all three of these return void or an Error code.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` mutates the process-global `res://` roots, so serialize under
/// `cargo test` (one process, many threads). `nextest` isolates per process.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const DT: f64 = 1.0 / 60.0;

/// The notification code the script broadcasts to the group. Deliberately not a
/// real Godot constant, so it cannot be confused with one the engine delivers
/// on its own (ENTER_TREE, READY, …) — if this shows up, only the script put it
/// there.
const CUSTOM_NOTIFICATION: i32 = 9041;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk59b_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A project whose main scene holds a node to delete and a grouped node to
/// notify, so each call has a distinct, separately observable target.
fn write_project(dir: &Path, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_main\"]\n\n\
         [node name=\"Main\" type=\"Node2D\"]\nscript = ExtResource(\"1_main\")\n\n\
         [node name=\"Victim\" type=\"Node2D\" parent=\".\"]\n\n\
         [node name=\"Listener\" type=\"Node2D\" parent=\".\" groups=[\"watched\"]]\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.gd"), gd).unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");
    boot
}

fn script_var(boot: &EngineBootstrap, var: &str) -> Variant {
    let tree = boot.main_loop().unwrap().tree();
    let id = tree
        .get_node_by_path("/root/Main")
        .expect("the main scene root is in the tree");
    tree.get_script(id)
        .expect("Main has a script")
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
        .unwrap_or(Variant::Nil)
}

/// Calls the three previously unbound methods in one frame.
///
/// ⚠️ The script registers the group ITSELF rather than relying on the
/// fixture's `groups=["watched"]`. A group declared in a `.tscn` reaches the
/// node's own list but NOT the tree-level index that `notify_group`/`call_group`
/// resolve against (`SceneTree::nodes_in_group_ordered` reads `self.groups`,
/// while the instancing path calls `Node::add_to_group`, and nothing syncs them
/// at `enter_tree`). That is a separate defect, reported rather than fixed
/// here; registering through `Node.add_to_group` — which DOES populate both —
/// keeps this test measuring the `notify_group_flags` binding instead of that
/// gap.
///
/// The asymmetry is what makes the gap easy to miss: `is_in_group` reads the
/// node's own list, so a script can confirm membership while every group CALL
/// silently reaches nobody.
///
/// ⚠️ `ran` is set LAST. Every call above it is a method that did not exist, so
/// any one of them raising aborts `_process` and leaves `ran` false — which is
/// exactly the state at HEAD and makes the flag a precise failure signal rather
/// than a vague one.
const GD: &str = "\
extends Node2D
var ran = false
var done = false

func _process(delta):
    if self.done:
        return
    self.done = true
    get_node(\"Listener\").add_to_group(\"watched\")
    get_tree().queue_delete(get_node(\"Victim\"))
    get_tree().notify_group_flags(0, \"watched\", 9041)
    get_tree().set_current_scene(get_node(\"Listener\"))
    self.ran = true
";

/// **The acceptance test for the remaining scope.**
#[test]
fn scene_tree_group_and_delete_methods_from_script() {
    let _guard = BOOT_LOCK.lock().unwrap();
    let dir = unique_project_dir("surface");
    write_project(&dir, GD);
    let mut boot = boot(&dir);

    // Capture the Listener's id before stepping: `set_current_scene` is asserted
    // against it, and after the frame the lookup path is still valid but the id
    // is what the tree records.
    let listener = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node_by_path("/root/Main/Listener")
        .expect("the fixture declares a Listener node");

    assert!(
        boot.main_loop()
            .unwrap()
            .tree()
            .get_node_by_path("/root/Main/Victim")
            .is_some(),
        "precondition: the Victim node must exist before the frame that deletes \
         it, or the deletion assertion below would pass vacuously"
    );

    boot.main_loop_mut().unwrap().step(DT);

    // -- the script got through all three calls -----------------------------
    assert_eq!(
        script_var(&boot, "ran"),
        Variant::Bool(true),
        "`_process` must reach its last line. False means one of \
         queue_delete / notify_group_flags / set_current_scene raised \
         UndefinedFunction and aborted the script — the pat-mgixk.59 remaining \
         scope exactly."
    );

    // -- queue_delete actually freed the node -------------------------------
    //
    // Asserted on the tree, not on a return value: Godot's queue_delete returns
    // void, so the only evidence it did anything is the node being gone after
    // the end-of-frame reap.
    assert!(
        boot.main_loop()
            .unwrap()
            .tree()
            .get_node_by_path("/root/Main/Victim")
            .is_none(),
        "`get_tree().queue_delete(node)` must free the node by the end of the \
         frame. Still present means the call was accepted and did nothing."
    );

    // -- notify_group_flags reached the group member ------------------------
    //
    // A code no engine path emits, so its presence can only come from the
    // script's broadcast.
    let got_notification = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node(listener)
        .map(|n| {
            n.notification_log()
                .iter()
                .any(|note| note.code() == CUSTOM_NOTIFICATION)
        })
        .unwrap_or(false);
    assert!(
        got_notification,
        "`notify_group_flags(0, \"watched\", {CUSTOM_NOTIFICATION})` must \
         deliver to the grouped node. Absent means the group was resolved but \
         nothing was delivered, or the group name was read from the wrong \
         argument index — the flags come first in this spelling and not in \
         `notify_group`."
    );

    // -- set_current_scene retargeted the tree ------------------------------
    assert_eq!(
        boot.main_loop().unwrap().tree().current_scene(),
        Some(listener),
        "`get_tree().set_current_scene(n)` must make `n` the current scene. The \
         booted main scene is `Main`, so an unchanged value here means the call \
         was a no-op."
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Reads the viewport's focus owner. Kept separate from the scene swap below
/// because that swap frees this very script's node.
const GD_FOCUS: &str = "\
extends Node2D
var focus_is_null = false
var ran = false
var done = false

func _process(delta):
    if self.done:
        return
    self.done = true
    self.focus_is_null = get_viewport().gui_get_focus_owner() == null
    self.ran = true
";

/// Hands an existing node over as the current scene.
///
/// ⚠️ Sets no flag after the call and the test reads none: `change_scene_to_node`
/// frees the OLD current scene, which is the node this script is attached to.
/// Anything it stored would be unreachable a line later.
const GD_SWAP: &str = "\
extends Node2D
var done = false

func _process(delta):
    if self.done:
        return
    self.done = true
    get_tree().change_scene_to_node(get_node(\"Listener\"))
";

/// `gui_get_focus_owner` answers null rather than raising.
///
/// ⚠️ Asserted as "is null", not "is some Control". Nothing in this fixture
/// takes focus, so null is the CORRECT answer — and the bug being fixed is that
/// the method did not exist, which aborts the script rather than returning
/// anything. Demanding a focused node would need a Control and a focus grab,
/// and would then be testing focus rather than the binding.
#[test]
fn viewport_focus_owner_from_script() {
    let _guard = BOOT_LOCK.lock().unwrap();
    let dir = unique_project_dir("focus");
    write_project(&dir, GD_FOCUS);
    let mut boot = boot(&dir);

    boot.main_loop_mut().unwrap().step(DT);

    assert_eq!(
        script_var(&boot, "ran"),
        Variant::Bool(true),
        "`_process` must reach its last line; false means \
         `get_viewport().gui_get_focus_owner()` raised UndefinedFunction"
    );
    assert_eq!(
        script_var(&boot, "focus_is_null"),
        Variant::Bool(true),
        "`gui_get_focus_owner()` must answer null when nothing holds focus"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `change_scene_to_node` installs the node AND disposes of the old scene.
///
/// ⚠️ Every assertion is on tree state, never on a script variable. The call
/// frees the node whose script made it (`free_current_scene` runs first), so a
/// flag set afterwards could not be read — which is exactly how the first
/// version of this test failed.
#[test]
fn change_scene_to_node_from_script() {
    let _guard = BOOT_LOCK.lock().unwrap();
    let dir = unique_project_dir("swap");
    write_project(&dir, GD_SWAP);
    let mut boot = boot(&dir);

    let listener = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node_by_path("/root/Main/Listener")
        .expect("the fixture declares a Listener node");
    let root = boot.main_loop().unwrap().tree().root_id();

    boot.main_loop_mut().unwrap().step(DT);

    // -- the node became the current scene ----------------------------------
    assert_eq!(
        boot.main_loop().unwrap().tree().current_scene(),
        Some(listener),
        "`change_scene_to_node(n)` must make `n` the current scene. Unchanged \
         means the call was accepted and did nothing — which is what HEAD does, \
         because the method does not exist there and the script aborts."
    );

    // -- and was re-rooted --------------------------------------------------
    assert_eq!(
        boot.main_loop()
            .unwrap()
            .tree()
            .get_node(listener)
            .and_then(|n| n.parent()),
        Some(root),
        "the handed-over node must be re-rooted under /root; still parented to \
         Main means it was recorded as current without being handed over"
    );

    // -- the old scene is gone ----------------------------------------------
    //
    // 🔑 This is what separates a real scene change from merely storing a
    // pointer: Godot frees the outgoing scene. It also proves the detach in the
    // binding worked — the moved node survived a free that took its old parent.
    assert!(
        boot.main_loop()
            .unwrap()
            .tree()
            .get_node_by_path("/root/Main")
            .is_none(),
        "the previous current scene must be freed by change_scene_to_node"
    );
    assert!(
        boot.main_loop()
            .unwrap()
            .tree()
            .get_node(listener)
            .is_some(),
        "the handed-over node must SURVIVE the old scene being freed. Gone \
         means it was still attached to Main when Main was disposed of, i.e. \
         the binding did not detach it first."
    );

    let _ = std::fs::remove_dir_all(&dir);
}
