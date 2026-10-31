//! pat-mgixk.31 — `MeshInstance3D.set/get_surface_override_material(i, ..)`
//! reach the per-surface property the render path reads.
//!
//! Godot ground truth: `scene/3d/mesh_instance_3d.cpp` binds both as ordinary
//! methods over the indexed `surface_material_override/<i>` property, so
//! `mesh.set_surface_override_material(0, m)` is how a script restyles one
//! surface of a multi-material model at runtime.
//!
//! # What was wrong
//!
//! ⚠️ The call was not merely unimplemented — it was **enumerated as ignored**.
//! `set_surface_override_material` sits in
//! `native_method_gaps::VOID_NOOP_METHODS`, so the interpreter returned `Nil`
//! on purpose: a script assigning a per-surface material got no error and no
//! effect. And the generic ClassDB accessor bridge cannot serve it — that arm
//! declines any call without exactly one argument, deliberately, because
//! writing `args[0]` for an indexed accessor would store the INDEX as the
//! value. So this needed the same ahead-of-generic treatment `set_param` has.
//!
//! Entry point, per the bead's Wiring requirement: a `.tscn` whose
//! `MeshInstance3D` already carries a real material on surface 1, and an
//! attached GDScript that moves it to surface 0 through the two methods. The
//! assertions read `surface_material_override/<i>` off the NODE — the same key
//! `render_server_3d::surface_materials_for_node` consults, so a pass means the
//! value landed where the renderer will find it rather than in a parallel
//! store.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

/// Surface 1 carries the material; surface 0 starts bare. That direction is
/// deliberate: `scene_saver.rs` treats override `0` specially (it flattens to
/// `__material__`), so authoring the fixture on `1` and writing to `0` exercises
/// the plain indexed path in both directions.
const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="StandardMaterial3D" id="Mat_B"]
albedo_color = Color(1, 0, 0, 1)

[node name="Root" type="Node3D"]

[node name="Mesh" type="MeshInstance3D" parent="."]
surface_material_override/1 = SubResource("Mat_B")
"#;

/// ⚠️ STAGED on purpose. A single `ran` flag turns any raise in `_ready` into
/// one opaque "did not complete", which is exactly what the first run of this
/// test reported — and with `get_surface_override_material` being non-void (so
/// an undispatched call RAISES rather than answering Nil) there were three
/// candidate statements and no way to tell them apart. The stage counter makes
/// the next failure name the statement.
const SCRIPT: &str = "\
extends Node3D
var stage = 0
var got_was_nil = true
var cls = \"unset\"
func _ready():
    self.cls = $Mesh.get_class()
    self.stage = 1
    var m = $Mesh.get_surface_override_material(1)
    self.stage = 2
    self.got_was_nil = (m == null)
    $Mesh.set_surface_override_material(0, m)
    self.stage = 3
";

fn node_prop(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_node(node)
        .expect("node exists")
        .get_property(name)
}

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

#[test]
fn surface_override_material_accessor_from_script() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let mesh = tree
        .find_child(scene_root, "Mesh", true)
        .expect("the MeshInstance3D exists");

    let script = GDScriptNodeInstance::from_source(SCRIPT, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));
    tree.process_script_ready(scene_root);

    // Each stage names the statement that would have raised, so a failure here
    // is a diagnosis rather than a mystery:
    //   0 = `_ready` never ran at all
    //   1 = get_surface_override_material raised (undispatched; it is non-void,
    //       so the interpreter raises rather than answering Nil)
    //   2 = set_surface_override_material raised
    //
    // ⚠️ Both calls use the INLINE `$Mesh.method()` form, not a method call on
    // a variable holding the node. Those are different dispatch surfaces — a
    // call on an ObjectId-valued variable consults the interpreter's own
    // ObjectId table before `SceneAccess::call_node_method` — and the inline
    // form is the one the green `node3d_accessor_methods_reach_properties`
    // proves reaches this bridge (`$Bulb.set_param(..)`). An earlier revision
    // of this test used a `var probe = $Mesh` and must not be reintroduced.
    // 🔑 The gate's INPUT, recorded before anything else. The arm gates on
    // `is_parent_class(class_name, "MeshInstance3D")`, and this is the only
    // input to that gate I could not confirm statically. If this is not
    // "MeshInstance3D" the gate is declining correctly and the bug is upstream
    // in how the node is classified, not in the arm.
    assert_eq!(
        script_var(&tree, scene_root, "cls"),
        Variant::String("MeshInstance3D".to_string()),
        "the scene node must classify as MeshInstance3D — that is what the \
         arm's class gate is tested against"
    );
    assert_eq!(
        script_var(&tree, scene_root, "stage"),
        Variant::Int(3),
        "`_ready` must run to completion; the stage reached names the statement \
         that raised (1 = the getter, 2 = the setter)"
    );

    // 🔑 The getter returned a REAL material, not Nil. This is the clause the
    // void-noop entry used to defeat: `get_surface_override_material` is not
    // itself void, but with nothing dispatching the family the script had no
    // way to read a material the scene plainly set.
    assert_eq!(
        script_var(&tree, scene_root, "got_was_nil"),
        Variant::Bool(false),
        "get_surface_override_material(1) must return the material the .tscn \
         set on surface 1, not null"
    );

    // 🔑 The setter landed on the right KEY and the right INDEX.
    //
    // ⚠️ What this does NOT show is that a pixel changed, and an earlier
    // revision of this comment claimed it did. `surface_materials_for_node`
    // (`render_server_3d.rs:3207`) does `const PREFIX: &str = "__surfmat"` and
    // then `strip_prefix(PREFIX)` — it never looks at
    // `surface_material_override/N`. Both render channels go through that one
    // function (the editor viewport at `viewport_3d_render.rs:190`, whose own
    // comment says it resolves overrides "through the SAME
    // `surface_materials_for_node` the render-server adapter uses"), so neither
    // reads this key.
    //
    // Those `__surfmat<idx>__*` companions are written at **LOAD** time by
    // `packed_scene.rs:667-698`, from a `Variant::String("SubResource:<id>")`
    // value, and `idx == 0` is skipped there because surface 0 belongs to the
    // `__material__*` pass. So a script write produces no companion at all, at
    // any index.
    //
    // Making a script write observable needs a runtime path — either
    // `surface_materials_for_node` resolving the raw key itself (which means
    // materialising a resource reference inside the render path), or this arm
    // writing the companions. Both are their own bead; see pat-mgixk.31's
    // split recommendation. Until then these assertions pin the dispatch and
    // the indexing, which is all they can honestly claim.
    let moved = node_prop(&tree, mesh, "surface_material_override/0");
    let source = node_prop(&tree, mesh, "surface_material_override/1");
    assert!(
        !matches!(moved, Variant::Nil),
        "set_surface_override_material(0, m) must write \
         `surface_material_override/0`, the key \
         `render_server_3d::surface_materials_for_node` consults. Got Nil, so \
         either nothing dispatched or the write went elsewhere."
    );
    assert_eq!(
        moved, source,
        "surface 0 must now hold exactly the material read back from surface 1"
    );

    // ⚠️ The control. Without it, an implementation that ignored the index and
    // wrote every surface — or one that wrote a single flattened
    // `material_override` — would satisfy both assertions above and still be
    // wrong for the multi-material model this API exists to serve.
    assert!(
        matches!(
            node_prop(&tree, mesh, "surface_material_override/2"),
            Variant::Nil
        ),
        "only the index the script named may be written; surface 2 was never \
         touched and must stay unset"
    );
}
