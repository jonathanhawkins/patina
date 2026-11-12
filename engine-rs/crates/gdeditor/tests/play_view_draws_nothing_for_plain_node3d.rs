//! pat-mgixk.51 acceptance: the Play view draws nothing for non-visual Node3Ds.
//!
//! `collect_scene_content` includes every node that `is_class("Node3D")`, and
//! `primitive_for_class` maps everything that is not a Camera or a Light to
//! `PrimitiveShape::Box`, drawn as a grey `Mesh3D::cube(0.4)`. `strip_editor_only_items`
//! dropped only Camera, Light and Collision items, so a *running game* showed a cube
//! floating at every plain `Node3D`, `Marker3D`, `Area3D`, `Skeleton3D`,
//! `AudioStreamPlayer3D` and physics body in the scene.
//!
//! Godot draws none of them: not one is a `VisualInstance3D`
//! (`scene/3d/node_3d.cpp`), so a running game shows nothing at those positions.
//!
//! ⚠️ This test is written as a BEFORE/AFTER on one list rather than as an absolute
//! count, because "no cubes" is trivially true of an empty render list. Each case
//! asserts the editor list HAS the item first, so a later zero cannot pass by the
//! fixture never having produced one.

use gdeditor::viewport_3d_render::{
    editor_3d_render_inputs, strip_editor_only_items, viewport_for_scene_camera, PrimitiveShape,
    Render3DItem, Render3DKind,
};
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const W: u32 = 96;
const H: u32 = 72;

/// Every class here is a `Node3D` with NO `VisualInstance3D` in its ancestry, so
/// Godot draws nothing for any of them.
///
/// Spread across the families on purpose — a pivot, a marker, a trigger volume, a
/// physics body, a skeleton and an audio emitter — because they reach the render
/// list through the same `is_class("Node3D")` filter and a fix that only special-cased
/// one name would leave the rest drawing cubes.
const NON_VISUAL: &[&str] = &[
    "Node3D",
    "Marker3D",
    "Area3D",
    "CharacterBody3D",
    "Skeleton3D",
    "AudioStreamPlayer3D",
];

struct Fixture {
    tree: SceneTree,
    camera: NodeId,
    mesh: NodeId,
}

/// The fixture as a Godot scene: a camera, one real mesh, and one node of every
/// non-visual class above.
///
/// 🔑 A `.tscn` rather than `Node::new` + `add_child`, and that is a requirement
/// rather than a preference. This bead carries a `Wiring requirement:` line, so
/// `apps/orchestrator/crate/src/wiring.rs` greps each demanded test for an
/// `ENTRY_POINT_MARKERS` string **in code, not comments** and rejects it BEFORE
/// anything is built — which leaves no `verifier.log` entry and a reopen with an
/// empty body, so the rejection is easy to mistake for a flaky verify. An
/// assembled tree carries no marker and is rejected every time, however green it
/// is locally.
///
/// ⚠️ The root is `type="Node"`, NOT `Node3D`, and the count depends on it. The
/// assembled version parented everything to `tree.root_id()` — the SceneTree's
/// `Window`, which `collect_scene_content`'s `is_class("Node3D")` filter
/// excludes. A `Node3D` root passes that filter and earns its own
/// `Primitive { Box }`, which would make `before.len()` 7 against
/// `NON_VISUAL.len()` of 6 and fail the BEFORE assertion for a reason unrelated
/// to this bead. `Node3D` itself is still covered — it is one of the six
/// children.
fn scene_source() -> String {
    let markers: String = NON_VISUAL
        .iter()
        .map(|class| format!("\n[node name=\"{class}\" type=\"{class}\" parent=\".\"]\n"))
        .collect();
    format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="RealMesh" type="MeshInstance3D" parent="."]
mesh = "primitive:BoxMesh"

[node name="Cam" type="Camera3D" parent="."]
current = true
{markers}"#
    )
}

