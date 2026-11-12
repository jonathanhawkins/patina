//! pat-mgixk.55 acceptance: a project whose paths are `uid://…` boots.
//!
//! # What was wrong
//!
//! `gdresource::uid` has had the registry and both parsers for a while
//! (`load_from_cache_bytes`, `register_uid_sidecar`, `load_uid_sidecar_file`),
//! and the `ResourceUID` script singleton reads them — but **no boot code ever
//! populated the registry**, and no path consumer resolved a uid. `grep -rn
//! 'load_from_cache_bytes|load_uid_sidecar_file|uid_cache.bin|global_registry()'`
//! matched only `uid.rs` itself and one test in `unified.rs`.
//!
//! So `project_boot` passed `run/main_scene` straight to
//! `FileAccess::read_file_string`, and `resolve_path` understands only `res://`
//! and `user://` — a `uid://b2x…` became a bare relative filesystem path and the
//! read failed. The Godot 4.6 editor writes `run/main_scene` as a uid
//! (`editor/editor_node.cpp:2627`), so **every project saved by a current
//! editor booted with no main scene at all.**
//!
//! # The path driven
//!
//! ```text
//! project.godot (run/main_scene = uid://…, autoload = *uid://…)
//!   -> boot_project -> load_uid_registry (res://.godot/uid_cache.bin)
//!     -> instantiate_autoload / load_scene_into_tree  (resolve_uid_path)
//!       -> MainLoop::step -> _ready -> load("uid://…") / ResourceUID.get_id_path
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use gdscene::scripting::GDScriptNodeInstance;
use gdscene::MainLoop;
use gdvariant::Variant;
use patina_runner::project_boot::boot_project;

/// `boot_project` mutates process-global state (the `res://` path roots, the
/// settings store, and — new in this bead — the UID registry), so these tests
/// must not interleave. `cargo nextest` gives each test its own process and
/// would not need this; `cargo test` shares one, and does.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_boot() -> std::sync::MutexGuard<'static, ()> {
    BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Godot-sized UIDs for the two scenes. Arbitrary, but asserted to round-trip
/// through `id_to_text`/`text_to_id` before anything depends on them — see
/// `fixture_uids_round_trip`.
const MAIN_UID_ID: i64 = 7777777777777777777;
const HELPER_UID_ID: i64 = 1234567890123456789;

/// The main scene's root is a distinctive name so "the main scene instanced" is
/// distinguishable from "some node exists".
const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="MainRoot" type="Node2D"]
script = ExtResource("1_main")
"#;

const HELPER_TSCN: &str = r#"[gd_scene format=3]

[node name="HelperRoot" type="Node2D"]
"#;

/// Reads back, from inside a running script, the two things the acceptance names
/// about the script-visible surface.
///
/// 🔑 The uid text is INTERPOLATED, not written by hand. `UID_CHARS` has no
/// `'z'` while the decoder maps `'z'` and `'0'` to the same value, so a
/// hand-written uid can decode to an id the encoder would never produce. Taking
/// the text from `id_to_text` makes the fixture self-consistent by construction.
fn main_gd(main_uid: &str) -> String {
    format!(
        "extends Node2D\n\
         var loaded_class = \"\"\n\
         var uid_path = \"\"\n\
         func _ready():\n\
         \tloaded_class = load(\"{main_uid}\").get_class()\n\
         \tuid_path = ResourceUID.get_id_path(ResourceUID.text_to_id(\"{main_uid}\"))\n"
    )
}

