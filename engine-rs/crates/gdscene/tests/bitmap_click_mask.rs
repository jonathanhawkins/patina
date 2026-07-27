//! pat-0q2kx.257 — `BitMap` as a resource, and `TextureButton.texture_click_mask`
//! rejecting clicks over transparent pixels.
//!
//! Godot ground truth: `doc/classes/BitMap.xml` over
//! `upstream/godot/scene/resources/bit_map.cpp`, and
//! `scene/gui/texture_button.cpp:62` (`TextureButton::has_point`) for the mask
//! half.
//!
//! The interesting property is not "the button ignores the click" but **where the
//! click goes instead**: Godot's `has_point` returning false makes the button
//! transparent to the hit test, so the control *behind* it receives the event.
//! A mask implementation that merely suppressed the button's own `pressed` signal
//! would pass a weaker test and still be wrong, so the assertions below check the
//! node the hit test actually returns.
//!
//! ⚠️ The acceptance function is `bitmap_resource_and_click_mask_test` while the
//! file is `bitmap_click_mask`: the verifier truncates a reported test command at
//! 58 characters, and `--test bitmap_resource_and_click_mask_test` overruns it.
//! The acceptance name is a function name, not a binary name.

use gdcore::math::{Color, Vector2, Vector2i};
use gdresource::bit_map::{BitMap, BitMapRect};
use gdresource::image::{Image, ImageFormat};
use gdscene::control::{control_at_point, control_has_point, texture_button_click_mask};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// An 8x8 image whose left half is opaque and right half fully transparent —
/// the shape every assertion below is calibrated against.
fn half_opaque_image() -> Image {
    let mut img = Image::create(8, 8, ImageFormat::Rgba8);
    for y in 0..8 {
        for x in 0..8 {
            let a = if x < 4 { 1.0 } else { 0.0 };
            img.set_pixel(x, y, Color::new(1.0, 1.0, 1.0, a));
        }
    }
    img
}

/// The same shape as a BitMap, built through `create_from_image_alpha`.
fn half_opaque_mask() -> BitMap {
    let mut b = BitMap::new();
    b.create_from_image_alpha(&half_opaque_image(), 0.1);
    b
}

/// A tree holding a `Panel` and, on top of it, a `TextureButton` carrying
/// `mask_prop` as its click mask. Both cover the same `size` rect at the origin,
/// so anything the button declines falls through to the panel.
///
/// The button is added *second* because `hit_test_control` visits children in
/// reverse order — later children are drawn on top, so this is what puts the
/// button in front.
fn scene_with_mask(size: Vector2, mask_prop: (&str, Variant)) -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let panel = tree
        .add_child(root, Node::new("Behind", "Panel"))
        .expect("add panel");
    tree.get_node_mut(panel)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::ZERO));
    tree.get_node_mut(panel)
        .unwrap()
        .set_property("size", Variant::Vector2(size));

    let button = tree
        .add_child(root, Node::new("Masked", "TextureButton"))
        .expect("add button");
    tree.get_node_mut(button)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::ZERO));
    tree.get_node_mut(button)
        .unwrap()
        .set_property("size", Variant::Vector2(size));
    tree.get_node_mut(button)
        .unwrap()
        .set_property(mask_prop.0, mask_prop.1);

    (tree, button, panel)
}

