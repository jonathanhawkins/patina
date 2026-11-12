//! pat-s3rrv.163: `FlowContainer.last_wrap_alignment` positions the trailing,
//! partly-filled line independently of `alignment`.
//!
//! pat-s3rrv.164 landed the FlowContainer family — ClassDB registration and a
//! two-pass `_resort` port that reads `alignment` and `reverse_fill` — and its
//! acceptance test covers both. It does not touch `last_wrap_alignment`:
//! `grep -c last_wrap_alignment flow_container_family_registered_and_aligned.rs`
//! is **0**. The property is implemented and unverified, which is what this
//! file fixes.
//!
//! Ported behaviour: `upstream/godot/scene/gui/flow_container.cpp` `_resort`,
//! the `switch (alignment)` block.
//!
//! # The two mistakes these numbers are chosen to catch
//!
//! 🔑 **The two enums do not share a numbering.** `AlignmentMode` is
//! `BEGIN=0, CENTER=1, END=2`; `LastWrapAlignmentMode` is
//! `INHERIT=0, BEGIN=1, CENTER=2, END=3` (`flow_container.h:39-49`). So
//! `last_wrap_alignment = 1` means **BEGIN**, not CENTER. An implementation
//! that reused one mapping for both reads `1` as CENTER — and CENTER happens to
//! produce the *same* trailing position as the INHERIT default, so the bug is
//! invisible unless a test pins the BEGIN case specifically. `W_BEGIN` below is
//! that test: it expects **25**, where the off-by-one gives 75.
//!
//! ⚠️ **END does not mean flush-right.** With `alignment = CENTER`, Godot's
//! trailing-line offset is `stretch_avail - prior_stretch_avail * 0.5`, so the
//! last line is measured against the block above it rather than the container
//! edge. Here that is `150 - 50*0.5 = 125`, **not** the 150 a flush-right
//! implementation would produce. `W_END` pins 125.
//!
//! # Why the numbers are exact
//!
//! `get_separation` returns 0 without a `theme_override_constants/separation`,
//! and the fixture is five 100x20 children in a 250-wide container: two per
//! line with 50 left over, wrapping 2+2+1. Only the third line is
//! "not filled", so only it is eligible for `last_wrap_alignment` — the two
//! full lines above must stay at 25 and 125 in every case, which is what proves
//! the property moves the trailing line *and nothing else*.

use gdcore::math::Vector2;
use gdscene::control::{arrange_container, flow_container_line_count, get_position};
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

/// Three `HFlowContainer`s that differ **only** in `last_wrap_alignment`.
///
/// All three set `alignment = 1` (CENTER), so any difference in the trailing
/// child's position is attributable to `last_wrap_alignment` alone.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Control"]

[node name="WInherit" type="HFlowContainer" parent="."]
alignment = 1

[node name="I0" type="Control" parent="WInherit"]
custom_minimum_size = Vector2(100, 20)

[node name="I1" type="Control" parent="WInherit"]
custom_minimum_size = Vector2(100, 20)

[node name="I2" type="Control" parent="WInherit"]
custom_minimum_size = Vector2(100, 20)

[node name="I3" type="Control" parent="WInherit"]
custom_minimum_size = Vector2(100, 20)

[node name="I4" type="Control" parent="WInherit"]
custom_minimum_size = Vector2(100, 20)

[node name="WBegin" type="HFlowContainer" parent="."]
alignment = 1
last_wrap_alignment = 1

[node name="B0" type="Control" parent="WBegin"]
custom_minimum_size = Vector2(100, 20)

[node name="B1" type="Control" parent="WBegin"]
custom_minimum_size = Vector2(100, 20)

[node name="B2" type="Control" parent="WBegin"]
custom_minimum_size = Vector2(100, 20)

[node name="B3" type="Control" parent="WBegin"]
custom_minimum_size = Vector2(100, 20)

[node name="B4" type="Control" parent="WBegin"]
custom_minimum_size = Vector2(100, 20)

[node name="WEnd" type="HFlowContainer" parent="."]
alignment = 1
last_wrap_alignment = 3

[node name="E0" type="Control" parent="WEnd"]
custom_minimum_size = Vector2(100, 20)

[node name="E1" type="Control" parent="WEnd"]
custom_minimum_size = Vector2(100, 20)

[node name="E2" type="Control" parent="WEnd"]
custom_minimum_size = Vector2(100, 20)

[node name="E3" type="Control" parent="WEnd"]
custom_minimum_size = Vector2(100, 20)

