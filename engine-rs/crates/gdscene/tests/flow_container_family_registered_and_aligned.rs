//! pat-s3rrv.164 acceptance: the FlowContainer family resolves through ClassDB
//! and its `alignment` / `reverse_fill` reach the children's rects.
//!
//! Ported behaviour: `upstream/godot/scene/gui/flow_container.cpp` `_resort`.
//! Godot uses two passes — the first groups children into wrapped lines, the
//! second places them with a per-line offset — because a line's alignment
//! offset depends on that line's total length, which is not known until the
//! line is closed. Patina had only the first.
//!
//! # Why the numbers below are exact rather than approximate
//!
//! `get_separation` returns **0** when a container carries no
//! `theme_override_constants/separation`, so every position here is a whole
//! number that can be derived by hand. A fixture with a themed gap would make
//! these assertions depend on a theme default that has nothing to do with flow
//! layout.
//!
//! ⚠️ The children are 100x20 in a 250-wide container, so exactly two fit per
//! line with **50 left over** — deliberately not a multiple of the child width.
//! A leftover of 0 would make `alignment = CENTER` indistinguishable from
//! `BEGIN`, which is the coincidence point this whole test exists to avoid.

use gdcore::math::Vector2;
use gdscene::control::{arrange_container, flow_container_line_count, get_position};
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

/// An `HFlowContainer` that centres each line and a `VFlowContainer` that fills
/// from the far edge, each holding five equal children.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Control"]

[node name="HFlow" type="HFlowContainer" parent="."]
alignment = 1

[node name="A0" type="Control" parent="HFlow"]
custom_minimum_size = Vector2(100, 20)

[node name="A1" type="Control" parent="HFlow"]
custom_minimum_size = Vector2(100, 20)

[node name="A2" type="Control" parent="HFlow"]
custom_minimum_size = Vector2(100, 20)

[node name="A3" type="Control" parent="HFlow"]
custom_minimum_size = Vector2(100, 20)

[node name="A4" type="Control" parent="HFlow"]
custom_minimum_size = Vector2(100, 20)

[node name="VFlow" type="VFlowContainer" parent="."]
reverse_fill = true

[node name="B0" type="Control" parent="VFlow"]
custom_minimum_size = Vector2(20, 100)

[node name="B1" type="Control" parent="VFlow"]
custom_minimum_size = Vector2(20, 100)

[node name="B2" type="Control" parent="VFlow"]
custom_minimum_size = Vector2(20, 100)

[node name="B3" type="Control" parent="VFlow"]
custom_minimum_size = Vector2(20, 100)

[node name="B4" type="Control" parent="VFlow"]
custom_minimum_size = Vector2(20, 100)
"#;

fn find(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == name)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no node named {name}"))
}

