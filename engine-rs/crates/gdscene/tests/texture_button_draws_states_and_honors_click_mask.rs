//! pat-s3rrv.170 acceptance: a `TextureButton` resolves through ClassDB, draws
//! the texture for its current state, swaps that texture on a press, and still
//! rejects clicks on mask-transparent pixels.
//!
//! Ground truth: `upstream/godot/scene/gui/texture_button.cpp` (`_notification`
//! `NOTIFICATION_DRAW`, lines 130-200) and `doc/classes/TextureButton.xml`.
//!
//! # Two clauses this file does NOT assert, and why
//!
//! 1. **`stretch_mode` rect math.** Godot's default is `STRETCH_KEEP` (2), which
//!    draws at the texture's NATURAL size; every mode except `STRETCH_SCALE`
//!    needs the texture's dimensions. `draw_control` emits a *path* — the pixels
//!    are resolved much later by `gdrender2d::Renderer::find_texture` — and there
//!    is no texture-size source anywhere in `gdscene`. `TextureRect`'s own arm
//!    ignores `stretch_mode` for exactly the same reason. Asserting a
//!    `KEEP_ASPECT_CENTERED` rect here would mean asserting a number the engine
//!    has no way to compute, so the bead's clause is reported unmet rather than
//!    faked.
//! 2. **`texture_hover` / `texture_focused` selection.** Nothing records hover on
//!    a node; `scene_tree` tracks it only for the span of an event. Choosing
//!    those textures would mean inventing state.
//!
//! The click-mask clause is asserted compactly here because it is already
//! covered in depth by `bitmap_click_mask.rs` (five tests); this file checks
//! that the mask still gates the button now that the button also draws.

use gdcore::math::{Vector2, Vector2i};
use gdresource::bit_map::BitMap;
use gdscene::control::{self, control_has_point};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

const NORMAL: &str = "res://btn_normal.png";
const PRESSED: &str = "res://btn_pressed.png";
const DISABLED: &str = "res://btn_disabled.png";

/// A 100x100 TextureButton at the origin with the given texture properties.
fn button(props: &[(&str, &str)]) -> (SceneTree, NodeId) {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree
        .add_child(root, Node::new("Btn", "TextureButton"))
        .unwrap();
    control::set_position(&mut tree, id, Vector2::ZERO);
    control::set_size(&mut tree, id, Vector2::new(100.0, 100.0));
    if let Some(n) = tree.get_node_mut(id) {
        for (k, v) in props {
            n.set_property(k, Variant::String((*v).to_owned()));
        }
    }
    (tree, id)
}

