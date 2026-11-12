//! pat-s3rrv.7 acceptance: booting a project populates the whole ClassDB.
//!
//! Godot fills ClassDB in `register_core_types()` / `register_scene_types()`
//! before any MainLoop exists (upstream/godot/main/main.cpp), so every class
//! default, method and signal is queryable from frame 0.
//!
//! Patina registered classes only from the **editor**: the 681 registrations in
//! `gdobject::class_db` sat behind eight `register_*_classes()` entry points
//! whose sole production callers were in `gdeditor`. `patina-runner` called
//! none of them — `bootstrap.rs::init_servers` only read `class_count()` and
//! documented the omission as intentional ("the caller is responsible for
//! registering classes"). No caller ever was.
//!
//! The damage was silent and load-time. `packed_scene::node_with_class_defaults`
//! and `SceneTree::create_node` seed each node's defaults from
//! `class_db::get_property_list()`, which returned `[]`, so a `.tscn`-loaded
//! `Sprite2D` booted with **no** `visible`, `modulate` or `scale` at all — and
//! `ClassDB.class_exists("Sprite2D")` answered false in a running game.
//!
//! ⚠️ These tests deliberately never call `register_*_classes()` themselves.
//! That is the whole point: the sibling test `object_introspection_property_list`
//! has to hand-call `register_2d_classes()` before booting, and documents that
//! without it the property list is legitimately empty. If a test here registers
//! anything directly it proves nothing about boot.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// Booting mutates process-global `res://` roots, so serialize.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_s3rrv7_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// ⚠️ The `Sprite2D` carries **no property overrides** on purpose. Everything
/// asserted about it therefore has to come from the ClassDB defaults the loader
/// seeded — an override in the `.tscn` would make the test pass without ClassDB
/// being populated at all.
const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Sprite2D" type="Sprite2D" parent="."]
"#;

/// Reads the sibling sprite's reflection surface, which is what a save system
/// or an inspector actually calls.
const MAIN_GD: &str = "\
extends Node2D
var prop_count = 0
var sprite_registered = false
var node_registered = false
func _ready():
    var sprite = $Sprite2D
    var props = sprite.get_property_list()
    self.prop_count = props.size()
    self.sprite_registered = ClassDB.class_exists(\"Sprite2D\")
    self.node_registered = ClassDB.class_exists(\"AnimationPlayer\")
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"ClassDBBoot\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_GD).unwrap();
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

/// **The acceptance test.**
///
/// Boots through the real runner path (`EngineBootstrap::run_all`) and asserts
/// the four things a populated ClassDB buys a running game: seeded node
/// defaults, a non-empty reflection surface, class existence, and a database
/// that is actually full rather than just non-empty.
#[test]
fn classdb_populated_at_runner_boot() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("boot");
    write_project(&dir);

    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");

    let tree = boot.main_loop().expect("main loop built").tree();
    let main_id = tree
        .get_node_by_path("/root/Main")
        .expect("main scene instanced");
    let sprite_id = tree
        .get_node_or_null(main_id, "Sprite2D")
        .expect("Sprite2D instanced");

    // -- the loader seeded ClassDB defaults onto the node ---------------------
    // This is the assertion that cannot pass vacuously: these values are read
    // off the node itself, and the `.tscn` sets neither. They exist only if
    // ClassDB was populated *before* the scene was built.
    let sprite = tree.get_node(sprite_id).expect("sprite node");
    assert_eq!(
        sprite.get_property("scale"),
        Variant::Vector2(gdcore::math::Vector2::ONE),
        "an un-overridden Sprite2D must carry Node2D's `scale` default (1,1) \
         straight out of the loader — Nil means ClassDB was empty at load time"
    );
    assert_eq!(
        sprite.get_property("visible"),
        Variant::Bool(true),
        "…and CanvasItem's `visible` default, proving the loader walked the \
         full Sprite2D -> Node2D -> CanvasItem chain"
    );

    // -- a script on that node sees a real reflection surface -----------------
    let prop_count = script_var(tree, main_id, "prop_count").expect("prop_count read");
    match prop_count {
        Variant::Int(n) => assert!(
            n > 0,
            "get_property_list() on a booted Sprite2D must be non-empty, got {n}"
        ),
        other => panic!("prop_count should be an int, got {other:?}"),
    }
    assert_eq!(
        script_var(tree, main_id, "sprite_registered"),
        Some(Variant::Bool(true)),
        "ClassDB.class_exists(\"Sprite2D\") must be true in a booted game"
    );
    // A class from a *different* registration table (audio/io/etc. rather than
    // 2D), so the test fails if boot wired up only one family.
    assert_eq!(
        script_var(tree, main_id, "node_registered"),
        Some(Variant::Bool(true)),
        "boot must populate every register_* table, not just the 2D one"
    );

    // -- the database is genuinely full --------------------------------------
    let count = gdobject::class_db::class_count();
    assert!(
        count >= 600,
        "boot must populate the full ClassDB (>= 600 classes), got {count}"
    );
}

/// The same guarantee through the other public boot entry point.
///
/// `project_boot::boot_project` is what loads a project from a directory (and,
/// via the shared `boot_from_project_text` funnel, what `boot_project_from_pck`
/// uses for an exported `.pck`). It builds its own `SceneTree` without going
/// through `EngineBootstrap`, so it needs — and here proves — its own
/// registration call rather than inheriting the one above.
#[test]
fn classdb_populated_by_boot_project_entry_point() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("project");
    write_project(&dir);

    let booted = patina_runner::project_boot::boot_project(&dir).expect("project boots");

    let main_id = booted.main_scene_root.expect("main scene instanced");
    let sprite_id = booted
        .tree
        .get_node_or_null(main_id, "Sprite2D")
        .expect("Sprite2D instanced");
    let sprite = booted.tree.get_node(sprite_id).expect("sprite node");

    assert_eq!(
        sprite.get_property("scale"),
        Variant::Vector2(gdcore::math::Vector2::ONE),
        "boot_project must seed class defaults too — it is a separate entry \
         point from EngineBootstrap and had the same empty-ClassDB bug"
    );
    assert_eq!(
        sprite.get_property("visible"),
        Variant::Bool(true),
        "CanvasItem default through boot_project"
    );
    assert!(
        gdobject::class_exists("Sprite2D"),
        "boot_project must populate ClassDB"
    );
}
