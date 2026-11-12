//! pat-s3rrv.34 acceptance: an exported `.pck` serves **every** `res://` read.
//!
//! Godot installs `FileAccessPack`/`DirAccessPack` as the process-wide backends
//! (`core/io/file_access_pack.cpp`), so after a pack is mounted every read —
//! scene loading, `load()`/`preload()`, `FileAccess.open`, `DirAccess` listing —
//! comes out of the archive. Patina had the pack VFS but almost nothing called
//! it: `FileAccess::open` went straight to the OS (its own doc conceded it
//! "cannot serve a pack"), `DirAccess::open` required a real directory to exist,
//! and `preload()` / `load()` used raw `std::fs::read_to_string`. Only the
//! static `read_file_bytes` was pack-aware.
//!
//! `runner_boots_godot_pck` already proves the *boot* half — project.godot and
//! the main scene parse out of a pack. This test starts where that one stops:
//! the source tree is **deleted** after packing, and then the reads a running
//! game actually performs are driven against pack-only content.

use std::path::PathBuf;

use gdcore::file_access::{DirAccess, FileAccess, FileMode};
use gdresource::pck::PckPacker;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;
use patina_runner::project_boot::boot_project_from_pck;

/// Mounted packs and `res://` roots are process-global, so the tests that touch
/// them serialize. Mirrors `runner_boots_godot_pck`.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn temp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("patina_pckread_{tag}_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

const PROJECT_GODOT: &str = r#"config_version=5

[application]
config/name="Packed Reads"
run/main_scene="res://main.tscn"
"#;

const MAIN_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]

[node name="HUD" type="Node2D" parent="."]
"#;

const SUB_TSCN: &str = r#"[gd_scene format=3]

[node name="Bullet" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
"#;

const NESTED_TSCN: &str = r#"[gd_scene format=3]

[node name="Nested" type="Node2D"]
"#;

const DATA_TXT: &str = "packed-bytes-42";

/// Writes the fixture project to disk, packs it, deletes the source tree, and
/// returns the path to the `.pck`.
///
/// The pack is written to a **different** directory than the source so that
/// `res://`'s fallback root (the pack's own directory) holds no loose project
/// files. Without that separation a read could be served by the leftover tree
/// and the test would pass without the pack doing anything.
fn pack_and_delete_source(tag: &str) -> PathBuf {
    let source = temp_dir(&format!("{tag}_src"));
    let out = temp_dir(&format!("{tag}_out"));

    std::fs::create_dir_all(source.join("scenes")).expect("nested dir");
    for (rel, body) in [
        ("project.godot", PROJECT_GODOT),
        ("main.tscn", MAIN_TSCN),
        ("sub.tscn", SUB_TSCN),
        ("data.txt", DATA_TXT),
        ("scenes/nested.tscn", NESTED_TSCN),
    ] {
        std::fs::write(source.join(rel), body).expect("fixture file is writable");
    }

    // Packed from the real files on disk, not from the string constants — this
    // is the path an export takes.
    let mut packer = PckPacker::new();
    for rel in [
        "project.godot",
        "main.tscn",
        "sub.tscn",
        "data.txt",
        "scenes/nested.tscn",
    ] {
        let bytes = std::fs::read(source.join(rel)).expect("fixture file is readable");
        packer.add_file(format!("res://{rel}"), bytes);
    }
    let pck_path = out.join("game.pck");
    std::fs::write(&pck_path, packer.pack().expect("pack builds")).expect("write pck");

    // "with no loose project files present" — the whole point of the bead.
    std::fs::remove_dir_all(&source).expect("source tree is removable");
    assert!(
        !source.exists(),
        "the source tree must be gone, or every assertion below could be \
         satisfied by disk instead of the pack"
    );

    pck_path
}

/// Runs `check()` on a fresh instance of `src` through real VM dispatch, the
/// way a script in the running game would call it.
fn script_check(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}"))
}

