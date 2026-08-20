//! pat-s3rrv.165 — `FoldableContainer` collapses to its title band and a
//! `FoldableGroup` makes folding exclusive.
//!
//! Ground truth: `upstream/godot/scene/gui/foldable_container.cpp`
//! (`set_folded` :66-82, `_update_group` :513, the title rect at :252, and the
//! `gui_input` toggle at :235-236).
//!
//! # What a Patina "foldable group" is
//!
//! Godot compares `Ref<FoldableGroup>` pointers. Patina resources are values
//! with no object identity, so a group is the **identity string** every member
//! names in `foldable_group` — the same arrangement `button_group` uses
//! (pat-s3rrv.161). The assertions below therefore compare the two containers'
//! group keys to each other rather than to a literal, so they do not depend on
//! whether the loader normalises `SubResource("x")` to `SubResource:x`.
//!
//! ⚠️ **The router hop is not covered here yet.** These tests call
//! `toggle_foldable_from_click`, which is the seam `gui_input_stage` will call
//! once `scene_tree.rs` is free — it is under another agent's lease. Everything
//! from that seam inward is pinned; the hop from a raw `push_input` to it is
//! not, and is the one thing still to land on this bead.

use gdcore::id::ObjectId;
use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdscene::control::{
    arrange_container, foldable_group_expanded, foldable_group_members, foldable_group_of,
    foldable_minimum_size, foldable_title_press, foldable_title_rect, get_position, get_size,
    is_foldable_folded, point_in_foldable_title, set_foldable_folded, set_position,
    toggle_foldable_from_click, FOLDABLE_TITLE_HEIGHT,
};
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;
use std::sync::{Arc, Mutex};

/// Two grouped containers, `F1` open and `F2` closed, each holding one child.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Control"]

[node name="F1" type="FoldableContainer" parent="."]
title = "First"
foldable_group = SubResource("Group_1")

[node name="C1" type="Control" parent="F1"]

[node name="F2" type="FoldableContainer" parent="."]
title = "Second"
folded = true
foldable_group = SubResource("Group_1")

[node name="C2" type="Control" parent="F2"]
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