[node name="E4" type="Control" parent="WEnd"]
custom_minimum_size = Vector2(100, 20)
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

/// Lays out `container` at the fixture size and returns the x of each child.
fn lay_out(tree: &mut SceneTree, container: &str, names: [&str; 5]) -> Vec<f32> {
    let id = find(tree, container);
    arrange_container(tree, id, Vector2::new(250.0, 200.0));
    assert_eq!(
        flow_container_line_count(tree, id),
        3,
        "{container}: five 100-wide children in 250 wrap into 3 lines (2+2+1)"
    );
    // Reborrowed immutably so the closure cannot be read as holding the `&mut`.
    let t: &SceneTree = tree;
    names.iter().map(|n| pos(t, n).x).collect()
}

/// **The acceptance.** `last_wrap_alignment` moves the trailing line, and only
/// the trailing line.
#[test]
fn last_wrap_alignment_positions_only_the_trailing_line() {
    let mut tree = loaded();

    let inherit = lay_out(&mut tree, "WInherit", ["I0", "I1", "I2", "I3", "I4"]);
    let begin = lay_out(&mut tree, "WBegin", ["B0", "B1", "B2", "B3", "B4"]);
    let end = lay_out(&mut tree, "WEnd", ["E0", "E1", "E2", "E3", "E4"]);

    // The two FULL lines are identical in all three containers: 50 left over,
    // centred, so they start at 25 and the second child follows at 125.
    for (label, xs) in [("INHERIT", &inherit), ("BEGIN", &begin), ("END", &end)] {
        // Tuple rather than a slice-vs-array comparison, whose PartialEq
        // impl is easy to get subtly wrong and impossible to check here.
        assert_eq!(
            (xs[0], xs[1], xs[2], xs[3]),
            (25.0, 125.0, 25.0, 125.0),
            "{label}: last_wrap_alignment must not disturb the FULL lines — \
             they stay centred by `alignment`"
        );
    }

    // INHERIT (0) falls through to `alignment`, so the trailing line is centred
    // in its own leftover: 150/2.
    assert_eq!(
        inherit[4], 75.0,
        "INHERIT must inherit CENTER: (250 - 100) / 2"
    );

    // 🔑 BEGIN is **1**, not 2. An implementation that reused AlignmentMode's
    // numbering reads 1 as CENTER and produces 75 here — identical to INHERIT
    // above, which is exactly why that bug survives without this assertion.
    assert_eq!(
        begin[4], 25.0,
        "last_wrap_alignment = 1 is BEGIN, giving prior/2 = 25; 75 would mean \
         it was read as CENTER"
    );

    // ⚠️ END is measured against the line above, not the container edge:
    // stretch_avail - prior/2 = 150 - 25.
    assert_eq!(
        end[4], 125.0,
        "last_wrap_alignment = 3 is END, giving 150 - 50/2 = 125; 150 would \
         mean it was implemented as flush-right"
    );

    // The three differ from each other — without this the assertions above
    // could all pass for an implementation that ignored the property and
    // happened to agree on one value.
    assert!(
        inherit[4] != begin[4] && begin[4] != end[4] && inherit[4] != end[4],
        "the three modes must produce three distinct trailing positions, got \
         {inherit:?} {begin:?} {end:?}"
    );
}

/// The trailing line is only eligible when it is genuinely the *wrapped*
/// remainder.
///
/// ⚠️ Godot gates every `last_wrap_alignment` arm behind
/// `current_line_idx != 0 && !line_data.is_filled`, so a container whose
/// children all fit on ONE line ignores the property entirely — that single
/// line is the first, and `alignment` governs it. Without this, an
/// implementation that applied the trailing offset to every line would still
/// pass the test above.
#[test]
fn a_single_line_ignores_last_wrap_alignment() {
    let mut tree = loaded();
    let id = find(&tree, "WEnd");

    // Wide enough for all five children (500 >= 5 * 100), so nothing wraps.
    arrange_container(&mut tree, id, Vector2::new(500.0, 200.0));
    assert_eq!(
        flow_container_line_count(&tree, id),
        1,
        "500 wide fits all five 100-wide children on one line"
    );

    // One line, so `alignment = CENTER` alone applies: 500 - 500 = 0 leftover.
    assert_eq!(
        pos(&tree, "E0").x,
        0.0,
        "a lone line is governed by `alignment`, not `last_wrap_alignment`"
    );
    assert_eq!(pos(&tree, "E4").x, 400.0);
}