/// **The acceptance test.**
#[test]
fn pck_only_project_boots_and_serves_all_reads() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gdcore::file_access::clear_mounted_packs();

    let pck_path = pack_and_delete_source("all_reads");
    let mut booted = boot_project_from_pck(&pck_path, None).expect("boot from .pck");

    // ---- 1. The main scene came out of the pack -----------------------------
    assert_eq!(
        booted.settings.project_name(),
        "Packed Reads",
        "project.godot must be parsed out of the pack"
    );
    let main = booted
        .main_scene_root
        .expect("run/main_scene must instance from the pack");
    assert_eq!(
        booted.tree.get_node(main).expect("main node exists").name(),
        "Main"
    );
    let hud = booted
        .tree
        .find_children(main, "HUD", None, true)
        .first()
        .copied()
        .expect("the main scene's child must instance too, not just its root");
    assert_eq!(booted.tree.get_node(hud).unwrap().name(), "HUD");

    // ---- 2. Rust-side FileAccess::open on a packed path ---------------------
    // The handle API the script binding sits on. Before this bead `open`
    // resolved to an OS path and failed here with "No such file or directory".
    let mut fa = FileAccess::open("res://data.txt", FileMode::Read)
        .expect("FileAccess::open must serve a packed file");
    assert_eq!(
        fa.get_as_text().expect("packed file reads as text"),
        DATA_TXT,
        "the bytes must come from the pack — the source tree is deleted"
    );

    // Writing into a pack is refused rather than silently buffered in memory,
    // matching Godot's ERR_FILE_CANT_WRITE for a packed path.
    assert!(
        FileAccess::open("res://data.txt", FileMode::Write).is_err(),
        "a pack is read-only; a write that appeared to succeed would lose data"
    );

    // ---- 3. Script-side FileAccess.open(...).get_as_text() ------------------
    let text = script_check(
        "\
extends Node

func check():
    var f = FileAccess.open(\"res://data.txt\", FileAccess.READ)
    return f.get_as_text()
",
    );
    assert_eq!(
        text,
        Variant::String(DATA_TXT.to_string()),
        "a script's FileAccess.open must read packed bytes; Nil or an error \
         here means the script surface still bypasses the pack"
    );

    // ---- 4. DirAccess listing of pack-only content --------------------------
    // `res://` has no directory on disk at all here, so this exercises both the
    // relaxed `open` and the merged listing.
    let dir = DirAccess::open("res://").expect("res:// must open from a pack alone");
    let files = dir.get_files();
    for expected in ["project.godot", "main.tscn", "sub.tscn", "data.txt"] {
        assert!(
            files.iter().any(|f| f == expected),
            "DirAccess.get_files() must list packed entry {expected}; got {files:?}"
        );
    }
    assert!(
        !files.iter().any(|f| f.contains('/')),
        "get_files() returns bare names, never paths; got {files:?}"
    );
    assert!(
        files.iter().all(|f| f != "nested.tscn"),
        "a file nested under res://scenes must NOT surface as a sibling of the \
         root — a pack stores a flat path list, so this is the failure mode if \
         subdirectories are not derived; got {files:?}"
    );
    assert_eq!(
        dir.get_directories(),
        vec!["scenes".to_string()],
        "subdirectories must be derived from packed paths"
    );

    // The script-visible form named by the acceptance criteria.
    let listed = script_check(
        "\
extends Node

func check():
    var d = DirAccess.open(\"res://\")
    return d.get_files()
",
    );
    match listed {
        Variant::PackedStringArray(names) => assert!(
            names.iter().any(|n| n == "data.txt"),
            "DirAccess.open(\"res://\").get_files() must list packed entries \
             from a script; got {names:?}"
        ),
        other => panic!("get_files() must return a PackedStringArray, got {other:?}"),
    }

    // ---- 5. preload("res://sub.tscn").instantiate() -------------------------
    // Driven on the booted tree, so this is the pack serving a *runtime* scene
    // load rather than the boot-time one already covered above.
    let script = GDScriptNodeInstance::from_source(
        "extends Node2D\n\
         var spawned = null\n\
         func _ready():\n\
         \tvar scene = preload(\"res://sub.tscn\")\n\
         \tself.spawned = scene.instantiate()\n",
        main,
    )
    .expect("script parses");
    booted.tree.attach_script(main, Box::new(script));
    booted.tree.process_script_ready(main);

    let spawned = booted
        .tree
        .get_script(main)
        .expect("Main has a script")
        .get_property("spawned")
        .unwrap_or(Variant::Nil);
    let bullet = match spawned {
        Variant::ObjectId(o) => gdscene::node::NodeId::from_object_id(o),
        Variant::Nil => panic!(
            "preload(\"res://sub.tscn\").instantiate() returned null — the \
             scene was not read from the pack (this is what the raw \
             std::fs::read_to_string did once the source tree was gone)"
        ),
        other => panic!("instantiate() must return an object id, got {other:?}"),
    };
    let node = booted
        .tree
        .get_node(bullet)
        .expect("spawned node is in tree");
    assert_eq!(
        node.name(),
        "Bullet",
        "the spawned root keeps its .tscn name"
    );
    let children = node.children().to_vec();
    assert_eq!(
        children.len(),
        1,
        "instantiate() must build the packed scene's children, not just its root"
    );
    assert_eq!(
        booted.tree.get_node(children[0]).unwrap().name(),
        "Sprite",
        "the child comes from the packed .tscn"
    );

    gdcore::file_access::clear_mounted_packs();
}

/// A directory that exists in neither the pack nor on disk still fails.
///
/// The guard against "fixed it by making DirAccess never fail": `open` was
/// relaxed to accept pack-only directories, and a relaxation that accepts
/// *everything* would make the listing assertions above meaningless.
#[test]
fn a_directory_in_neither_pack_nor_disk_still_fails() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gdcore::file_access::clear_mounted_packs();

    let pck_path = pack_and_delete_source("negative");
    let _booted = boot_project_from_pck(&pck_path, None).expect("boot from .pck");

    assert!(
        DirAccess::open("res://does_not_exist").is_err(),
        "a directory absent from both the pack and disk must still be an error"
    );
    assert!(
        !DirAccess::dir_exists("res://does_not_exist"),
        "dir_exists must not report a directory no mount supplies"
    );
    assert!(
        DirAccess::dir_exists("res://scenes"),
        "…while a directory that exists only inside the pack does exist"
    );

    gdcore::file_access::clear_mounted_packs();
}