fn loaded(src: &str) -> SceneTree {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

/// Records every `folding_changed(bool)` payload, in order.
fn watch_folding(tree: &mut SceneTree, node: NodeId, tag: u64) -> Arc<Mutex<Vec<bool>>> {
    let log: Arc<Mutex<Vec<bool>>> = Arc::default();
    let sink = log.clone();
    tree.connect_signal(
        node,
        "folding_changed",
        Connection::with_callback(ObjectId::from_raw(tag), "on_folding", move |args| {
            sink.lock()
                .unwrap()
                .push(matches!(args.first(), Some(Variant::Bool(true))));
            Variant::Nil
        }),
    );
    log
}

/// 🔑 THE ACCEPTANCE.
#[test]
fn foldable_container_group_exclusive_folding() {
    let mut tree = loaded(SCENE);
    let f1 = find(&tree, "F1");
    let f2 = find(&tree, "F2");
    let c1 = find(&tree, "C1");
    let c2 = find(&tree, "C2");

    // -- membership --------------------------------------------------------
    assert_eq!(
        foldable_group_of(&tree, f1),
        foldable_group_of(&tree, f2),
        "both containers name the same group resource"
    );
    assert!(
        !foldable_group_of(&tree, f1).is_empty(),
        "and that group must not be the empty (ungrouped) key"
    );
    let group = foldable_group_of(&tree, f1);
    assert_eq!(foldable_group_members(&tree, &group), vec![f1, f2]);
    assert_eq!(
        foldable_group_expanded(&tree, &group),
        Some(f1),
        "F1 starts open, F2 starts folded"
    );

    // -- folded collapses to the title band --------------------------------
    arrange_container(&mut tree, f1, Vector2::new(200.0, 100.0));
    arrange_container(&mut tree, f2, Vector2::new(200.0, 100.0));

    assert_eq!(
        get_position(&tree, c1),
        Vector2::new(0.0, FOLDABLE_TITLE_HEIGHT),
        "an OPEN container's child sits below the title band"
    );
    assert_eq!(
        get_size(&tree, c1),
        Vector2::new(200.0, 100.0 - FOLDABLE_TITLE_HEIGHT),
        "and fills what is left"
    );
    // ⚠️ The FOLDED container's own rect is deliberately NOT asserted here.
    // Godot shrinks a folded container by dropping its MINIMUM size and letting
    // the parent container re-sort it; a FoldableContainer under a plain Control
    // keeps the rect it was given. Asserting the arranged size would pin a
    // shortcut — overwriting the size here throws away the expanded height, so
    // the next unfold re-arranges against the collapsed rect and every child
    // comes back with zero space (see `folding_round_trips`).
    assert_eq!(
        foldable_minimum_size(&tree, f2),
        Vector2::new(0.0, FOLDABLE_TITLE_HEIGHT),
        "a folded container's MINIMUM size covers only the title band"
    );
    assert_eq!(
        foldable_minimum_size(&tree, f1),
        Vector2::ZERO,
        "while an open one imposes no minimum of its own"
    );
    assert_eq!(
        get_size(&tree, c2),
        Vector2::ZERO,
        "and its child is collapsed away"
    );

    // -- clicking F2's title unfolds it and folds F1 -----------------------
    let f1_log = watch_folding(&mut tree, f1, 71);
    let f2_log = watch_folding(&mut tree, f2, 72);

    let (title_pos, title_size) = foldable_title_rect(&tree, f2);
    assert_eq!(title_pos, Vector2::ZERO, "title is at the top by default");
    let inside = Vector2::new(title_size.x * 0.5, title_size.y * 0.5);
    assert!(
        point_in_foldable_title(&tree, f2, inside),
        "the point under test must actually be inside the title rect"
    );
    assert!(
        !point_in_foldable_title(&tree, f2, Vector2::new(inside.x, title_size.y + 1.0)),
        "and a point below the band must NOT be — only the title toggles, or an \
         expanded container's children could never be clicked"
    );

    assert!(
        toggle_foldable_from_click(&mut tree, f2),
        "clicking a folded container's title must open it"
    );

    assert!(!is_foldable_folded(&tree, f2), "F2 is now open");
    assert!(
        is_foldable_folded(&tree, f1),
        "and F1 must have been folded by the group — that exclusivity is the \
         whole point of a FoldableGroup"
    );
    assert_eq!(
        foldable_group_expanded(&tree, &group),
        Some(f2),
        "the group's open container follows the click"
    );

    assert_eq!(
        *f2_log.lock().unwrap(),
        vec![false],
        "folding_changed fires on the clicked container, reporting its new \
         state (false = not folded)"
    );
    // ⚠️ Godot emits `folding_changed` from `fold()`/`unfold()`/`gui_input`, never
    // from `set_folded` itself, so the sibling the group closes stays silent
    // (`foldable_container.cpp:57-63, 235-236, 513`).
    assert!(
        f1_log.lock().unwrap().is_empty(),
        "the container folded BY THE GROUP must not emit — Godot emits from the \
         click path, not from set_folded, so one click fires one signal"
    );
}

/// Re-clicking the only open member is refused, because closing it would leave
/// the group with nothing open and `allow_folding_all` defaults to false.
///
/// ⚠️ Paired with the acceptance on purpose: refusing is the DEFAULT, so a build
/// that never implemented the check would still pass the test above.
#[test]
fn the_last_open_member_cannot_be_folded_shut() {
    let mut tree = loaded(SCENE);
    let f1 = find(&tree, "F1");
    let group = foldable_group_of(&tree, f1);

    assert_eq!(foldable_group_expanded(&tree, &group), Some(f1));

    let log = watch_folding(&mut tree, f1, 73);
    assert!(
        !toggle_foldable_from_click(&mut tree, f1),
        "the toggle must report that nothing changed"
    );
    assert!(
        !is_foldable_folded(&tree, f1),
        "the group's only open container must stay open"
    );
    assert_eq!(
        foldable_group_expanded(&tree, &group),
        Some(f1),
        "so the group is never left with everything shut"
    );

    // ⚠️ Godot reads `folded` back out on the line AFTER `set_folded`, so a
    // refused toggle still emits — with the unchanged value.
    assert_eq!(
        *log.lock().unwrap(),
        vec![false],
        "a refused fold still emits folding_changed, carrying the state that \
         actually holds rather than the one that was asked for"
    );
}

/// `allow_folding_all` lets the group be emptied.
///
/// ⚠️ The flag is read from the `__foldable_group__*` companion the loader
/// flattens from the group's sub-resource body, exactly as
/// `__button_group__allow_unpress` is. That flatten is not written yet, so this
/// test sets the companion directly — which is precisely what the pass will
/// produce.
#[test]
fn allow_folding_all_lets_every_member_close() {
    let mut tree = loaded(SCENE);
    let f1 = find(&tree, "F1");
    let f2 = find(&tree, "F2");
    for id in [f1, f2] {
        if let Some(n) = tree.get_node_mut(id) {
            n.set_property("__foldable_group__allow_folding_all", Variant::Bool(true));
        }
    }

    assert!(
        set_foldable_folded(&mut tree, f1, true),
        "with allow_folding_all the last open container may close"
    );
    assert!(is_foldable_folded(&tree, f1));
    let group = foldable_group_of(&tree, f1);
    assert_eq!(
        foldable_group_expanded(&tree, &group),
        None,
        "and the group is then legitimately empty"
    );
}

/// An ungrouped container folds freely, and never drags anything else with it.
///
/// ⚠️ Without this, an implementation that treated the empty group key as a real
/// group would make every ungrouped FoldableContainer in a scene mutually
/// exclusive with all the others.
#[test]
fn ungrouped_containers_are_independent() {
    let mut tree = loaded(
        &SCENE
            .replace("foldable_group = SubResource(\"Group_1\")\n", "")
            .replace("folded = true\n", ""),
    );
    let f1 = find(&tree, "F1");
    let f2 = find(&tree, "F2");

    assert!(foldable_group_of(&tree, f1).is_empty());
    assert!(
        foldable_group_members(&tree, "").is_empty(),
        "the empty key must match nothing"
    );

    assert!(toggle_foldable_from_click(&mut tree, f1));
    assert!(
        is_foldable_folded(&tree, f1),
        "an ungrouped container closes"
    );
    assert!(
        !is_foldable_folded(&tree, f2),
        "without a group its neighbour is untouched"
    );
    assert!(
        toggle_foldable_from_click(&mut tree, f1),
        "and it reopens — nothing refuses an ungrouped fold"
    );
    assert!(!is_foldable_folded(&tree, f1));
}

/// `title_position = POSITION_BOTTOM` moves the band to the bottom edge and the
/// content to the top.
#[test]
fn a_bottom_title_swaps_the_bands() {
    let mut tree = loaded(&SCENE.replace("title = \"First\"", "title_position = 1"));
    let f1 = find(&tree, "F1");
    let c1 = find(&tree, "C1");

    arrange_container(&mut tree, f1, Vector2::new(200.0, 100.0));

    let (pos, size) = foldable_title_rect(&tree, f1);
    assert_eq!(
        pos,
        Vector2::new(0.0, 100.0 - FOLDABLE_TITLE_HEIGHT),
        "the title band sits flush with the bottom edge"
    );
    assert_eq!(size, Vector2::new(200.0, FOLDABLE_TITLE_HEIGHT));
    assert_eq!(
        get_position(&tree, c1),
        Vector2::ZERO,
        "and the content starts at the top instead of below the band"
    );
    assert_eq!(
        get_size(&tree, c1),
        Vector2::new(200.0, 100.0 - FOLDABLE_TITLE_HEIGHT)
    );
}

// ---------------------------------------------------------------------------
// The router entry point
// ---------------------------------------------------------------------------

/// `foldable_title_press` is the whole of the input behaviour, so the scene-tree
/// router only has to call it for every press it already resolves.
///
/// ⚠️ `F2` is deliberately placed at y = 200 rather than left at the origin. The
/// entry point converts the router's GLOBAL point into the container's own space
/// before testing the title rect; with the container at (0, 0) that subtraction
/// is the identity and a build that forgot it entirely would still pass.
#[test]
fn a_press_on_the_title_bar_toggles_through_the_router_entry_point() {
    let mut tree = loaded(SCENE);
    let f2 = find(&tree, "F2");
    set_position(&mut tree, f2, Vector2::new(0.0, 200.0));
    arrange_container(&mut tree, f2, Vector2::new(200.0, 100.0));
    assert!(is_foldable_folded(&tree, f2), "F2 starts folded");

    // Global (10, 210) is local (10, 10) — inside the title band.
    assert!(
        foldable_title_press(&mut tree, f2, Vector2::new(10.0, 210.0)),
        "a press inside the title bar must toggle the fold"
    );
    assert!(!is_foldable_folded(&tree, f2), "F2 is now open");

    // Global (10, 190) is local (10, -10) — above the container entirely.
    assert!(
        !foldable_title_press(&mut tree, f2, Vector2::new(10.0, 190.0)),
        "a press outside the container must not toggle anything — if this fires, \
         the global point was tested against the title rect without first \
         subtracting the container's own origin"
    );
    assert!(!is_foldable_folded(&tree, f2), "and F2 is unchanged");
}

/// The class gate is not optional.
///
/// ⚠️ `point_in_foldable_title` answers for ANY node that has a size, so without
/// the `is_foldable_container` check the router would toggle something whenever
/// a press landed in the top 32 pixels of any Control in the scene.
#[test]
fn a_press_on_a_plain_control_is_ignored() {
    let mut tree = loaded(SCENE);
    let f1 = find(&tree, "F1");
    let c1 = find(&tree, "C1");
    arrange_container(&mut tree, f1, Vector2::new(200.0, 100.0));

    let before = is_foldable_folded(&tree, f1);
    assert!(
        !foldable_title_press(&mut tree, c1, Vector2::new(10.0, 10.0)),
        "a plain Control is not a FoldableContainer and must never toggle"
    );
    assert_eq!(
        is_foldable_folded(&tree, f1),
        before,
        "and nothing else moved either"
    );
}

/// 🔑 Folding and unfolding returns the children to their places.
///
/// ⚠️ This is the test that catches the two mistakes that make folding look
/// correct while being broken:
///
/// 1. **No re-sort.** Godot calls `queue_sort()` inside `set_folded`. Without
///    an equivalent, a click flips the flag and nothing re-arranges, so an
///    unfolded container keeps its children collapsed until some unrelated
///    event triggers layout. Asserting the fold STATE after a click passes
///    happily while the container is visibly empty.
/// 2. **Shrinking the container itself while folded.** That throws away the
///    expanded height, so the unfold re-arranges against a 32px rect and hands
///    every child zero space — the container reopens empty.
#[test]
fn folding_round_trips() {
    let mut tree = loaded(SCENE);
    let f1 = find(&tree, "F1");
    let c1 = find(&tree, "C1");
    arrange_container(&mut tree, f1, Vector2::new(200.0, 100.0));

    let open_pos = get_position(&tree, c1);
    let open_size = get_size(&tree, c1);
    assert_eq!(
        open_size,
        Vector2::new(200.0, 100.0 - FOLDABLE_TITLE_HEIGHT)
    );

    // Fold it. `allow_folding_all` is off, so go through set_foldable_folded on
    // the sibling first to leave F1 free to close.
    // ⚠️ Hoisted out of the call. `set_foldable_folded(&mut tree, find(&tree, ..))`
    // is E0502: the `&mut tree` argument is taken first, so the `&tree` inside
    // the second argument is an immutable borrow while a mutable one is live.
    let f2 = find(&tree, "F2");
    assert!(set_foldable_folded(&mut tree, f2, false));
    assert!(is_foldable_folded(&tree, f1), "the group folded F1");

    // ⚠️ Materialise the collapse before reopening. Without this the child was
    // never actually zeroed, and the round-trip assertions below would pass on
    // a build that re-sorts nothing at all — the test would prove nothing.
    arrange_container(&mut tree, f1, Vector2::new(200.0, 100.0));
    assert_eq!(
        get_size(&tree, c1),
        Vector2::ZERO,
        "a folded container's child must be collapsed away"
    );

    assert!(
        toggle_foldable_from_click(&mut tree, f1),
        "clicking F1 reopens it, which folds F2 again"
    );
    assert!(!is_foldable_folded(&tree, f1));

    assert_eq!(
        get_position(&tree, c1),
        open_pos,
        "after a fold/unfold round trip the child returns to its place"
    );
    assert_eq!(
        get_size(&tree, c1),
        open_size,
        "and to its full size — ZERO here means the unfold never re-sorted, or \
         re-sorted against a container whose expanded height had been thrown away"
    );
}
