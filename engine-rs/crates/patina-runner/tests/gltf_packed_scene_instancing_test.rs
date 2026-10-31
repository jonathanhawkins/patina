//! pat-mgixk.23 part 1: a `.glb` instantiates as a real scene.
//!
//! Godot imports a `.glb`/`.gltf` through `ResourceImporterScene`, whose
//! resource type is **PackedScene** — so `load("res://m.glb")` hands a script a
//! PackedScene and `instantiate()` builds a `Node3D` hierarchy with per-node
//! transforms and one `MeshInstance3D` per mesh-bearing node.
//!
//! Patina parsed the whole document and could not instantiate it:
//!   * `import_gltf` reported class `GltfDocument`, so `can_instantiate` was
//!     false and `instantiate()` raised "cannot instantiate a GltfDocument
//!     resource";
//!   * nothing built nodes from the parsed `nodes` array, so no transforms were
//!     ever applied;
//!   * `mesh3d_from_gltf_resource` took EVERY sub-resource in the document —
//!     i.e. every primitive of every mesh in the file — so each node resolved to
//!     the same combined mesh, and a node that matched nothing fell back to
//!     `Mesh3D::cube(1.0)`.
//!
//! Entry point: a real `project.godot` + `main.tscn` + `two_meshes.glb` on disk,
//! booted through `patina_runner::bootstrap`, with a GDScript doing
//! `add_child(load("res://two_meshes.glb").instantiate())`. Assertions are on
//! what the running game has: node paths, each node's own transform, and the
//! mesh the RENDER SERVER resolves for it (`RenderServer3DAdapter::resolve_mesh`
//! is the same entry the renderer uses — not a direct call to the new builder).
//!
//! ## Why the fixture's vertex counts are 3 / 4 / 5
//!
//! `two_meshes.glb` is generated, not hand-waved: `MeshA` -> mesh 0 (one
//! primitive, 3 verts) and `MeshB` -> mesh 1 (TWO primitives, 4 and 5 verts).
//! Every count differs, and none is 24, so an assertion cannot pass by reading
//! the wrong accessor, by picking up the other node's mesh, or by falling back
//! to the default cube. That is the whole point of the numbers.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdscene::render_server_3d::RenderServer3DAdapter;
use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

/// Committed alongside this test so the bytes under assertion never drift.
const TWO_MESHES_GLB: &[u8] = include_bytes!("fixtures/two_meshes.glb");

const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node3D"]
script = ExtResource("1_main")
"#;

const MAIN_GD: &str = r#"extends Node3D

var loaded_ok = false

func _ready():
    var s = load("res://two_meshes.glb")
    if s is PackedScene:
        loaded_ok = true
        print("ok_is_packed_scene")
    add_child(s.instantiate())
"#;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk23_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"GltfScene\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_GD).unwrap();
    std::fs::write(dir.join("two_meshes.glb"), TWO_MESHES_GLB).unwrap();
}

fn vec3_of(tree: &gdscene::SceneTree, id: gdscene::NodeId, key: &str) -> (f32, f32, f32) {
    match tree.get_node(id).expect("node exists").get_property(key) {
        Variant::Vector3(v) => (v.x, v.y, v.z),
        other => panic!("{key} should be a Vector3, got {other:?}"),
    }
}

/// **The acceptance test.**
#[test]
fn gltf_packed_scene_instancing_test() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("gltf");
    write_project(&dir);

    gdobject::class_db::register_2d_classes();
    gdobject::class_db::register_3d_classes();
    // ⚠️ `EngineBootstrap` does NOT configure `res://` — only the full
    // `project_boot` path calls `set_path_roots`. Without this the script's
    // `load("res://two_meshes.glb")` cannot resolve, `UnifiedLoader` fails, and
    // `load` falls through to its extension-guess chain, which has no `.glb`
    // arm — so the resource comes back as the wrong class and `is PackedScene`
    // is false. That is a property of the harness, not of the code under test.
    gdcore::file_access::set_path_roots(Some(dir.to_path_buf()), None);
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");
    boot.main_loop_mut().expect("main loop").step(1.0 / 60.0);

    let tree = boot.main_loop_mut().expect("main loop built").tree_mut();
    let main_id = tree
        .get_node_by_path("/root/Main")
        .expect("main scene instanced");

    // -- load() returned a PackedScene -------------------------------------
    //
    // Reported by the script itself, so it is the interpreter's `is` that
    // answered — the thing that was false while the importer said
    // "GltfDocument".
    let printed: Vec<String> = tree
        .get_script_mut(main_id)
        .expect("main.gd attached")
        .drain_print_output();
    assert!(
        printed.iter().any(|l| l == "ok_is_packed_scene"),
        "load(\"res://two_meshes.glb\") must be a PackedScene; got {printed:?}",
    );

    // -- instantiate() built the glTF hierarchy ----------------------------
    //
    // The scene root is named from the glTF scene (`TwoMeshes`), with the
    // file's root nodes beneath it.
    let a_id = tree
        .get_node_by_path("/root/Main/TwoMeshes/MeshA")
        .expect("MeshA must exist under the instanced glTF scene root");
    let b_id = tree
        .get_node_by_path("/root/Main/TwoMeshes/MeshB")
        .expect("MeshB must exist under the instanced glTF scene root");
    assert_eq!(
        tree.get_node(a_id).unwrap().class_name(),
        "MeshInstance3D",
        "a glTF node with a mesh becomes a MeshInstance3D",
    );

    // -- the authored transforms came through ------------------------------
    //
    // Distinct, non-zero, and different from each other: before this change no
    // transform was applied at all, so both nodes sat at the origin.
    assert_eq!(vec3_of(tree, a_id, "position"), (1.0, 2.0, 3.0));
    assert_eq!(vec3_of(tree, b_id, "position"), (-4.0, 5.0, -6.0));

    // -- each node addresses ITS OWN mesh ----------------------------------
    assert_eq!(
        tree.get_node(a_id).unwrap().get_property("gltf_mesh_index"),
        Variant::Int(0),
    );
    assert_eq!(
        tree.get_node(b_id).unwrap().get_property("gltf_mesh_index"),
        Variant::Int(1),
    );

    // -- and the render server resolves the real geometry -------------------
    //
    // `resolve_mesh` is the renderer's own entry point. 3 and 4 verts are the
    // fixture's actual accessor counts; 24 would be the fallback cube, and
    // seeing 4 (or 3+4+5 combined) on BOTH nodes is what the missing per-mesh
    // filtering used to produce.
    let mesh_a = RenderServer3DAdapter::resolve_mesh(tree, a_id);
    assert_eq!(
        mesh_a.vertices.len(),
        3,
        "MeshA must resolve to mesh 0's 3 vertices, not a cube or the other mesh",
    );
    assert_eq!(
        mesh_a.surface_count(),
        1,
        "mesh 0 declares one primitive, so one surface",
    );

    let mesh_b = RenderServer3DAdapter::resolve_mesh(tree, b_id);
    assert_eq!(
        mesh_b.vertices.len(),
        4,
        "MeshB's primary surface is mesh 1's first primitive (4 verts)",
    );
    assert_eq!(
        mesh_b.surface_count(),
        2,
        "mesh 1 declares TWO primitives, so surface_count must be 2",
    );
    assert_eq!(
        mesh_b.surfaces[0].vertices.len(),
        5,
        "and the second surface is the 5-vertex primitive",
    );
}
