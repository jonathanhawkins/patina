//! pat-0q2kx.64 acceptance: a script in a loaded scene can introspect a node's
//! properties and revert one to its class default.
//!
//! Godot scripts, save systems and plugin code rely on
//! `Object.get_property_list()` returning `Array[Dictionary]` with
//! `name`/`class_name`/`type`/`hint`/`hint_string`/`usage`
//! (`core/object/object.cpp`), plus `property_can_revert`/`property_get_revert`
//! answering from the class default.
//!
//! Patina exposed none of it: `PropertyInfo` carried only name/default/hint, so
//! the dict shape was unrepresentable, and no interpreter arm existed for any of
//! these methods.
//!
//! This drives the real boot path — a `.tscn` with a script, stepped through
//! `EngineBootstrap` — so the ClassDB query, the interpreter arms and the scene
//! access all have to line up.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` mutates process-global `res://` roots, so serialize.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx64_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Sprite2D" type="Sprite2D" parent="."]
position = Vector2(0, 0)
"#;

/// Introspects the sibling sprite through the `ObjectId` method surface —
/// `$Sprite2D.get_property_list()` rather than a bare self-call, which is the
/// dispatch path these methods are implemented on.
const MAIN_GD: &str = "\
extends Node2D
var prop_count = 0
var pos_type = -1
var pos_usage = 0
var can_revert = false
var revert_x = -1.0

func _ready():
    var sprite = $Sprite2D
    var props = sprite.get_property_list()
    prop_count = props.size()
    for p in props:
        if p[\"name\"] == \"position\":
            pos_type = p[\"type\"]
            pos_usage = p[\"usage\"]
    can_revert = sprite.property_can_revert(\"position\")
    var rev = sprite.property_get_revert(\"position\")
    revert_x = rev.x
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"Introspect\"\nrun/main_scene=\"res://main.tscn\"\n",
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
#[test]
fn object_introspection_property_list() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("introspect");
    write_project(&dir);

    // ⚠️ `EngineBootstrap` does NOT populate the ClassDB — `init_servers`
    // documents that the caller registers classes before loading a scene that
    // references them. Without this the property list is legitimately EMPTY
    // (`get_property_list("Sprite2D")` for an unregistered class), which is what
    // an earlier version of this test tripped over: `prop_count == 0` was the
    // engine behaving as designed, not a missing introspection arm.
    gdobject::class_db::register_2d_classes();

    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");

    let main_id = boot
        .main_loop()
        .expect("main loop built")
        .tree()
        .get_node_by_path("/root/Main")
        .expect("main scene instanced");
    let tree = boot.main_loop().unwrap().tree();

    // The list is non-empty: a Sprite2D has registered properties.
    let count = script_var(tree, main_id, "prop_count").expect("prop_count read");
    match count {
        Variant::Int(n) => assert!(
            n > 0,
            "get_property_list() must return the node's registered properties, got {n}"
        ),
        other => panic!("prop_count should be an int, got {other:?}"),
    }

    // ⚠️ The dict carries a real Variant TYPE id for `position`, not 0/absent —
    // that field is what a serializer switches on.
    let pos_type = script_var(tree, main_id, "pos_type").expect("pos_type read");
    let expected_type = gdscript_interop::global_constants::variant_type_code(&Variant::Vector2(
        gdcore::math::Vector2::ZERO,
    ));
    assert_eq!(
        pos_type,
        Variant::Int(expected_type),
        "position's dict `type` must be the Vector2 type id"
    );

    // …and a non-zero usage, so a save system can tell it is storable.
    let usage = script_var(tree, main_id, "pos_usage").expect("pos_usage read");
    match usage {
        Variant::Int(u) => assert!(u != 0, "position must report a non-zero usage, got {u}"),
        other => panic!("pos_usage should be an int, got {other:?}"),
    }

    // Revert answers from the class default.
    assert_eq!(
        script_var(tree, main_id, "can_revert"),
        Some(Variant::Bool(true)),
        "a registered property reports property_can_revert() == true"
    );
    let revert_x = script_var(tree, main_id, "revert_x").expect("revert_x read");
    match revert_x {
        Variant::Float(x) => assert!(
            x.abs() < 1e-6,
            "property_get_revert(\"position\") returns the class default (0,0), got x={x}"
        ),
        other => panic!("revert_x should be a float, got {other:?}"),
    }
}