/// `uid_cache.bin` in Godot's `ResourceUID::load_from_cache` format:
/// `entry_count: u32 LE`, then per entry `id: u64 LE`, `len: u32 LE`, `len`
/// UTF-8 bytes with no terminator.
fn uid_cache_bytes(entries: &[(i64, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (id, path) in entries {
        out.extend_from_slice(&(*id as u64).to_le_bytes());
        out.extend_from_slice(&(path.len() as u32).to_le_bytes());
        out.extend_from_slice(path.as_bytes());
    }
    out
}

/// Writes the fixture project. `cache` decides which mappings the cache carries,
/// so a test can author a project whose uid is deliberately unresolvable.
fn write_project(dir: &Path, cache: &[(i64, &str)]) {
    let main_uid = gdresource::uid::id_to_text(MAIN_UID_ID);
    let helper_uid = gdresource::uid::id_to_text(HELPER_UID_ID);

    // `*` marks a singleton autoload, exactly as Godot writes it; the parser
    // strips it, leaving the uid.
    std::fs::write(
        dir.join("project.godot"),
        format!(
            "[application]\n\
             config/name=\"UidFixture\"\n\
             run/main_scene=\"{main_uid}\"\n\
             \n\
             [autoload]\n\
             Helper=\"*{helper_uid}\"\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("helper.tscn"), HELPER_TSCN).unwrap();
    std::fs::write(dir.join("main.gd"), main_gd(&main_uid)).unwrap();

    std::fs::create_dir_all(dir.join(".godot")).unwrap();
    std::fs::write(
        dir.join(".godot").join("uid_cache.bin"),
        uid_cache_bytes(cache),
    )
    .unwrap();
}

/// A unique project directory under the system temp dir.
///
/// ⚠️ Hand-rolled rather than `tempfile::tempdir()`: `patina-runner` has no
/// `[dev-dependencies]` at all, so `tempfile` is not available here. Copied from
/// `project_settings_script_defaults_test.rs`, which boots projects the same way.
fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("patina_mgixk55_{tag}_{}_{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn script_var(tree: &gdscene::SceneTree, node: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
        .unwrap_or(Variant::Nil)
}

/// A precondition, not a feature test: every assertion below reads a uid text
/// produced by `id_to_text` and expects the engine to decode it back to the id
/// the cache was keyed with. If the codec is not symmetric for these two ids,
/// the fixture is wrong and every other failure in this file would be a red
/// herring.
#[test]
fn fixture_uids_round_trip() {
    for id in [MAIN_UID_ID, HELPER_UID_ID] {
        let text = gdresource::uid::id_to_text(id);
        assert!(
            text.starts_with("uid://") && text != gdresource::uid::UID_INVALID_TEXT,
            "id {id} must encode to a real uid, got {text:?}",
        );
        assert_eq!(
            gdresource::uid::text_to_id(&text).raw(),
            id,
            "the fixture's uid text {text:?} must decode back to {id}",
        );
    }
}

/// **The acceptance.** A project with a `uid://` main scene, a `uid://` autoload
/// and a `.godot/uid_cache.bin` boots, and the uid surface is live inside a
/// script.
#[test]
fn uid_main_scene_boot_resolution() {
    let _guard = lock_boot();
    let dir = &unique_project_dir("boot");
    write_project(
        dir,
        &[
            (MAIN_UID_ID, "res://main.tscn"),
            (HELPER_UID_ID, "res://helper.tscn"),
        ],
    );

    let booted = boot_project(dir).expect("a project with a uid:// main scene must boot");

    // -- the main scene resolved and instanced -------------------------------
    let root = booted
        .main_scene_root
        .expect("main_scene_root must be Some: a uid:// run/main_scene resolves through the registry. None is the pre-bead behaviour, where the uid became a bare relative path and the read failed");
    assert_eq!(
        booted
            .tree
            .get_node(root)
            .expect("the instanced root is in the tree")
            .name(),
        "MainRoot",
        "the resolved scene must be res://main.tscn, identified by its root name \
         — 'a root exists' alone would also pass if some other scene loaded",
    );

    // -- the uid autoload is reachable at /root/<Name> ------------------------
    //
    // 🔑 This is the half that a fix confined to `load_scene_into_tree` would
    // MISS. `instantiate_autoload` tests `.tscn`/`.scn` on the raw string to
    // decide scene-vs-script, and a uid has no extension — so an unresolved uid
    // autoload becomes a bare `Node` carrying the uid as its script path. It
    // would still be reachable at `/root/Helper`, which is why the assertion is
    // on the INSTANCED CONTENT, not on the path existing.
    let helper = booted
        .tree
        .get_node_by_path("/root/Helper")
        .expect("the autoload must be reachable at /root/<Name>");
    assert_eq!(
        booted
            .tree
            .get_node(helper)
            .expect("the autoload node is in the tree")
            .class_name(),
        "Node2D",
        "the uid autoload must be INSTANCED as its scene (helper.tscn's root is a \
         Node2D). A plain 'Node' means the uid was classified as a script \
         autoload because the extension test ran on the unresolved uid — the \
         node exists either way, so the class is what tells them apart",
    );

    // -- the script-visible uid surface --------------------------------------
    let main_uid = gdresource::uid::id_to_text(MAIN_UID_ID);
    let mut tree = booted.tree;
    let source = std::fs::read_to_string(dir.join("main.gd")).unwrap();
    tree.attach_script(
        root,
        Box::new(GDScriptNodeInstance::from_source(&source, root).expect("the script parses")),
    );
    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(root);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(ml.tree(), root, "loaded_class"),
        Variant::String("PackedScene".into()),
        "load(\"{main_uid}\") must return the mapped scene. Every branch of the \
         load builtin dispatches on the path's EXTENSION and a uid has none, so \
         an unresolved uid falls through to the path-only placeholder and \
         reports the generic \"Resource\" class",
    );
    assert_eq!(
        script_var(ml.tree(), root, "uid_path"),
        Variant::String("res://main.tscn".into()),
        "ResourceUID.get_id_path(text_to_id(..)) must see the registry the BOOT \
         populated — the singleton and the boot share one global registry, so an \
         empty string here means nothing loaded the cache",
    );
}

/// A `*.uid` sidecar is registered against its source asset's **`res://`** path.
///
/// 🔑 Keyed the way it will be queried. `uid.rs`'s own `load_uid_sidecar_file`
/// strips `.uid` from whatever path it is handed, so feeding it a filesystem
/// path registers `/tmp/xyz/thing.gd` — a key no consumer ever looks up, because
/// every consumer asks with `res://thing.gd`. A sidecar scan that forgot the
/// conversion would populate the registry and still resolve nothing.
#[test]
fn uid_sidecars_register_under_res_paths() {
    let _guard = lock_boot();
    let dir = &unique_project_dir("sidecar");
    write_project(
        dir,
        &[
            (MAIN_UID_ID, "res://main.tscn"),
            (HELPER_UID_ID, "res://helper.tscn"),
        ],
    );

    // A sidecar for a nested asset, so the test also covers the relative-path
    // join rather than only a file at the project root.
    let sidecar_id: i64 = 999999999999999999;
    let sidecar_uid = gdresource::uid::id_to_text(sidecar_id);
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join("scripts").join("thing.gd"), "extends Node\n").unwrap();
    std::fs::write(
        dir.join("scripts").join("thing.gd.uid"),
        format!("{sidecar_uid}\n"),
    )
    .unwrap();

    boot_project(dir).expect("the fixture project boots");

    assert_eq!(
        gdresource::uid::get_id_path(gdresource::uid::text_to_id(&sidecar_uid)).as_deref(),
        Some("res://scripts/thing.gd"),
        "the sidecar must register its source under a res:// path. A filesystem \
         path here means the scan registered a key nothing queries",
    );
}

/// An unresolvable uid fails with an error naming the uid.
///
/// 🔑 `uid::ensure_path` returns an EMPTY string for an unregistered uid (it
/// wraps `get_id_path`, which Godot fails to `String()`). Passing that straight
/// through would produce `failed to read scene ''` — an error naming nothing,
/// which is materially worse for a user than the pre-bead behaviour. This pins
/// the fallback that keeps the original text in the message.
#[test]
fn an_unresolvable_uid_main_scene_reports_the_uid() {
    let _guard = lock_boot();
    let dir = &unique_project_dir("unresolvable");
    // The cache maps the HELPER uid only, so the main scene's uid is unknown.
    write_project(dir, &[(HELPER_UID_ID, "res://helper.tscn")]);

    let main_uid = gdresource::uid::id_to_text(MAIN_UID_ID);
    // ⚠️ A `match`, not `expect_err`: that requires `T: Debug` and
    // `BootedProject` does not derive it (it holds a `SceneTree`). The sibling
    // bead pat-mgixk.56 hit exactly this as E0277 on the same call.
    let err = match boot_project(dir) {
        Ok(_) => panic!("an unresolvable main-scene uid must fail the boot"),
        Err(e) => e,
    };
    assert!(
        err.contains(&main_uid),
        "the error must name the uid that could not be resolved, so the user can \
         see which mapping is missing; got {err:?}",
    );
}
