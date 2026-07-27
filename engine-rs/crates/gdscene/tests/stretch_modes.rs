//! pat-0q2kx.179 — `TextureRect.StretchMode` must match Godot's constants.
//!
//! The table was **wrong**, not merely incomplete: `Keep => 1`,
//! `KeepAspect => 5`, `KeepAspectCentered => 6`, with no `Tile`, `KeepCentered`
//! or `KeepAspectCovered` at all. Godot's order (`scene/gui/texture_rect.h`) is
//! SCALE=0, TILE=1, KEEP=2, KEEP_CENTERED=3, KEEP_ASPECT=4,
//! KEEP_ASPECT_CENTERED=5, KEEP_ASPECT_COVERED=6.
//!
//! So **every non-zero `stretch_mode` in a `.tscn` was misread**: `1` (TILE)
//! became `Keep`, `4` (KEEP_ASPECT) fell through to `Scale`, `5` became
//! `KeepAspect`, `6` became `KeepAspectCentered`. A wrong constant table is worse
//! than a missing feature — it renders the wrong thing rather than nothing, and
//! nothing errors.
//!
//! These live in an integration test because `control.rs`'s `#[cfg(test)]`
//! module carries a **pre-existing** duplicate `#[test]` attribute that prevents
//! `-p gdscene --lib` from compiling (reported under pat-0q2kx.175).

use gdscene::control::{get_stretch_mode, set_stretch_mode, StretchMode};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// Godot's seven constants, in declaration order.
const GODOT_ORDER: [(i64, StretchMode); 7] = [
    (0, StretchMode::Scale),
    (1, StretchMode::Tile),
    (2, StretchMode::Keep),
    (3, StretchMode::KeepCentered),
    (4, StretchMode::KeepAspect),
    (5, StretchMode::KeepAspectCentered),
    (6, StretchMode::KeepAspectCovered),
];

/// Every constant maps both ways. This is the regression guard for the wrong
/// table: under the old mapping `1` yielded `Keep` and `4` yielded `Scale`.
#[test]
fn stretch_modes_match_godots_constants_both_ways() {
    for (int, mode) in GODOT_ORDER {
        assert_eq!(
            StretchMode::from_int(int),
            mode,
            "Godot constant {int} must read as {mode:?}"
        );
        assert_eq!(mode.to_int(), int, "{mode:?} must write back as {int}");
    }
}

/// The specific values the old table got wrong, called out so a regression is
/// unambiguous rather than hidden in a loop.
#[test]
fn the_previously_wrong_constants_are_correct() {
    // 1 was read as Keep; it is TILE.
    assert_eq!(StretchMode::from_int(1), StretchMode::Tile);
    // Keep was written as 1; it is 2.
    assert_eq!(StretchMode::Keep.to_int(), 2);
    // 4 fell through to Scale entirely — the mode did not exist.
    assert_eq!(StretchMode::from_int(4), StretchMode::KeepAspect);
    // KeepAspect was written as 5; it is 4.
    assert_eq!(StretchMode::KeepAspect.to_int(), 4);
    // 6 was read as KeepAspectCentered; it is KEEP_ASPECT_COVERED.
    assert_eq!(StretchMode::from_int(6), StretchMode::KeepAspectCovered);
}

/// Out-of-range values fall back to Godot's documented default rather than
/// panicking or wrapping onto a real mode.
#[test]
fn out_of_range_falls_back_to_scale() {
    assert_eq!(StretchMode::from_int(7), StretchMode::Scale);
    assert_eq!(StretchMode::from_int(-1), StretchMode::Scale);
    assert_eq!(StretchMode::from_int(9999), StretchMode::Scale);
    assert_eq!(StretchMode::default(), StretchMode::Scale);
}

/// The accessors round-trip through the node property, so a `.tscn` value and
/// the resolved mode agree.
#[test]
fn stretch_mode_round_trips_through_the_node_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let rect = tree
        .add_child(root, Node::new("Tex", "TextureRect"))
        .unwrap();

    // Unset reads as the default.
    assert_eq!(get_stretch_mode(&tree, rect), StretchMode::Scale);

    for (int, mode) in GODOT_ORDER {
        set_stretch_mode(&mut tree, rect, mode);
        assert_eq!(
            get_stretch_mode(&tree, rect),
            mode,
            "{mode:?} must survive a set/get round trip"
        );
        // And the stored integer is Godot's, so a saved `.tscn` is portable.
        assert_eq!(
            tree.get_node(rect).unwrap().get_property("stretch_mode"),
            Variant::Int(int),
            "{mode:?} must persist as Godot's constant {int}"
        );
    }
}
