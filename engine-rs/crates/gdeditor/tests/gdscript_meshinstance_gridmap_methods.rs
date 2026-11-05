//! pat-mgixk.93 — a GDScript drives `GridMap`'s cell API and
//! `MeshInstance3D`'s material / collision API, and the results reach the 3D
//! render snapshot and the physics world.
//!
//! # What was broken
//!
//! `set_cell_item`, `get_cell_item`, `get_used_cells`, `local_to_map`,
//! `map_to_local`, `get_active_material` and `create_trimesh_collision` are all
//! registered in ClassDB (`class_db.rs:15129` and `:1711`), so `has_method`
//! answered **true** while every call returned **Nil**. Worse for the void ones:
//! `set_cell_item` and `create_trimesh_collision` sit in
//! `native_method_gaps::VOID_NOOP_METHODS`, so the interpreter returned Nil *on
//! purpose* — a script building a level out of GridMap cells saw no error and no
//! effect. The implementations already existed on `GridMap`; nothing connected
//! them to a script.
//!
//! # Why this test lives in `gdeditor`
//!
//! The bead's Wiring requirement is that changes show in the 3D render snapshot.
//! ⚠️ For GridMap that is **editor-only**: `grep GridMap
//! crates/gdscene/src/render_server_3d.rs` finds NOTHING — the runtime 3D path
//! never emits a GridMap cell. The sole consumer is
//! `gdeditor/src/viewport_3d_render.rs:411`, which is also the only place a
//! `MeshLibrary` `.tres` is ever loaded (`GridMap::from_node` captures only the
//! library's *path*). So the render oracle has to be the editor's, and the green
//! `gridmap_cells_render_from_scene.rs` (pat-s3rrv.89) is the test that already
//! pins this exact layer.
//!
//! ⚠️ Deliberately NOT asserted: `set_surface_override_material` /
//! `get_surface_override_material`. Those have been live since pat-mgixk.31
//! (`core_node_methods.rs:468`) with their own green acceptance; the setter is
//! used here only as the *input* that makes `get_active_material` meaningful.

use gdcore::math::Vector3;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{editor_3d_render_inputs, Render3DKind};
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// The mesh the library's item 0 names. Asserting on this exact path is what
/// distinguishes a rendered GRID CELL from the plain `MeshInstance3D` in the
/// same scene, which also emits a `Render3DKind::Mesh` item.
const ITEM_MESH: &str = "res://meshes/cube.obj";

/// A Godot `MeshLibrary` `.tres` whose usable item is **id 0**, the id the
/// acceptance passes to `set_cell_item`. `MeshLibrary::from_tres` parses the id
/// with `parse::<i32>()` and special-cases nothing, so 0 is as valid as 1.
const MESH_LIBRARY_TRES: &str = r#"[gd_resource type="MeshLibrary" load_steps=2 format=3]

[ext_resource type="Mesh" path="res://meshes/cube.obj" id="1_cube"]

[resource]
item/0/name = "cube"
item/0/mesh = ExtResource("1_cube")
"#;

/// The scene, with the mesh library's real path interpolated.
///
/// `mesh_library` is written as a PLAIN quoted path, not an `ExtResource(..)`
/// reference: `GridMap::from_node` takes the raw string only when it is not a
/// reference literal, and the library lives in a tempdir that no `res://` lookup
/// could resolve.
///
/// 🔑 `data` is deliberately absent, so the grid starts with ZERO cells. That is
/// what makes `get_used_cells().size() == 1` a real measurement rather than an
/// off-by-one against pre-authored content.
fn scene_source(library_path: &str) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Grid" type="GridMap" parent="."]
cell_size = Vector3(2, 2, 2)
mesh_library = "{library_path}"

[node name="Mesh" type="MeshInstance3D" parent="."]
"#
    )
}

/// ⚠️ The calls use the INLINE `$Child.method()` form on purpose. A call on a
/// variable holding the node consults the interpreter's own ObjectId table
/// before `SceneAccess::call_node_method`, which is a DIFFERENT dispatch
/// surface; the inline form is the one the green
/// `surface_override_material_accessor_from_script` proves reaches this bridge.
/// Do not refactor these to `var g = $Grid`.
///
/// `stage` is staged so a raise names the statement that failed instead of
/// collapsing into one opaque "did not complete".
const SCRIPT: &str = "\
extends Node3D
var stage = 0
var got_item = -99
var used = -1
var round_trip = false
var active_nil = true
var ray_hit = false
var ray_done = false

