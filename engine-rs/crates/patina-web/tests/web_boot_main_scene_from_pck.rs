//! pat-mgixk.73 — the web runtime boots the exported project out of the
//! mounted `.pck`.
//!
//! Godot's web export mounts the pack and then runs the ordinary
//! `Main::setup` / `Main::start`, so the project's `application/run/main_scene`,
//! autoloads and `[input]` map are all live on the first frame
//! (`upstream/godot/platform/web/web_main.cpp`).
//!
//! Patina's `start_engine` built a bare `Node2D` tree and its doc comment
//! deferred loading the project to "the export pipeline's job". The export shell
//! (`gdplatform/src/web_export.rs`) mounts the pack and then calls
//! `startEngine(canvas)`, which never read it. So an exported game booted an
//! empty scene: no `_ready`, no `_process`, no input map — and silently, because
//! an empty tree renders the clear colour rather than failing.
//!
//! # What this drives
//!
//! ```text
//! PckPacker -> mount_pack_bytes          (what the export shell's mountPack does)
//!   -> WebEngine::boot_mounted_project   (the entry start_engine now calls)
//!     -> gdscene::res_boot::boot_from_res
//!       -> FileAccess res:// reads, served from the pack
//!     -> LifecycleManager::enter_tree, configure_from_project_settings, set_input_map
//!   -> WebEngine::frame(now_ms)          (the existing rAF loop)
//! ```
//!
//! ⚠️ Nothing here touches the filesystem. The pack is built in memory and
//! mounted in memory, which is the point: the native boot path reads
//! `project.godot` with `std::fs` and has no browser equivalent.

use std::sync::{Mutex, MutexGuard};

use gdcore::file_access::{clear_mounted_packs, set_path_roots, set_user_storage};
use gdplatform::input::{InputEvent, Key};
use gdresource::pck::PckPacker;
use gdvariant::Variant;
use patina_web::engine::WebEngine;
use patina_web::vfs::mount_pack_bytes;

/// Mounts and path roots are process-global, so these tests cannot overlap.
static VFS_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    VFS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The browser's starting state: nothing mounted, no roots, no `user://`.
fn reset() {
    set_user_storage(None);
    clear_mounted_packs();
    set_path_roots(None, None);
}

/// A project declaring a main scene, one autoload and one `[input]` action.
///
/// ⚠️ The tick rate is deliberately NOT the engine default, so
/// `configure_from_project_settings` is observable: if the boot skipped it the
/// value read back would be 60.
const PROJECT_GODOT: &str = r#"config_version=5

[application]

config/name="Packed Web Game"
run/main_scene="res://main.tscn"

[autoload]

Bootstrapper="*res://autoload.tscn"

[physics]

common/physics_ticks_per_second=30

[input]

fire={
"deadzone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":70,"physical_keycode":0,"key_label":0,"unicode":102,"location":0,"echo":false,"script":null)
]
}
"#;

const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")
"#;

/// Records what a running game can observe of its own boot.
///
/// ⚠️ `autoload_seen` is captured in `_ready`, not read later by the test.
/// Reading it afterwards would pass even if the autoload entered the tree
/// *after* the main scene, because by then it is present either way. Godot
/// guarantees it is there *when `_ready` runs*, and that ordering is the claim.
const MAIN_GD: &str = "extends Node2D\n\
     var ready_ran = false\n\
     var ticks = 0\n\
     var tps = 0\n\
     var autoload_seen = false\n\
     var fired = false\n\
     \n\
     func _ready():\n\
     \tself.ready_ran = true\n\
     \tself.tps = Engine.physics_ticks_per_second\n\
     \tself.autoload_seen = get_node_or_null(\"/root/Bootstrapper\") != null\n\
     \n\
     func _process(delta):\n\
     \tself.ticks = self.ticks + 1\n\
     \tif Input.is_action_pressed(\"fire\"):\n\
     \t\tself.fired = true\n\
";

const AUTOLOAD_TSCN: &str = r#"[gd_scene format=3]

[node name="AutoloadRoot" type="Node"]
"#;

/// The pack an export would ship: everything under `res://`, no filesystem.
fn packed_project() -> Vec<u8> {
    let mut packer = PckPacker::new();
    packer.add_file("res://project.godot", PROJECT_GODOT.as_bytes().to_vec());
    packer.add_file("res://main.tscn", MAIN_TSCN.as_bytes().to_vec());
    packer.add_file("res://main.gd", MAIN_GD.as_bytes().to_vec());
    packer.add_file("res://autoload.tscn", AUTOLOAD_TSCN.as_bytes().to_vec());
    packer.pack().expect("the fixture packs")
}

fn script_var(engine: &WebEngine, path: &str, var: &str) -> Variant {
    let tree = engine.tree();
    let id = tree
        .get_node_by_path(path)
        .unwrap_or_else(|| panic!("`{path}` must be in the booted tree"));
    tree.get_script(id)
        .unwrap_or_else(|| panic!("`{path}` must have a script to read `{var}`"))
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
        .unwrap_or(Variant::Nil)
}