fn fixture() -> Fixture {
    // The render list filters on `is_class("Node3D")`, which reads the ClassDB —
    // without this the 3D classes are unknown, the list comes back empty and every
    // assertion below passes vacuously.
    gdobject::class_db::register_3d_classes();
    // 🔑 AudioStreamPlayer3D is NOT in `register_3d_classes` — it is registered by
    // `register_audio_classes` (`class_db.rs:25596`), even though it declares
    // `.parent("Node3D")` and is one of the six classes `NON_VISUAL` names.
    // Without this call it is simply an UNKNOWN class, `is_class("Node3D")` answers
    // false, it never reaches the render list, and the BEFORE assertion below counts
    // 5 against the 6 it expects. That is not a hypothetical: it is what the verifier
    // reported (`got 5 ([14, 15, 16, 17, 18])`), and since the ids are consecutive
    // with no gap the absentee is the LAST node in scene order — this one.
    //
    // ⚠️ Registering by SUBSYSTEM rather than by inheritance is the trap: a class
    // can declare a 3D parent and still be absent from the 3D registrar, so
    // "it is a Node3D" and "the 3D registration knows it" are different questions.
    gdobject::class_db::register_audio_classes();

    let packed = PackedScene::from_tscn(&scene_source()).expect("fixture .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).expect("scene instances");

    // Real geometry, so there is something a game frame MUST keep. Without it this
    // test could be satisfied by a change that strips the whole render list.
    let mesh = tree
        .get_node_or_null(scene_root, "RealMesh")
        .expect("fixture has the MeshInstance3D");
    let camera = tree
        .get_node_or_null(scene_root, "Cam")
        .expect("fixture has the Camera3D");

    Fixture { tree, camera, mesh }
}

/// Items that are a box placeholder — the grey cube a non-visual node drew.
fn box_markers(items: &[Render3DItem]) -> Vec<u64> {
    items
        .iter()
        .filter(|item| {
            matches!(
                item.kind,
                Render3DKind::Primitive {
                    shape: PrimitiveShape::Box,
                }
            )
        })
        .map(|item| item.node_id)
        .collect()
}

/// **The acceptance test (pat-mgixk.51 part 1).**
#[test]
fn play_view_draws_nothing_for_plain_node3d() {
    let f = fixture();
    let viewport = viewport_for_scene_camera(&f.tree, f.camera, W, H);
    let mut inputs = editor_3d_render_inputs(&viewport, &f.tree);

    // -- BEFORE: the editor list carries a cube for each non-visual node ------
    //
    // Asserted first and by COUNT, so the "none left" assertion below cannot pass
    // because the fixture never produced any.
    let before = box_markers(&inputs.items);
    assert_eq!(
        before.len(),
        NON_VISUAL.len(),
        "the editor viewport should mark each of the {} non-visual nodes with a box \
         placeholder; got {} ({:?}). If this is short, the fixture is not producing \
         the markers this test exists to remove",
        NON_VISUAL.len(),
        before.len(),
        before
    );

    // -- AFTER: a game frame has none of them --------------------------------
    strip_editor_only_items(&mut inputs);
    let after = box_markers(&inputs.items);
    assert!(
        after.is_empty(),
        "a running game must draw nothing for a non-visual Node3D — none of them is a \
         VisualInstance3D, so Godot submits no geometry. {} box placeholders survived \
         into the game list (node ids {:?})",
        after.len(),
        after
    );

    // …and every one of those nodes is gone from the list entirely, not merely
    // reshaped into some other kind that still draws.
    for class in NON_VISUAL {
        let id = f
            .tree
            .all_nodes_in_tree_order()
            .into_iter()
            .find(|id| {
                f.tree
                    .get_node(*id)
                    .is_some_and(|n| n.class_name() == *class)
            })
            .unwrap_or_else(|| panic!("the fixture has a {class}"));
        assert!(
            !inputs.items.iter().any(|item| item.node_id == id.raw()),
            "{class} still has a render item in the game list"
        );
    }

    // -- and the real mesh SURVIVES ------------------------------------------
    //
    // This is the half that stops the fix being "strip everything". The editor
    // and the game both have to keep actual geometry.
    assert!(
        inputs.items.iter().any(|item| item.node_id == f.mesh.raw()),
        "the MeshInstance3D with a real mesh must stay in the game list — the bead \
         drops editor furniture, not scene content"
    );
}
