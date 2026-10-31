//! **Acceptance test for pat-mgixk.105.** Godot's indexed get/set on the
//! struct-like builtins, driven from a scene script.
//!
//! # What was broken
//!
//! `index_into` only read packed arrays, `Array`, `Projection`, `Vector4`,
//! `Vector4i`, `Dictionary`, `Resource` and `String`; `set_index` only wrote
//! packed arrays, `Array`, `Dictionary` and `Resource`. So
//! `Vector2`/`Vector2i`/`Vector3`/`Vector3i`/`Color`/`Quaternion`/`Basis`/
//! `Transform2D` all fell through to "cannot index", and `v[0] = 1.0` on any
//! vector errored — even though Godot registers every one of them in
//! `core/variant/variant_setget.cpp` (`INDEXED_SETGET_STRUCT_TYPED`).
//!
//! `String` indexing used the **byte** length for negative offsets
//! (`s.len() as i64 + i`) while reading with `chars().nth()`, so `"héllo"[-1]`
//! computed 6 - 1 = 5 against a 5-char string and went out of bounds.
//!
//! # 🔑 Why there is an extra Basis case the bead did not ask for
//!
//! The acceptance text asks for `Basis.IDENTITY[1]` == `Vector3(0, 1, 0)`.
//! **That assertion cannot distinguish a correct implementation from a
//! transposed one**, because the identity matrix is its own transpose.
//!
//! The distinction is real and easy to get wrong: Godot's `Basis` stores
//! `Vector3 rows[3]` and `operator[]` returns `rows[i]`, while Patina's
//! `gdcore::math3d::Basis` stores `x`/`y`/`z` as **columns** — its own
//! `xform` computes `x*v.x + y*v.y + z*v.z`, a linear combination of the stored
//! vectors, which is the column convention. So `basis[i]` has to *gather*
//! `Vector3(x[i], y[i], z[i])` rather than hand back `x`, `y` or `z`.
//!
//! Returning the stored vector instead would pass every clause the bead lists
//! and be wrong for every non-identity basis. Hence `BASIS_COLUMNS` below,
//! where the row and the column differ.
//!
//! # Wiring
//!
//! Boots a project through `patina_runner`; a `Node2D` script performs the
//! indexing in `_ready`. The write half is asserted on the **node's own
//! `position` property** read back through the SceneTree, not on a script
//! variable, so it proves the indexed assignment reached the engine rather than
//! just mutating a local copy.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

/// What the script writes into `position[0]`.
///
/// ⚠️ Chosen to be exactly representable in `f32` so the assertion can be an
/// equality rather than a tolerance: the property stores `f32`, and a value
/// like 0.1 would arrive as 0.100000001490116 and make an `==` impossible.
const POS_X: f32 = 42.0;

/// The blue channel of the Color under test.
///
/// ⚠️ Compared with a tolerance, NOT `==`. `Color` stores `f32`, so 0.3 comes
/// back as 0.30000001192092896 once widened to `f64` — an exact comparison
/// against the `f64` literal 0.3 can never pass.
const COLOR_B: f64 = 0.3;

/// Columns of the non-identity Basis, and the row index 0 must gather from
/// them. Godot's `Basis(x_axis, y_axis, z_axis)` takes **columns**
/// (`set_columns`), and `basis[0]` returns **row** 0 — so with these columns
/// row 0 is `(1, 4, 7)`, while the first *column* is `(1, 2, 3)`.
const BASIS_COLUMNS: &str = "Basis(Vector3(1, 2, 3), Vector3(4, 5, 6), Vector3(7, 8, 9))";

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk105_index_{}_{}_{}",
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

/// ⚠️ `"héllo"` is deliberately non-ASCII: its byte length is 6 and its char
/// length is 5, which is the entire point of the String clause. An ASCII
/// fixture would pass with the byte-length bug still in place.
const MAIN_GD_TEMPLATE: &str = "\
extends Node2D

var v2i_y = -1
var color_b = -1.0
var t2d_origin = null
var identity_row = null
var gathered_row = null
var last_char = \"\"
var ready_done = false

func _ready():
    position[0] = 42.0
    v2i_y = Vector2i(3, 4)[1]
    color_b = Color(0.1, 0.2, 0.3, 0.4)[2]
    t2d_origin = Transform2D.IDENTITY[2]
    identity_row = Basis.IDENTITY[1]
    gathered_row = {basis}[0]
    last_char = \"héllo\"[-1]
    ready_done = true
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"VariantIndex\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(
        dir.join("main.gd"),
        MAIN_GD_TEMPLATE.replace("{basis}", BASIS_COLUMNS),
    )
    .unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
    gdobject::class_db::register_2d_classes();
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

