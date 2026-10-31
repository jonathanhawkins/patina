//! pat-mgixk.71 acceptance: the local-to-scene pass is WIRED into scene
//! loading, not merely implemented.
//!
//! Godot ground truth: `SceneState::instantiate`
//! (`scene/resources/packed_scene.cpp` `make_local_resource`) duplicates every
//! sub-resource with `resource_local_to_scene = true` per instanced scene and
//! calls `Resource::setup_local_to_scene`, which dispatches
//! `GDVIRTUAL_CALL(_setup_local_to_scene)`.
//!
//! # What was broken
//!
//! `gdscene::local_to_scene` was a **finished module with no production
//! caller**. `grep -rn "setup_local_to_scene(" crates/` found the definition and
//! exactly one call: its own unit-level test. So the module's own test was green
//! while every real scene load — runner boot, `change_scene_to_*`, a script's
//! `PackedScene.instantiate()` — shared one resource across instances and never
//! fired the virtual.
//!
//! 🔑 **This test never calls `setup_local_to_scene`.** That is the entire
//! point: the sibling test `resource_local_to_scene_setup_virtual` used to call
//! it directly and passed throughout the bug. Here the only thing that can
//! produce a copy is the loader, so a copy appearing IS the wiring.

use gdscene::local_to_scene::{clear_for_testing, local_resource_property};
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;
use std::path::PathBuf;
use std::sync::OnceLock;

/// A per-process `res://` root holding the resource script and the sub-scene.
fn res_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("patina_mgixk71_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp res:// root is creatable");
        // ⚠️ Single-line body with explicit \n and \t: the GDScript parser wants
        // real newlines and tab indentation, and a Rust multi-line string would
        // carry this file's own indentation into the script.
        std::fs::write(
            dir.join("stamper.gd"),
            "extends Resource\nvar stamp = 0\nvar ran = 0\nfunc _setup_local_to_scene():\n\tran = 1\n\tstamp = get_instance_id()\n",
        )
        .expect("stamper.gd is writable");
        std::fs::write(dir.join("child.tscn"), CHILD_TSCN).expect("child.tscn is writable");
        gdcore::file_access::set_path_roots(Some(dir.clone()), None);
        dir
    })
}

/// One node carrying a local-to-scene sub-resource. Used both as a standalone
/// scene instanced twice, and as the scene a script instantiates at runtime.
const CHILD_TSCN: &str = "[gd_scene load_steps=2 format=3]\n\
     \n\
     [ext_resource type=\"Script\" path=\"res://stamper.gd\" id=\"1_s\"]\n\
     \n\
     [sub_resource type=\"Resource\" id=\"Res_1\"]\n\
     script = ExtResource(\"1_s\")\n\
     resource_local_to_scene = true\n\
     stamp = 0\n\
     \n\
     [node name=\"Child\" type=\"Node\"]\n\
     mat = SubResource(\"Res_1\")\n";

/// The `(stamp, ran)` of the copy on `node`, or `None` when it has no copy.
fn copy_of(tree: &SceneTree, node: NodeId) -> Option<(Variant, Variant)> {
    match tree.get_node(node)?.get_property("__local_res__mat") {
        Variant::ObjectId(id) => Some((
            local_resource_property(id, "stamp"),
            local_resource_property(id, "ran"),
        )),
        _ => None,
    }
}

/// **The acceptance test.** Instancing the same scene twice through the loader
/// gives each instance its own stamped copy — with nothing in this test calling
/// the pass.
#[test]
fn resource_local_to_scene_wired_through_scene_load() {
    res_root();
    clear_for_testing();

    let scene = PackedScene::from_tscn(CHILD_TSCN).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // The same `PackedScene`, instanced twice — Godot's `make_local_resource`
    // runs per instantiate, so these must not share a copy.
    let first = add_packed_scene_to_tree(&mut tree, root, &scene).expect("first instance");
    let second = add_packed_scene_to_tree(&mut tree, root, &scene).expect("second instance");
    assert_ne!(first, second, "precondition: two separate instances");

    let a = copy_of(&tree, first).unwrap_or_else(|| {
        panic!(
            "the FIRST instance has no `__local_res__mat` — the loader never ran \
             the local-to-scene pass, which is the gap this bead closes"
        )
    });
    let b = copy_of(&tree, second)
        .expect("the SECOND instance must get its own copy, not reuse the first");

    // The virtual fired on each copy. Separated from the stamp comparison
    // below because "never ran" and "ran but get_instance_id() answered 0" both
    // surface as two equal stamps otherwise.
    assert_eq!(
        a.1,
        Variant::Int(1),
        "`_setup_local_to_scene` must run on the first instance's copy"
    );
    assert_eq!(
        b.1,
        Variant::Int(1),
        "...and on the second instance's copy, exactly once each"
    );

    // 🔑 The per-instance observable: two different identities. One shared copy
    // — the pre-bead behaviour — makes these equal.
    assert_ne!(
        a.0, b.0,
        "the two instances must carry DIFFERENT stamps; equal ({:?}) means they \
         share one copy",
        a.0
    );
    // ...and neither is the file's declared default, which is what both would
    // read if the callback had not run at all.
    assert_ne!(
        a.0,
        Variant::Int(0),
        "a stamped copy must not still carry the .tscn's `stamp = 0`"
    );
}

/// The script path: `preload(..).instantiate()` must get the pass too.
///
/// This is a different entry point from the one above —
/// `instantiate_packed_scene` builds a DETACHED subtree through
/// `instantiate_packed_scene_detached` — so it is wired separately and asserted
/// separately.
const SPAWNER_GD: &str = "\
extends Node
var spawned = false

func _ready():
    var n = preload(\"res://child.tscn\").instantiate()
    add_child(n)
    self.spawned = true
";

#[test]
fn a_script_instantiated_scene_gets_its_own_local_resource() {
    res_root();
    clear_for_testing();

    let main_src = "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node\"]\n";
    let scene = PackedScene::from_tscn(main_src).expect("main scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &scene).expect("main instances");
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(SPAWNER_GD, main).expect("script parses")),
    );

    let mut ml = MainLoop::new(tree);
    // `MainLoop::step` does not fire `_ready`; the host does it after loading a
    // scene, as `patina-runner` boot does.
    ml.tree_mut().process_script_ready(main);
    ml.step(1.0 / 60.0);

    // CONTROL: the script ran and the spawn happened. Without this, "no copy"
    // below cannot be told apart from "`preload(..).instantiate()` returned
    // null and nothing was ever added".
    let spawned = ml
        .tree()
        .get_script(main)
        .and_then(|s| {
            s.list_properties()
                .into_iter()
                .find(|p| p.name == "spawned")
                .map(|p| p.default_value)
        })
        .unwrap_or(Variant::Nil);
    assert_eq!(
        spawned,
        Variant::Bool(true),
        "precondition: the spawner script's `_ready` must have completed"
    );

    let child = ml
        .tree()
        .find_child(main, "Child", true)
        .expect("precondition: the instantiated scene must be in the tree");

    let copy = copy_of(ml.tree(), child).unwrap_or_else(|| {
        panic!(
            "a script-instantiated scene must get its own local-to-scene copy; \
             none means `PackedScene.instantiate()` skips the pass"
        )
    });
    assert_eq!(
        copy.1,
        Variant::Int(1),
        "`_setup_local_to_scene` must run on a script-spawned copy too"
    );
    assert_ne!(
        copy.0,
        Variant::Int(0),
        "the spawned copy must be stamped, not left at the .tscn default"
    );
}
