//! pat-mgixk.107 — `String` byte buffers, hashing, validation and the six statics.
//!
//! The String arm of `call_method_on` ended in
//! `UndefinedFunction("String.{method}")` with no fallback, and nothing
//! dispatched a static on the `String` type ident at all — so every call below
//! RAISED rather than returning a wrong value.
//!
//! Results are recorded onto a `Flag` node's META and read from Rust, which is
//! the pattern the green `gdscript_camera3d_projection_methods` and
//! `variant_stringify_parity_test` use. `done` is written LAST so a failure
//! distinguishes "a method answered wrongly" from "the frame died at the first
//! undispatched call" — and every one of these raised
//! `UndefinedFunction("String.<name>")` before this change, so an early abort is
//! the expected pre-change shape.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `EngineBootstrap` installs process-global `res://` roots and ClassDB
/// registrations, so concurrent boots would read each other's state.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk107_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create project dir");
    dir
}

fn write_project(dir: &Path, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .expect("write project.godot");
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_m\"]\n\n\
         [node name=\"Main\" type=\"Node2D\"]\n\
         script = ExtResource(\"1_m\")\n\n\
         [node name=\"Flag\" type=\"Node2D\" parent=\".\"]\n",
    )
    .expect("write main.tscn");
    std::fs::write(dir.join("main.gd"), gd).expect("write main.gd");
}

fn boot(dir: &Path) -> EngineBootstrap {
    let mut boot = EngineBootstrap::new(
        BootConfig::with_scene(dir.join("main.tscn")).project_dir(dir.to_path_buf()),
    );
    boot.run_all().expect("the runner must boot the scene");
    boot
}

fn flag_of(boot: &EngineBootstrap) -> gdscene::node::NodeId {
    let tree = boot.main_loop().expect("MainLoop").tree();
    let root = tree.root_id();
    let main = tree
        .find_child(root, "Main", true)
        .expect("the main scene instantiated");
    tree.find_child(main, "Flag", true)
        .expect("the scene has a Flag node")
}

fn meta(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Variant {
    boot.main_loop()
        .expect("MainLoop")
        .tree()
        .get_node(flag)
        .expect("the Flag node is still in the tree")
        .get_meta(key)
}

const MAIN_GD: &str = "\
extends Node2D

func _ready():
    var f = get_node(\"Flag\")
    f.set_meta(\"done\", 0)
    f.set_meta(\"utf8\", \"h\u{e9}\".to_utf8_buffer())
    f.set_meta(\"md5\", \"abc\".md5_text())
    f.set_meta(\"sha1\", \"abc\".sha1_text())
    f.set_meta(\"ident_ok\", \"_a1\".is_valid_identifier())
    f.set_meta(\"ident_bad\", \"1a\".is_valid_identifier())
    f.set_meta(\"hex\", \"0x1F\".is_valid_hex_number(true))
    f.set_meta(\"node_name\", \"a:b\".validate_node_name())
    f.set_meta(\"num\", String.num(3.14159, 2))
    f.set_meta(\"chr\", String.chr(65))
    f.set_meta(\"size\", String.humanize_size(1536))
    f.set_meta(\"done\", 1)
";

/// **The acceptance test.**
#[test]
fn string_api_buffers_validation_test() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("buffers");
    write_project(&dir, MAIN_GD);
    let boot = boot(&dir);
    let flag = flag_of(&boot);

    assert_eq!(
        meta(&boot, flag, "done"),
        Variant::Int(1),
        "`_ready` must reach its last line; `Nil` or 0 means one of these calls \
         raised `UndefinedFunction(\"String.<name>\")` and aborted the frame"
    );

    // 🔑 UTF-8 of "hé" is three bytes: `h` plus the two-byte `é`. A length of 2
    // would mean the string was treated as Latin-1 somewhere.
    assert_eq!(
        meta(&boot, flag, "utf8"),
        Variant::PackedByteArray(vec![104, 195, 169]),
        "to_utf8_buffer must encode, not truncate"
    );
    assert_eq!(
        meta(&boot, flag, "md5"),
        Variant::String("900150983cd24fb0d6963f7d28e17f72".to_string()),
        "the published MD5 of \"abc\""
    );
    assert_eq!(
        meta(&boot, flag, "sha1"),
        Variant::String("a9993e364706816aba3e25717850c26c9cd0d89d".to_string()),
        "the published SHA-1 of \"abc\""
    );
    // Both directions: a validator that answers `true` for everything would
    // pass the first of these and fail the second.
    assert_eq!(
        meta(&boot, flag, "ident_ok"),
        Variant::Bool(true),
        "`_a1` is a valid identifier"
    );
    assert_eq!(
        meta(&boot, flag, "ident_bad"),
        Variant::Bool(false),
        "`1a` is NOT: an identifier cannot start with a digit"
    );
    assert_eq!(
        meta(&boot, flag, "hex"),
        Variant::Bool(true),
        "`0x1F` is a valid hex number when the prefix is required"
    );
    assert_eq!(
        meta(&boot, flag, "node_name"),
        Variant::String("a_b".to_string()),
        "`:` is reserved in a node name and becomes `_`"
    );
    assert_eq!(
        meta(&boot, flag, "num"),
        Variant::String("3.14".to_string()),
        "String.num(x, 2) formats to two decimals"
    );
    assert_eq!(
        meta(&boot, flag, "chr"),
        Variant::String("A".to_string()),
        "String.chr(65) is `A`"
    );
    // 🔑 `1.5` not `1.50`: humanize_size runs through Godot's `String::num`,
    // whose trailing-zero trim is the whole reason this reads naturally.
    assert_eq!(
        meta(&boot, flag, "size"),
        Variant::String("1.5 KiB".to_string()),
        "1536 bytes is 1.5 KiB"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
