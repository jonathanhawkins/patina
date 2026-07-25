//! pat-0q2kx.66 acceptance: the transform math types are usable from a script
//! in a running scene — members (read AND write), constructors, `*` operators
//! and methods.
//!
//! Godot scripts touch these constantly: `global_transform.origin`,
//! `transform * point`, `Transform2D(rot, pos)`, `basis.get_euler()`,
//! `Quaternion(axis, angle).slerp(...)` (`core/variant/variant_op.cpp` for the
//! operators, `extension_api.json` for the 73 methods).
//!
//! The dispatch lives in `gdscript_interop::variant_math`, reached from the
//! interpreter's member get/set, constructor, `*` and method-call paths. This
//! drives all of them through the real boot path, because a unit test on
//! `variant_math` alone would not prove the interpreter is WIRED to it — which
//! is exactly the failure mode this bead describes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx66_{}_{}_{}",
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
"#;

/// Exercises, in order: a `Transform2D(rotation, position)` constructor, member
/// READ of `.origin`, the `transform * Vector2` operator, member WRITE back onto
/// `.origin`, and `Quaternion.slerp`.
const MAIN_GD: &str = "\
extends Node2D
var ctor_x = -1.0
var ctor_y = -1.0
var xformed_x = -1.0
var xformed_y = -1.0
var written_x = -1.0
var slerp_w = -1.0
var approx_same = false

func _ready():
    # Transform2D(rotation, position): a 90-degree rotation at (5, 5).
    var t = Transform2D(1.5707963267948966, Vector2(5, 5))
    ctor_x = t.origin.x
    ctor_y = t.origin.y

    # `transform * point` rotates (1,0) onto (0,1) and adds the origin.
    var p = t * Vector2(1, 0)
    xformed_x = p.x
    xformed_y = p.y

    # Member WRITE: assigning through .origin must round-trip.
    t.origin = Vector2(9, 3)
    written_x = t.origin.x

    # Quaternion slerp: halfway between identity and identity is identity.
    var q0 = Quaternion(0, 0, 0, 1)
    var q1 = Quaternion(0, 0, 0, 1)
    var mid = q0.slerp(q1, 0.5)
    slerp_w = mid.w

    approx_same = t.is_equal_approx(t)
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"TransformMath\"\nrun/main_scene=\"res://main.tscn\"\n",
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

fn float_var(tree: &gdscene::SceneTree, node_id: gdscene::node::NodeId, var: &str) -> f64 {
    match script_var(tree, node_id, var) {
        Some(Variant::Float(f)) => f,
        Some(Variant::Int(i)) => i as f64,
        other => panic!("{var} should be a number, got {other:?}"),
    }
}

/// **The acceptance test.**
#[test]
fn transform_math_script_surface() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("transform");
    write_project(&dir);

    // The scene references Node2D, so the ClassDB has to be populated first —
    // `EngineBootstrap` deliberately leaves that to the caller.
    gdobject::class_db::register_2d_classes();

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

    // -- constructor + member read -------------------------------------------
    assert!(
        (float_var(tree, main_id, "ctor_x") - 5.0).abs() < 1e-5,
        "Transform2D(rot, pos).origin.x should be 5, got {}",
        float_var(tree, main_id, "ctor_x")
    );
    assert!(
        (float_var(tree, main_id, "ctor_y") - 5.0).abs() < 1e-5,
        "Transform2D(rot, pos).origin.y should be 5"
    );

    // -- `transform * Vector2` ------------------------------------------------
    //
    // A 90° rotation takes (1,0) to (0,1); adding the origin gives (5,6).
    assert!(
        (float_var(tree, main_id, "xformed_x") - 5.0).abs() < 1e-5,
        "t * Vector2(1,0) x should be 5, got {}",
        float_var(tree, main_id, "xformed_x")
    );
    assert!(
        (float_var(tree, main_id, "xformed_y") - 6.0).abs() < 1e-5,
        "t * Vector2(1,0) y should be 6, got {}",
        float_var(tree, main_id, "xformed_y")
    );

    // -- member WRITE round-trips --------------------------------------------
    assert!(
        (float_var(tree, main_id, "written_x") - 9.0).abs() < 1e-5,
        "assigning t.origin must be readable back as 9, got {}",
        float_var(tree, main_id, "written_x")
    );

    // -- Quaternion.slerp ------------------------------------------------------
    assert!(
        (float_var(tree, main_id, "slerp_w") - 1.0).abs() < 1e-5,
        "slerping identity with identity stays identity (w = 1), got {}",
        float_var(tree, main_id, "slerp_w")
    );

    // -- is_equal_approx -------------------------------------------------------
    assert_eq!(
        script_var(tree, main_id, "approx_same"),
        Some(Variant::Bool(true)),
        "a transform is approximately equal to itself"
    );
}