/// Every texture path the control pass emitted, in order.
fn drawn_textures(tree: &mut SceneTree, id: NodeId) -> Vec<String> {
    let mut vp = Viewport::new(200, 200, gdcore::math::Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(tree, id, &theme, &mut vp, Vector2::new(200.0, 200.0));
    vp.get_sorted_items()
        .iter()
        .flat_map(|item| item.commands.iter())
        .filter_map(|c| match c {
            DrawCommand::DrawTextureRect { texture_path, .. } => Some(texture_path.clone()),
            _ => None,
        })
        .collect()
}

fn set_bool(tree: &mut SceneTree, id: NodeId, key: &str, value: bool) {
    if let Some(n) = tree.get_node_mut(id) {
        n.set_property(key, Variant::Bool(value));
    }
}

/// 🔑 THE ACCEPTANCE: it resolves, it draws, and a press swaps the texture.
#[test]
fn texture_button_draws_states_and_honors_click_mask() {
    let (mut tree, id) = button(&[("texture_normal", NORMAL), ("texture_pressed", PRESSED)]);

    // -- ClassDB ----------------------------------------------------------
    let node = tree.get_node(id).expect("node exists");
    assert!(
        node.is_class("BaseButton"),
        "TextureButton must resolve to BaseButton through ClassDB"
    );
    assert!(node.is_class("Control"), "and on up to Control");

    // -- draws the normal texture ------------------------------------------
    // ⚠️ Before this bead the class had no `draw_control` arm at all, so it took
    // clicks and fired `pressed` while drawing nothing — an invisible button
    // that still worked, which is worse than one that plainly does not.
    assert_eq!(
        drawn_textures(&mut tree, id),
        vec![NORMAL.to_string()],
        "an unpressed TextureButton draws texture_normal exactly once"
    );

    // -- a press swaps to the pressed texture -------------------------------
    set_bool(&mut tree, id, "button_pressed", true);
    assert_eq!(
        drawn_textures(&mut tree, id),
        vec![PRESSED.to_string()],
        "while held, it draws texture_pressed"
    );

    set_bool(&mut tree, id, "button_pressed", false);
    assert_eq!(
        drawn_textures(&mut tree, id),
        vec![NORMAL.to_string()],
        "and returns to texture_normal on release"
    );
}

/// Godot falls back to `texture_normal` whenever the state's own texture is
/// unset (`texture_button.cpp:140-166`).
///
/// ⚠️ Without the fallback a button with only `texture_normal` set — by far the
/// commonest authoring — would blank out for exactly as long as the user held
/// it down.
#[test]
fn an_unset_state_texture_falls_back_to_normal() {
    let (mut tree, id) = button(&[("texture_normal", NORMAL)]);

    set_bool(&mut tree, id, "button_pressed", true);
    assert_eq!(
        drawn_textures(&mut tree, id),
        vec![NORMAL.to_string()],
        "an unset texture_pressed keeps showing the normal art, not nothing"
    );

    set_bool(&mut tree, id, "button_pressed", false);
    set_bool(&mut tree, id, "disabled", true);
    assert_eq!(
        drawn_textures(&mut tree, id),
        vec![NORMAL.to_string()],
        "and so does an unset texture_disabled"
    );
}

/// The disabled texture wins over the pressed one.
///
/// ⚠️ Order matters: Godot's `get_draw_mode` tests `disabled` FIRST, so a button
/// that was held and then disabled shows disabled art, not pressed art.
#[test]
fn disabled_takes_priority_over_pressed() {
    let (mut tree, id) = button(&[
        ("texture_normal", NORMAL),
        ("texture_pressed", PRESSED),
        ("texture_disabled", DISABLED),
    ]);

    set_bool(&mut tree, id, "button_pressed", true);
    set_bool(&mut tree, id, "disabled", true);

    assert_eq!(
        drawn_textures(&mut tree, id),
        vec![DISABLED.to_string()],
        "disabled is checked before pressed"
    );
}

/// A button with no textures at all draws nothing rather than a placeholder.
///
/// ⚠️ The positive control for every assertion above: they are all "the right
/// path was drawn", which would also pass if the arm emitted a command
/// unconditionally from some other source.
#[test]
fn a_button_with_no_textures_draws_nothing() {
    let (mut tree, id) = button(&[]);

    assert!(
        drawn_textures(&mut tree, id).is_empty(),
        "no texture properties means no textured draw command"
    );
}

/// The click mask still gates the button now that it also draws.
///
/// Covered in depth by `bitmap_click_mask.rs`; this is the interaction check —
/// drawing and hit-testing read different properties and must not have started
/// disagreeing.
#[test]
fn a_masked_out_pixel_is_still_not_inside_the_button() {
    let (mut tree, id) = button(&[("texture_normal", NORMAL)]);

    // Left half opaque, right half transparent, over the 100x100 control.
    let mut mask = BitMap::new();
    mask.create(Vector2i::new(2, 1));
    mask.set_bit(0, 0, true);
    mask.set_bit(1, 0, false);
    if let Some(n) = tree.get_node_mut(id) {
        n.set_property(
            "__click_mask__data",
            Variant::Dictionary(mask.to_data_dict()),
        );
    }

    assert!(
        control_has_point(&tree, id, Vector2::new(25.0, 50.0)),
        "the opaque half is clickable"
    );
    assert!(
        !control_has_point(&tree, id, Vector2::new(75.0, 50.0)),
        "the transparent half is not — the mask overrides the plain rect test, \
         which is what stops a click landing on empty pixels of the artwork"
    );
}