/// **The acceptance test.**
#[test]
fn web_boot_main_scene_from_pck() {
    let _guard = lock();
    reset();
    mount_pack_bytes("game", packed_project(), 0).expect("the fixture is a valid pck");

    let mut engine = WebEngine::boot_mounted_project()
        .expect("the mounted pack must boot; this is what start_engine now calls");

    // -- (a) the main scene is in the tree ----------------------------------
    assert!(
        engine.tree().get_node_by_path("/root/Main").is_some(),
        "the project's run/main_scene must be instanced under /root. Absent is \
         the pat-mgixk.73 bug: start_engine built a bare Node2D and never read \
         the pack the export shell had mounted."
    );

    // -- localisation: property, then attach, then lifecycle ----------------
    //
    // Three links, asserted separately, because "`_ready` did not run" has three
    // very different causes and the script variable cannot tell them apart.
    let main_id = engine
        .tree()
        .get_node_by_path("/root/Main")
        .expect("checked above");
    assert_eq!(
        engine
            .tree()
            .get_node(main_id)
            .map(|n| n.get_property("_script_path")),
        Some(Variant::String("res://main.gd".to_string())),
        "the scene's `script = ExtResource(...)` must survive packing and \
         instancing as `_script_path`. Nil means `PackedScene::from_tscn` did \
         not resolve the Script ext_resource out of the packed `.tscn`, so there \
         was never anything for the attach pass to load."
    );
    assert!(
        engine.tree().get_script(main_id).is_some(),
        "`attach_subtree_scripts_from_res` must have turned `_script_path` into \
         a live script. None with the property present means the pass ran and \
         skipped it — it reads the source through `FileAccess`, so the likely \
         cause is `res://main.gd` not being readable out of the mounted pack."
    );

    // -- fixture check: did the script attach and run at all? ---------------
    //
    // ⚠️ FIRST, because every script-variable assertion below reads a value
    // whose declared default is the failing value. A `_ready` that never ran
    // leaves `autoload_seen` false, which is indistinguishable from an autoload
    // that entered the tree late — and the second is a much more alarming
    // diagnosis than the first. Ask the cheap question before the clever one.
    assert_eq!(
        script_var(&engine, "/root/Main", "ready_ran"),
        Variant::Bool(true),
        "the main scene's script must be attached and `_ready` must have run. \
         False means either `_script_path` never became a live script, or the \
         lifecycle never reached the main scene — not that anything below is \
         wrong."
    );

    // -- the autoload NODE exists, by its registered name -------------------
    //
    // Asserted before the script's observation of it, so "the autoload was
    // never instanced" cannot be reported as "the autoload entered too late".
    assert!(
        engine
            .tree()
            .get_node_by_path("/root/Bootstrapper")
            .is_some(),
        "the `[autoload]` entry must be instanced under /root and renamed to its \
         registered name (`Bootstrapper`), not left under the scene's own root \
         name (`AutoloadRoot`) — that name is what scripts reach it by"
    );

    // -- (b) and it was already there when the main scene's _ready ran ------
    //
    // Captured by the script during `_ready`, not read now: by now it is
    // present either way, so reading it here would pass even if the main scene
    // entered first. The ordering is the claim.
    assert_eq!(
        script_var(&engine, "/root/Main", "autoload_seen"),
        Variant::Bool(true),
        "the autoload must already be in the tree when the main scene's \
         `_ready` runs, as Godot guarantees. With the two assertions above \
         green, false here means specifically an ORDERING fault: the main scene \
         entered before the autoload, which breaks \
         `get_node(\"/root/Bootstrapper\")` in `_ready` — and only for projects \
         that use autoloads."
    );

    // The project's own tick rate, proving configure_from_project_settings ran.
    // The engine default is 60, so 30 can only have come from project.godot.
    assert_eq!(
        script_var(&engine, "/root/Main", "tps"),
        Variant::Int(30),
        "`Engine.physics_ticks_per_second` must be the project's 30, not the \
         engine default 60 — otherwise the boot assembled the tree but skipped \
         `configure_from_project_settings`"
    );

    // Synthetic rAF timestamps, as the browser's frame loop supplies.
    let mut now = 0.0;
    for _ in 0..8 {
        now += 1000.0 / 60.0;
        engine.frame(now);
    }
    let ticks = match script_var(&engine, "/root/Main", "ticks") {
        Variant::Int(n) => n,
        other => panic!("`ticks` should be an Int, got {other:?}"),
    };
    assert!(
        ticks > 1,
        "`_process` must run on later frames: got {ticks} after 8 rAF frames. \
         0 means the script is attached but never driven; 1 would mean only the \
         boot frame ran."
    );

    // -- (d) the packed [input] action resolves ------------------------------
    //
    // ⚠️ The action is declared ONLY inside the packed project.godot, so this
    // fails unless the boot installed the map. Asserted through
    // `Input.is_action_pressed` from script — what a game actually calls —
    // rather than by inspecting the InputMap, which would pass while the
    // runtime still had no map installed.
    assert_eq!(
        script_var(&engine, "/root/Main", "fired"),
        Variant::Bool(false),
        "precondition: the action must not read as pressed before the key is \
         pushed, or the assertion below proves nothing"
    );
    engine.push_input(InputEvent::Key {
        key: Key::F,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    now += 1000.0 / 60.0;
    engine.frame(now);
    assert_eq!(
        script_var(&engine, "/root/Main", "fired"),
        Variant::Bool(true),
        "`Input.is_action_pressed(\"fire\")` must resolve the action declared in \
         the packed project.godot after an F key event. False means the \
         `[input]` map was parsed and never installed."
    );

    reset();
}

/// With nothing mounted there is no project, and the boot says so rather than
/// returning an empty engine.
///
/// ⚠️ This is the error path `start_engine` discriminates on: the dev page
/// (`patina-web/web/index.html`) calls `startEngine` with no pack and must keep
/// working, so it falls back to a bare tree when `res://project.godot` is
/// absent. A project that EXISTS and fails to boot is reported instead — the
/// distinction is `FileAccess::file_exists`, not the error text.
#[test]
fn booting_with_nothing_mounted_reports_the_missing_project() {
    let _guard = lock();
    reset();

    let err = WebEngine::boot_mounted_project()
        .err()
        .expect("with no pack mounted there is no project to boot");
    assert!(
        err.contains("project.godot"),
        "the error must name the stage that failed so JS can surface it instead \
         of a blank canvas; got {err:?}"
    );

    reset();
}
