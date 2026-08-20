//! pat-s3rrv.157 acceptance: `resource_local_to_scene` gives each scene
//! instance its own copy of a resource, and `_setup_local_to_scene` runs on it.
//!
//! # What the flag actually buys, in this engine
//!
//! ⚠️ Patina has no shared resource object to begin with: `packed_scene` never
//! builds a `Variant::Resource`, an inline resource is flattened onto its node
//! as companion properties, and `ResourceRef` carries no id. So "duplicate it"
//! was never the missing piece — **identity** was. Without an id there is
//! nothing for `get_instance_id()` to answer and the two copies a script stamps
//! are indistinguishable, which is exactly what this test measures.
//!
//! Godot calls the virtual ONLY on local-to-scene copies, so with the flag off
//! the callback never runs and both users read the stamp the file declared —
//! equal. With it on, each copy is stamped separately — different. Equality
//! versus difference is the observable; object identity is not.

use gdscene::local_to_scene::{clear_for_testing, local_resource_property, setup_local_to_scene};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;
use std::path::PathBuf;
use std::sync::OnceLock;

/// A per-process `res://` root holding the script both scenes reference.
fn res_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "patina_local_to_scene_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp res:// root is creatable");
        // ⚠️ Single-line body with explicit \n and \t: the GDScript parser here
        // wants real newlines and tab indentation, and a Rust multi-line string
        // would carry the source file's own indentation into the script.
        std::fs::write(
            dir.join("stamper.gd"),
            "extends Resource\nvar stamp = 0\nvar ran = 0\nfunc _setup_local_to_scene():\n\tran = 1\n\tstamp = get_instance_id()\n",
        )
        .expect("stamper.gd is writable");
        gdcore::file_access::set_path_roots(Some(dir.clone()), None);
        dir
    })
}

/// Two nodes referencing ONE sub-resource. `local` chooses the flag.
///
/// Two sibling nodes rather than two sub-scene instances: after loading, an
/// instanced sub-scene is simply a node carrying the same `__resref__`
/// companion, so this drives the identical code path with far less machinery
/// between the fixture and the behaviour under test.
fn scene_src(local: bool) -> String {
    format!(
        "[gd_scene load_steps=2 format=3]\n\
         \n\
         [ext_resource type=\"Script\" path=\"res://stamper.gd\" id=\"1_s\"]\n\
         \n\
         [sub_resource type=\"Resource\" id=\"Res_1\"]\n\
         script = ExtResource(\"1_s\")\n\
         resource_local_to_scene = {local}\n\
         stamp = 0\n\
         \n\
         [node name=\"Root\" type=\"Node\"]\n\
         \n\
         [node name=\"A\" type=\"Node\" parent=\".\"]\n\
         mat = SubResource(\"Res_1\")\n\
         \n\
         [node name=\"B\" type=\"Node\" parent=\".\"]\n\
         mat = SubResource(\"Res_1\")\n"
    )
}

/// Instances the scene, runs the pass, and returns each node's stamp.
type Stamp = (Variant, Variant);

fn stamps(local: bool) -> (Stamp, Stamp, usize) {
    res_root();
    clear_for_testing();

    let scene = PackedScene::from_tscn(&scene_src(local)).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let made = setup_local_to_scene(&mut tree, &scene);

    let mut out: Vec<Stamp> = Vec::new();
    for name in ["A", "B"] {
        let child = tree
            .find_child(scene_root, name, false)
            .unwrap_or_else(|| panic!("the scene must contain a node named {name}"));
        let handle = tree
            .get_node(child)
            .expect("child exists")
            .get_property("__local_res__mat");
        out.push(match handle {
            Variant::ObjectId(id) => (
                local_resource_property(id, "stamp"),
                local_resource_property(id, "ran"),
            ),
            // No copy was made for this node — reported as Nil so the assertion
            // below says "no copy" rather than failing on an unwrap.
            _ => (Variant::Nil, Variant::Nil),
        });
    }
    (out[0].clone(), out[1].clone(), made)
}

/// **The acceptance.** Flag on: two copies, each stamped separately.
#[test]
fn a_local_to_scene_resource_is_copied_and_stamped_per_instance() {
    let (a, b, made) = stamps(true);

    assert_eq!(
        made, 2,
        "each of the two nodes must get its own copy; {made} were made"
    );
    assert_ne!(
        a.0,
        Variant::Nil,
        "node A has no local copy — the flag was not honoured"
    );
    // ⚠️ Separates the two ways this can fail. Without it, "the callback never
    // ran" and "it ran but get_instance_id() answered 0" both surface as two
    // equal stamps, and the first round of this test could not tell them apart.
    assert_eq!(
        a.1,
        Variant::Int(1),
        "_setup_local_to_scene never ran on the copy"
    );
    assert_ne!(
        a.0, b.0,
        "the callback ran on both copies but they carry the same stamp ({:?}) — \
         the copies share one identity, which is the bug this bead is about",
        a.0
    );
}

/// Flag off: no copy, no callback, and the two agree.
///
/// ⚠️ This is the control. Without it the test above passes for a resource that
/// is copied unconditionally, which would be wrong in the opposite direction —
/// an author who wants one shared resource would silently get two.
#[test]
fn a_shared_resource_is_not_copied_and_not_stamped() {
    let (a, b, made) = stamps(false);

    assert_eq!(
        made, 0,
        "a resource without the flag must NOT be copied; {made} copies were made"
    );
    assert_eq!(
        a.0, b.0,
        "the two nodes must observe the same resource state when the flag is off"
    );
    assert_eq!(
        a.1,
        Variant::Nil,
        "the callback must NOT run for a resource that is not local to scene"
    );
}