/// **The acceptance test.** BitMap instantiates, `create_from_image_alpha` sets
/// bits, and a masked TextureButton ignores clicks over its transparent half.
#[test]
fn bitmap_resource_and_click_mask_test() {
    // -- BitMap is a registered Resource -------------------------------------
    gdobject::class_db::register_io_classes();
    assert!(
        gdobject::class_db::class_exists("BitMap"),
        "BitMap must be registered in ClassDB"
    );
    assert!(
        gdobject::class_db::is_parent_class("BitMap", "Resource"),
        "BitMap inherits Resource; a severed chain here means the Resource \
         bootstrap in register_io_classes is missing"
    );
    assert!(
        gdobject::class_db::instantiate("BitMap").is_some(),
        "BitMap must instantiate — it is a concrete Resource, not abstract"
    );
    for method in [
        "create",
        "create_from_image_alpha",
        "get_bit",
        "set_bit",
        "get_true_bit_count",
        "grow_mask",
        "opaque_to_polygons",
    ] {
        assert!(
            gdobject::class_db::class_has_method("BitMap", method),
            "BitMap.{method}() must be registered"
        );
    }

    // -- create_from_image_alpha sets bits from the image's alpha -------------
    let mask = half_opaque_mask();
    assert_eq!(
        mask.get_size(),
        Vector2i::new(8, 8),
        "the mask takes the image's dimensions"
    );
    assert_eq!(
        mask.get_true_bit_count(),
        32,
        "half of 64 pixels are opaque, so exactly half the bits are set"
    );
    assert!(
        mask.get_bit(0, 0) && mask.get_bit(3, 7),
        "the opaque left half must be set"
    );
    assert!(
        !mask.get_bit(4, 0) && !mask.get_bit(7, 7),
        "the transparent right half must be clear"
    );

    // -- the mask is reachable from the node ---------------------------------
    let size = Vector2::new(8.0, 8.0);
    let (tree, button, panel) = scene_with_mask(
        size,
        (
            "texture_click_mask",
            Variant::Dictionary(mask.to_data_dict()),
        ),
    );
    let resolved = texture_button_click_mask(&tree, button)
        .expect("the click mask must resolve off the node, or the hit test has nothing to sample");
    assert_eq!(resolved, mask, "the mask must round-trip through the node");

    // -- the opaque half is clickable ----------------------------------------
    assert_eq!(
        control_at_point(&tree, Vector2::new(2.0, 2.0)),
        Some(button),
        "a click over an opaque pixel must reach the button"
    );
    assert!(
        control_has_point(&tree, button, Vector2::new(2.0, 2.0)),
        "control_has_point must agree with the tree walk"
    );

    // -- the transparent half is NOT, and the click falls through -------------
    assert!(
        !control_has_point(&tree, button, Vector2::new(6.0, 2.0)),
        "a click over a transparent pixel must not be inside the button"
    );
    assert_eq!(
        control_at_point(&tree, Vector2::new(6.0, 2.0)),
        Some(panel),
        "the rejected click must reach the control BEHIND the button — a mask \
         that only suppressed the button's own press would leave this as \
         Some(button)"
    );

    // -- the control behaves normally with no mask at all --------------------
    let (plain_tree, plain_button, _) = scene_with_mask(
        size,
        // A property the mask resolver ignores, i.e. effectively no mask.
        ("tooltip_text", Variant::String("unmasked".into())),
    );
    assert!(
        texture_button_click_mask(&plain_tree, plain_button).is_none(),
        "no mask property means no mask"
    );
    assert_eq!(
        control_at_point(&plain_tree, Vector2::new(6.0, 2.0)),
        Some(plain_button),
        "without a mask the whole rect is clickable — this is the control that \
         proves the assertions above are the mask's doing and not a broken \
         hit test"
    );
}

/// The point is mapped from the control's rect into the mask's resolution, so a
/// button drawn at twice its mask's size still rejects the right pixels.
///
/// This is the assertion that fails if the mapping is left as identity: at 16x16
/// with an 8x8 mask, `(12, 4)` would sample mask cell `(12, 4)` — out of bounds,
/// which happens to also reject — but `(4, 4)` would sample `(4, 4)`, landing in
/// the transparent half and wrongly rejecting a click over an opaque pixel.
#[test]
fn click_mask_scales_from_the_control_rect() {
    let mask = half_opaque_mask();
    let (tree, button, panel) = scene_with_mask(
        Vector2::new(16.0, 16.0),
        (
            "texture_click_mask",
            Variant::Dictionary(mask.to_data_dict()),
        ),
    );

    // (4, 4) in a 16-wide control -> (2, 2) in an 8-wide mask: opaque.
    assert_eq!(
        control_at_point(&tree, Vector2::new(4.0, 4.0)),
        Some(button),
        "a click in the scaled-up opaque half must still hit the button"
    );
    // (12, 4) -> (6, 2): transparent.
    assert_eq!(
        control_at_point(&tree, Vector2::new(12.0, 4.0)),
        Some(panel),
        "a click in the scaled-up transparent half must fall through"
    );
}

/// A `.tscn`-authored mask arrives as `texture_click_mask = SubResource("id")`,
/// whose bits `packed_scene.rs` copies onto the node as `__click_mask__data`.
/// The `SubResource:<id>` string itself carries no bits, so the companion is the
/// only thing that makes a scene-loaded mask work.
#[test]
fn scene_authored_mask_resolves_from_the_companion() {
    let mask = half_opaque_mask();
    let (tree, button, panel) = scene_with_mask(
        Vector2::new(8.0, 8.0),
        (
            "__click_mask__data",
            Variant::Dictionary(mask.to_data_dict()),
        ),
    );
    // The real property keeps the reference, exactly as the scene round-trip
    // requires — and it alone must not be mistaken for a mask.
    assert_eq!(
        texture_button_click_mask(&tree, button),
        Some(mask),
        "the companion must resolve even though texture_click_mask is a string"
    );
    assert_eq!(
        control_at_point(&tree, Vector2::new(6.0, 2.0)),
        Some(panel),
        "a scene-authored mask must reject transparent clicks too"
    );
}

/// A `SubResource:<id>` string with **no** companion resolves to no mask rather
/// than to an empty one. An empty mask would reject every click and make the
/// button silently unclickable — a far worse failure than ignoring the mask.
#[test]
fn unresolved_subresource_reference_leaves_the_button_clickable() {
    let (tree, button, _) = scene_with_mask(
        Vector2::new(8.0, 8.0),
        (
            "texture_click_mask",
            Variant::String("SubResource:BitMap_xyz".into()),
        ),
    );
    assert!(
        texture_button_click_mask(&tree, button).is_none(),
        "an unresolved reference is not a mask"
    );
    assert_eq!(
        control_at_point(&tree, Vector2::new(6.0, 2.0)),
        Some(button),
        "with no usable mask the button keeps its full rect"
    );
}