/// ⚠️ `get_property`, not `list_properties().default_value` — the latter is
/// parse-time metadata and would report each variable's declared initializer
/// forever.
fn script_var(tree: &gdscene::SceneTree, node: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(var)
        .unwrap_or(Variant::Nil)
}

/// **The acceptance test.**
#[test]
fn variant_indexed_access_test() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("main");
    write_project(&dir);

    let mut boot = boot(&dir);
    let tree = boot.main_loop().expect("main loop built").tree();
    let main_id = tree.get_node_by_path("/root/Main").expect("Main");

    // -- 0. `_ready` ran to completion --------------------------------------
    //
    // 🔑 Checked FIRST. Every clause below is a separate `cannot index` /
    // `invalid index assignment` runtime error at HEAD, and the FIRST one
    // aborts `_ready` — so without this, the output would be six sentinels and
    // no indication that the script died on line one.
    assert_eq!(
        script_var(tree, main_id, "ready_done"),
        Variant::Bool(true),
        "_ready must run to completion. `false` means it raised partway — at \
         HEAD that is `position[0] = 42.0` on the very first line, because \
         set_index had no Vector2 arm"
    );

    // -- 1. 🔑 the WRITE reached the engine ----------------------------------
    //
    // Asserted on the node's own property through the SceneTree, not on a
    // script var: that is what distinguishes "the indexed assignment wrote
    // back through the property chain" from "a local copy was mutated and
    // discarded".
    match tree
        .get_node(main_id)
        .expect("Main node")
        .get_property("position")
    {
        Variant::Vector2(v) => {
            assert_eq!(
                v.x, POS_X,
                "`position[0] = 42.0` must write the node's x. Still 0 means \
                 the component write never reached the property"
            );
            assert_eq!(
                v.y, 0.0,
                "`position[0]` must touch ONLY x — y changing means the write \
                 replaced the whole Vector2 instead of one component"
            );
        }
        other => panic!("`position` should be a Vector2, got {other:?}"),
    }

    // -- 2. integer and float component reads --------------------------------
    assert_eq!(
        script_var(tree, main_id, "v2i_y"),
        Variant::Int(4),
        "`Vector2i(3, 4)[1]` must read the y component as an Int. Godot's \
         indexing_return_type for Vector2i is int, not float"
    );
    match script_var(tree, main_id, "color_b") {
        Variant::Float(b) => assert!(
            (b - COLOR_B).abs() < 1e-6,
            "`Color(0.1, 0.2, 0.3, 0.4)[2]` must read the BLUE channel ({}), \
             got {b}. 0.2 would mean the index is off by one and 0.4 that it \
             counts from the end",
            COLOR_B
        ),
        other => panic!("`color_b` should be a Float, got {other:?}"),
    }

    // -- 3. the aggregate reads: a column and a row --------------------------
    //
    // Transform2D indexes by COLUMN with 2 == origin, which is Godot's
    // `columns[i]` and happens to match Patina's `{x, y, origin}` storage
    // directly.
    assert_eq!(
        script_var(tree, main_id, "t2d_origin"),
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        "`Transform2D.IDENTITY[2]` must be the origin, Vector2(0, 0)"
    );
    assert_eq!(
        script_var(tree, main_id, "identity_row"),
        Variant::Vector3(gdcore::math::Vector3::new(0.0, 1.0, 0.0)),
        "`Basis.IDENTITY[1]` must be Vector3(0, 1, 0)"
    );

    // -- 4. 🔑 row, not column — the clause the bead's own fixture cannot test
    assert_eq!(
        script_var(tree, main_id, "gathered_row"),
        Variant::Vector3(gdcore::math::Vector3::new(1.0, 4.0, 7.0)),
        "`{}[0]` must be Godot's ROW 0, which gathers the first component of \
         each stored column: Vector3(1, 4, 7). Getting Vector3(1, 2, 3) means \
         the implementation returned the stored COLUMN `x` instead — a \
         transposed accessor, which the identity case above cannot detect \
         because the identity matrix is its own transpose",
        BASIS_COLUMNS
    );

    // -- 5. String indexes by CHARACTER, negative from the end ---------------
    assert_eq!(
        script_var(tree, main_id, "last_char"),
        Variant::String("o".into()),
        "`\"héllo\"[-1]` must be \"o\". The string is 6 BYTES and 5 CHARS, so \
         a negative offset computed from `s.len()` lands at char 5 and raises \
         IndexOutOfBounds — which is exactly the bug this pins"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