func _ready():
    self.stage = 1
    $Grid.set_cell_item(Vector3i(1, 0, 2), 0)
    self.stage = 2
    self.got_item = $Grid.get_cell_item(Vector3i(1, 0, 2))
    self.stage = 3
    self.used = $Grid.get_used_cells().size()
    self.stage = 4
    var c = $Grid.local_to_map(Vector3(2.5, 0.5, 5.5))
    var back = $Grid.map_to_local(c)
    self.round_trip = $Grid.local_to_map(back) == c
    self.stage = 5
    $Mesh.set_surface_override_material(0, \"res://red_mat.tres\")
    self.stage = 6
    self.active_nil = $Mesh.get_active_material(0) == null
    self.stage = 7
    $Mesh.create_trimesh_collision()
    self.stage = 8

func _physics_process(delta):
    if self.ray_done:
        return
    if self.stage < 8:
        return
    self.ray_done = true
    var q = PhysicsRayQueryParameters3D.new()
    q.from = Vector3(0, 5, 0)
    q.to = Vector3(0, -5, 0)
    var space = get_world_3d().direct_space_state
    var hit = space.intersect_ray(q)
    if hit:
        self.ray_hit = true
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .and_then(|s| s.get_property(name))
        .unwrap_or(Variant::Nil)
}

/// World positions of every rendered cell carrying the library item's mesh.
fn item_cell_origins(tree: &SceneTree) -> Vec<Vector3> {
    editor_3d_render_inputs(&Viewport3D::default(), tree)
        .items
        .into_iter()
        .filter_map(|i| match i.kind {
            Render3DKind::Mesh { path } if path == ITEM_MESH => Some(i.global_transform.origin),
            _ => None,
        })
        .collect()
}

