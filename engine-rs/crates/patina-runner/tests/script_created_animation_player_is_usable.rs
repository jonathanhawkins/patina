//! pat-mgixk.79 (remaining scope) — an `AnimationPlayer` built in code is a
//! working player.
//!
//! In Godot the node **is** the mixer, so a player always carries its own state.
//! In Patina the state is a side table (`SceneTree::animation_players`) that
//! only the `.tscn` loader ever populated — `animation_tscn.rs:562` was
//! `attach_animation_player`'s sole non-test caller. So
//! `AnimationPlayer.new()` + `add_child` produced a node of the right class with
//! no player behind it, and `animation_player_method` declined every call.
//!
//! ⚠️ **Declining is not harmless.** The dispatcher falls through to
//! `UndefinedFunction`, which aborts the calling frame. Measured against HEAD
//! with the prebuilt runner: a script that printed `get_class()` and then called
//! `get_animation_list()` printed the class and **nothing after it** — the rest
//! of `_physics_process` never ran. The bead's note called this "every method
//! declines to Nil"; the observable behaviour is worse than that.
//!
//! # What is and is not asserted
//!
//! 🔑 The player's *animation* behaviour is NOT asserted here, because a script
//! still has no way to put an `Animation` into a code-built player — that is the
//! Animation-resource API, a separate bead. What is asserted is the thing this
//! change is responsible for: the player exists, and its method surface answers
//! instead of killing the frame.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` mutates the process-global `res://` roots, so serialize under
/// the single-process test runner. `nextest` isolates per process.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const DT: f64 = 1.0 / 60.0;

/// Takes the boot lock, tolerating a previous test's panic, so one failing test
/// does not turn every later one into an opaque `PoisonError`.
fn boot_lock() -> std::sync::MutexGuard<'static, ()> {
    BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk79_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// ⚠️ The scene authors **no** `AnimationPlayer`. That is the point: the only
/// player in this test is the one the script constructs, so a pass cannot come
/// from the `.tscn` loader having attached one.
fn write_project(dir: &Path, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_m\"]\n\n\
         [node name=\"Main\" type=\"Node2D\"]\n\
         script = ExtResource(\"1_m\")\n\n\
         [node name=\"Flag\" type=\"Node2D\" parent=\".\"]\n",
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

/// ⚠️ `reached_end` is written **after** the player calls, and it is the
/// assertion that distinguishes this bug's real symptom from the reported one.
/// A method that merely returned `Nil` would leave the flag set; a method that
/// raises leaves it unset, because the raise takes the rest of the function with
/// it.
///
/// ⚠️ Recorded on a `Flag` NODE rather than in a script variable. Node metadata
/// lives in the tree, so it is readable from Rust without depending on how
/// script instance state is written back.
///
/// 🔑 **The call ORDER is load bearing, because there are two dispatch tables.**
/// `get_current_animation_length` and `get_assigned_animation` live in
/// `scripting.rs`'s `animation_player_method`, which is where the player is
/// materialised. `has_animation` lives in the INTERPRETER, behind a guard
/// (`animation_player_is_playing(id).is_some()`) that only matches when the
/// struct already exists — and that guard is there on purpose, to stop the arm
/// shadowing a script's own `has_animation()`. So `has_animation` is called
/// LAST: it is reachable only once an earlier call has materialised the player.
/// Calling it first on a brand-new player still aborts the frame, which is
/// reported as remaining scope rather than papered over here.
const GD: &str = "\
extends Node2D
var phase = 0

func _physics_process(delta):
    if self.phase != 0:
        return
    self.phase = 1
    var f = get_node(\"Flag\")
    f.set_meta(\"reached_end\", 0)
    var ap = AnimationPlayer.new()
    add_child(ap)
    var got = get_node(\"AnimationPlayer\")
    f.set_meta(\"cls\", got.get_class())
    f.set_meta(\"len\", got.get_current_animation_length())
    f.set_meta(\"assigned\", got.get_assigned_animation())
    f.set_meta(\"has_walk\", got.has_animation(\"walk\"))
    f.set_meta(\"reached_end\", 1)
";

fn meta(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Variant {
    boot.main_loop()
        .unwrap()
        .tree()
        .get_node(flag)
        .expect("the Flag node is still in the tree")
        .get_meta(key)
}

#[test]
fn script_created_animation_player_is_usable() {
    let _guard = boot_lock();
    let dir = unique_project_dir("newplayer");
    write_project(&dir, GD);
    let mut boot = boot(&dir);

    let flag = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node_by_path("/root/Main/Flag")
        .expect("the fixture declares a Flag node");

    for _ in 0..3 {
        boot.main_loop_mut().unwrap().step(DT);
    }

    // -- the frame survived the calls ----------------------------------------
    assert_eq!(
        meta(&boot, flag, "reached_end"),
        Variant::Int(1),
        "`_physics_process` must reach its last line. 0 means a player method \
         raised and aborted the frame — the behaviour at HEAD, where a \
         code-built AnimationPlayer has no player struct and every method \
         declines into `UndefinedFunction`"
    );

    // -- the node really is a code-built AnimationPlayer ---------------------
    //
    // Guards against the test passing for the boring reason that `.new()`
    // produced something else, or nothing.
    assert_eq!(
        meta(&boot, flag, "cls"),
        Variant::String("AnimationPlayer".to_string()),
        "the constructed node must be an AnimationPlayer"
    );

    // -- the method surface ANSWERED, rather than declining ------------------
    //
    // 🔑 A typed zero and `Nil` are the two outcomes this separates, and only
    // one of them means the method ran. Asserting truthiness, or "not 5.0",
    // would accept the decline.
    assert_eq!(
        meta(&boot, flag, "len"),
        Variant::Float(0.0),
        "`get_current_animation_length` must answer 0.0 for a player with \
         nothing assigned — Godot's own answer. `Nil` means the call was \
         declined rather than run"
    );
    assert_eq!(
        meta(&boot, flag, "assigned"),
        Variant::String(String::new()),
        "`get_assigned_animation` must answer the empty string, not decline"
    );

    // -- and the interpreter-side arms become reachable afterwards -----------
    //
    // `has_animation` is guarded on the struct already existing, so this is the
    // observable consequence of materialising: a call that could not run before
    // any other method had been used now answers.
    assert_eq!(
        meta(&boot, flag, "has_walk"),
        Variant::Bool(false),
        "`has_animation` must answer `false` once the player exists; `Nil` \
         means its interpreter guard still saw no player"
    );

    // -- and the engine-side state exists ------------------------------------
    //
    // The observable the fix is actually responsible for: the side table now
    // has a player for this node, so `process_animations` will drive it once a
    // clip can be added.
    let player_node = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node_by_path("/root/Main/AnimationPlayer")
        .expect("the script's add_child put the player in the tree");
    assert!(
        boot.main_loop()
            .unwrap()
            .tree()
            .get_animation_player(player_node)
            .is_some(),
        "a node of class AnimationPlayer must have a player struct attached \
         after its first method call; without one the loop skips it entirely \
         and no clip could ever play on a code-built player"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