/// The mask is a TextureButton-only property. A `Panel` that happens to carry a
/// `texture_click_mask` must keep its plain rect test, or the gate in
/// `texture_button_click_mask` is not doing its job.
#[test]
fn mask_is_ignored_on_non_texture_button_controls() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let panel = tree
        .add_child(root, Node::new("NotAButton", "Panel"))
        .expect("add panel");
    tree.get_node_mut(panel)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::ZERO));
    tree.get_node_mut(panel)
        .unwrap()
        .set_property("size", Variant::Vector2(Vector2::new(8.0, 8.0)));
    tree.get_node_mut(panel).unwrap().set_property(
        "texture_click_mask",
        Variant::Dictionary(half_opaque_mask().to_data_dict()),
    );

    assert!(
        texture_button_click_mask(&tree, panel).is_none(),
        "a Panel is not a TextureButton, so it has no click mask"
    );
    assert_eq!(
        control_at_point(&tree, Vector2::new(6.0, 2.0)),
        Some(panel),
        "the Panel must still be hit over what would be a transparent mask cell"
    );
}

/// `grow_mask` and `opaque_to_polygons` are part of the bead's surface but not of
/// the click path, so they are exercised here against the same fixture rather
/// than only in the unit tests inside `bit_map.rs`.
#[test]
fn grow_mask_and_polygons_operate_on_the_image_derived_mask() {
    let mut mask = half_opaque_mask();
    assert_eq!(mask.get_true_bit_count(), 32);

    // Dilating by 1 pushes the opaque/transparent boundary one column right.
    mask.grow_mask(1, mask.full_rect());
    assert!(
        mask.get_bit(4, 3),
        "dilation must claim the first transparent column"
    );
    assert!(
        !mask.get_bit(5, 3),
        "a 1px dilation must not reach two columns out"
    );

    // The dilated left block is one solid region, so it traces one polygon.
    let polys = mask.opaque_to_polygons(mask.full_rect(), 2.0);
    assert_eq!(
        polys.len(),
        1,
        "one contiguous opaque region traces one polygon, got {}",
        polys.len()
    );
    assert!(
        polys[0].len() >= 3,
        "a polygon needs at least three vertices, got {:?}",
        polys[0]
    );

    // Eroding by the same radius does NOT undo the dilation — a morphological
    // closing is not the identity, and Godot erodes at the rect border too.
    //
    // `grow_mask` with a negative radius counts anything outside the rect as an
    // unset bit (upstream/godot/scene/resources/bit_map.cpp:581-588, "Outside of
    // rectangle counts as bit not set"), so every pixel touching an edge is
    // eroded along with the dilated column. On this 8x8 mask the radius-1
    // neighbourhood is the 4 orthogonal neighbours (diagonals are sqrt(2) > 1),
    // so the survivors are x in 1..=3 and y in 1..=6 — column 0 loses to the
    // x=-1 edge, rows 0 and 7 to the y edges, and column 4 to the transparent
    // x=5 — leaving 3 * 6 = 18.
    mask.grow_mask(-1, mask.full_rect());
    assert_eq!(
        mask.get_true_bit_count(),
        18,
        "erosion also eats the rect border, so it cannot restore the original 32"
    );
    assert!(
        mask.get_bit(1, 1) && mask.get_bit(3, 6),
        "the eroded interior must survive"
    );
    assert!(
        !mask.get_bit(0, 3) && !mask.get_bit(4, 3),
        "the border column and the dilated column must both be gone"
    );
}

/// `set_bit_rect` + `get_bit` over a hand-built mask, driving the click path from
/// bits set directly rather than from an image — the path a script that builds a
/// mask procedurally would take.
#[test]
fn hand_built_mask_drives_the_click_path() {
    let mut mask = BitMap::new();
    mask.create(Vector2i::new(4, 4));
    // Only the bottom-right 2x2 is clickable.
    mask.set_bit_rect(BitMapRect::new(2, 2, 2, 2), true);
    assert_eq!(mask.get_true_bit_count(), 4);

    let (tree, button, panel) = scene_with_mask(
        Vector2::new(4.0, 4.0),
        (
            "texture_click_mask",
            Variant::Dictionary(mask.to_data_dict()),
        ),
    );
    assert_eq!(
        control_at_point(&tree, Vector2::new(3.0, 3.0)),
        Some(button),
        "inside the 2x2 set region"
    );
    assert_eq!(
        control_at_point(&tree, Vector2::new(0.0, 0.0)),
        Some(panel),
        "outside it, the click falls through"
    );
}