/// **The acceptance test.**
#[test]
fn gdscript_meshinstance_gridmap_methods() {
    // ⚠️ REQUIRED, for two independent reasons:
    //   * `PhysicsRayQueryParameters3D.new()` resolves through ClassDB and
    //     nothing else populates it — without this the constructor answers Nil
    //     and the ray has no endpoints, which looks exactly like a broken query.
    //   * `GridMap` is registered in the **2D** set (`class_db.rs:11794`)
    //     despite being a Node3D. Without it `is_class("Node3D")` answers false,
    //     `collect_scene_content` skips the node, and the render list comes back
    //     empty — a failure that reads as "the mesh library never loaded".
    // `register_all_classes` covers both.
    gdobject::class_db::register_all_classes();

    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("lib.tres");
    std::fs::write(&library, MESH_LIBRARY_TRES).expect("write the mesh library");

    let source = scene_source(&library.to_string_lossy());
    let scene = PackedScene::from_tscn(&source).expect("scene parses");
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let root = add_packed_scene_to_tree(&mut tree, tree_root, &scene).expect("scene instances");

    // -- CONTROL, taken BEFORE the script runs -------------------------------
    //
    // 🔑 This is what makes the render assertion a differential rather than an
    // absolute. The same scene, the same renderer, the same library — the only
    // difference below is that the script has run. Without it, "one cell
    // rendered" could just as easily mean the fixture authored a cell.
    assert!(
        item_cell_origins(&tree).is_empty(),
        "before the script runs the grid must render NO cells; finding one means \
         the fixture authored cells and the measurement below is vacuous"
    );

    tree.attach_script(
        root,
        Box::new(GDScriptNodeInstance::from_source(SCRIPT, root).expect("script parses")),
    );
    // 🔑 REQUIRED, and the first run of this test failed for exactly its absence
    // (stage stayed 0 — `_ready` never ran at all, which is why the stage
    // counter starts at 0 rather than 1).
    //
    // ⚠️ `_ready` is dispatched by ENTER_TREE, and `_propagate_ready` notifies a
    // node only once (`lifecycle.rs:40`). These nodes entered the tree during
    // `add_packed_scene_to_tree` ABOVE, before the script existed, so the
    // notification had already been spent on a script-less node and no later
    // `MainLoop::step` re-runs it. `process_script_ready` is the path that
    // delivers `_ready` to a script that "was not there the first time"
    // (`scene_tree.rs:6226`) — the same call the green
    // `surface_override_material_accessor_from_script` makes immediately after
    // its own `attach_script`.
    //
    // It must also stay BEFORE `MainLoop::new`: `_ready` is what creates the
    // collision body, and `register_physics_bodies` below has to see it.
    tree.process_script_ready(root);

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    // Several frames: `_ready` creates the collision body during the first
    // script phase, and `sync_to_physics` (main_loop.rs:4086) is what adds a
    // newly created body — so the ray cannot run until a later frame. The
    // script guards on `stage == 8` rather than on a frame count.
    for _ in 0..6 {
        ml.step(1.0 / 60.0);
    }

    // -- 0. the script ran to completion -------------------------------------
    //
    // Checked first: every assertion below reads a var this sets, so a raise
    // part-way through would make them report stale initializers instead of the
    // real failure. The stage names the statement that raised.
    assert_eq!(
        script_var(&ml, root, "stage"),
        Variant::Int(8),
        "`_ready` must run to completion. The stage reached names the statement \
         that raised: 1=set_cell_item 2=get_cell_item 3=get_used_cells \
         4=local_to_map/map_to_local 5=set_surface_override_material \
         6=get_active_material 7=create_trimesh_collision"
    );

    // -- 1. GridMap cell round trip through the script ------------------------
    assert_eq!(
        script_var(&ml, root, "got_item"),
        Variant::Int(0),
        "get_cell_item must return the item the script just set. -1 means the \
         cell never landed in the node's `data` dictionary, which is the only \
         place the render path reads"
    );
    assert_eq!(
        script_var(&ml, root, "used"),
        Variant::Int(1),
        "the one set cell must be the only used cell"
    );
    assert_eq!(
        script_var(&ml, root, "round_trip"),
        Variant::Bool(true),
        "map_to_local(local_to_map(p)) must land back in the same cell. Godot \
         floors in local_to_map and adds the half-cell centre offset in \
         map_to_local, so the pair round-trips only if BOTH apply their own half \
         of that convention"
    );

    // -- 2. 🔑 the cell reaches the 3D RENDER SNAPSHOT -------------------------
    //
    // The wiring requirement. cell (1,0,2) at cell_size 2 → world (3, 1, 5):
    // the cell CENTRE, as Godot's `map_to_local` (pat-2ifdx).
    let origins = item_cell_origins(ml.tree());
    assert_eq!(
        origins.len(),
        1,
        "the script's cell must render exactly one mesh item. 0 means \
         set_cell_item never reached the node's `data` dictionary, so \
         `GridMap::from_node` rebuilt an empty grid for the renderer"
    );
    let origin = origins[0];
    assert!(
        (origin.x - 3.0).abs() < 1e-4
            && (origin.y - 1.0).abs() < 1e-4
            && (origin.z - 5.0).abs() < 1e-4,
        "the rendered cell must sit at its centre (3, 1, 5), got \
         ({}, {}, {}) — a wrong position means the cell decoded to the wrong \
         coordinates, i.e. the packed triple was encoded wrong",
        origin.x,
        origin.y,
        origin.z
    );

    // -- 3. MeshInstance3D.get_active_material --------------------------------
    assert_eq!(
        script_var(&ml, root, "active_nil"),
        Variant::Bool(false),
        "get_active_material(0) must return the override the script set on \
         surface 0, not null"
    );

    // -- 4. create_trimesh_collision builds a body a RAY HITS -----------------
    //
    // The fixture has no other collider, so nothing could answer this ray
    // before the change — the hit is attributable to the new body alone.
    assert_eq!(
        script_var(&ml, root, "ray_done"),
        Variant::Bool(true),
        "the ray must actually have been cast; false means `_physics_process` \
         never saw stage 8 and the hit assertion below would be vacuous"
    );
    assert_eq!(
        script_var(&ml, root, "ray_hit"),
        Variant::Bool(true),
        "a downward ray through the origin must hit the StaticBody3D that \
         create_trimesh_collision added under the MeshInstance3D. false means \
         either no body was created or its ConcavePolygonShape3D carried no \
         faces — `shape_from_node_3d` reads the soup from the `data` key"
    );

    // The structure Godot builds, asserted separately from the ray so a failure
    // distinguishes "no body" from "body present but not hittable".
    let body = ml
        .tree()
        .find_child(root, "Mesh_col", true)
        .expect("create_trimesh_collision must add a `<name>_col` StaticBody3D child");
    assert_eq!(
        ml.tree().get_node(body).map(|n| n.class_name().to_string()),
        Some("StaticBody3D".to_string()),
        "the collision child must be a StaticBody3D, as Godot's \
         create_trimesh_collision builds"
    );
}