fn loaded() -> SceneTree {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

fn pos(tree: &SceneTree, name: &str) -> Vector2 {
    get_position(tree, find(tree, name))
}

/// 🔑 THE ACCEPTANCE.
#[test]
fn flow_container_family_registered_and_aligned() {
    let mut tree = loaded();
    let hflow = find(&tree, "HFlow");
    let vflow = find(&tree, "VFlow");

    // -- ClassDB inheritance ----------------------------------------------
    // Before registration these resolved to nothing, so `is_class("Container")`
    // was false and the layout only ran because `arrange_container` carries a
    // hardcoded list of container class names.
    for (id, class) in [(hflow, "HFlowContainer"), (vflow, "VFlowContainer")] {
        let node = tree.get_node(id).expect("node exists");
        assert!(
            node.is_class("Container"),
            "{class} must resolve to Container through ClassDB"
        );
        assert!(
            node.is_class("Control"),
            "{class} must resolve to Control through ClassDB"
        );
        assert!(
            node.is_class("FlowContainer"),
            "{class} must resolve to its own base, FlowContainer"
        );
    }

    // -- HFlowContainer, alignment = CENTER --------------------------------
    // 250 wide, children 100 wide, separation 0 => two per line, 50 left over,
    // so each full line starts at 50/2 = 25.
    arrange_container(&mut tree, hflow, Vector2::new(250.0, 200.0));

    assert_eq!(
        flow_container_line_count(&tree, hflow),
        3,
        "five 100-wide children in a 250-wide container wrap into 3 lines (2+2+1)"
    );

    assert_eq!(pos(&tree, "A0"), Vector2::new(25.0, 0.0));
    assert_eq!(
        pos(&tree, "A1"),
        Vector2::new(125.0, 0.0),
        "the second child follows the first with no separation"
    );
    assert_eq!(
        pos(&tree, "A2"),
        Vector2::new(25.0, 20.0),
        "the second line starts a row down, and is centred the same way"
    );
    assert_eq!(pos(&tree, "A3"), Vector2::new(125.0, 20.0));
    // The last line holds ONE child, so it has 150 spare and centres at 75.
    // A build that ignored `alignment` would put this at 0; a build that
    // applied the FIRST line's offset to every line would put it at 25.
    assert_eq!(
        pos(&tree, "A4"),
        Vector2::new(75.0, 40.0),
        "the short last line must be centred on its OWN leftover space (150/2), \
         not on the full lines' (50/2)"
    );

    // -- VFlowContainer, reverse_fill = true -------------------------------
    // Columns advance rightward normally; reversed, they stack from the right
    // edge inward. 200 wide, columns 20 wide.
    arrange_container(&mut tree, vflow, Vector2::new(200.0, 250.0));

    assert_eq!(
        flow_container_line_count(&tree, vflow),
        3,
        "five 100-tall children in a 250-tall container wrap into 3 columns"
    );

    assert_eq!(
        pos(&tree, "B0"),
        Vector2::new(180.0, 0.0),
        "the FIRST column must sit at the far edge (200 - 0 - 20) when \
         reverse_fill is set — at 0 it was ignored"
    );
    assert_eq!(pos(&tree, "B1"), Vector2::new(180.0, 100.0));
    assert_eq!(
        pos(&tree, "B2"),
        Vector2::new(160.0, 0.0),
        "and later columns march inward, not outward"
    );
    assert_eq!(pos(&tree, "B3"), Vector2::new(160.0, 100.0));
    assert_eq!(pos(&tree, "B4"), Vector2::new(140.0, 0.0));
}

/// **The control for both halves above.** The same geometry with the default
/// `alignment = BEGIN` and `reverse_fill = false` must pack from the origin.
///
/// ⚠️ Without this, an implementation that centred unconditionally — or mirrored
/// unconditionally — would pass every assertion in the acceptance test. It also
/// pins the pre-existing default behaviour that the two-pass rewrite had to
/// preserve.
#[test]
fn the_defaults_still_pack_from_the_origin() {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(
        &SCENE
            .replace("alignment = 1\n", "")
            .replace("reverse_fill = true\n", ""),
    )
    .expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let hflow = find(&tree, "HFlow");
    let vflow = find(&tree, "VFlow");
    arrange_container(&mut tree, hflow, Vector2::new(250.0, 200.0));
    arrange_container(&mut tree, vflow, Vector2::new(200.0, 250.0));

    assert_eq!(
        pos(&tree, "A0"),
        Vector2::new(0.0, 0.0),
        "BEGIN starts at 0"
    );
    assert_eq!(pos(&tree, "A1"), Vector2::new(100.0, 0.0));
    assert_eq!(pos(&tree, "A2"), Vector2::new(0.0, 20.0));
    assert_eq!(
        pos(&tree, "A4"),
        Vector2::new(0.0, 40.0),
        "the short last line is NOT nudged when alignment is BEGIN"
    );

    assert_eq!(
        pos(&tree, "B0"),
        Vector2::new(0.0, 0.0),
        "without reverse_fill the first column is at the near edge"
    );
    assert_eq!(pos(&tree, "B2"), Vector2::new(20.0, 0.0));
    assert_eq!(pos(&tree, "B4"), Vector2::new(40.0, 0.0));
}

/// `alignment = END` pushes each line to the far end of its own leftover space.
///
/// ⚠️ END and CENTER differ only by a factor of two, so a `* 0.5` dropped from
/// or added to the wrong branch turns one into the other. Asserting both pins
/// which is which.
#[test]
fn alignment_end_pushes_each_line_to_its_own_far_end() {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(&SCENE.replace("alignment = 1", "alignment = 2"))
        .expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let hflow = find(&tree, "HFlow");
    arrange_container(&mut tree, hflow, Vector2::new(250.0, 200.0));

    assert_eq!(
        pos(&tree, "A0"),
        Vector2::new(50.0, 0.0),
        "a full line's 50 spare all goes in front of it"
    );
    assert_eq!(pos(&tree, "A1"), Vector2::new(150.0, 0.0));
    assert_eq!(
        pos(&tree, "A4"),
        Vector2::new(150.0, 40.0),
        "the last line's 150 spare all goes in front of it — 75 here would mean \
         the CENTER branch ran"
    );
}
