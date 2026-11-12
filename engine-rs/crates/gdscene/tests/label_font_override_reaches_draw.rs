//! pat-0q2kx.146 — a Control's `theme_override_fonts/font` reaches the draw
//! stream.
//!
//! Godot resolves a Control's font as local `theme_override_fonts/font` first,
//! then the theme item, then the default — the same precedence
//! `theme_override_colors/font_color` already follows here. Patina resolved the
//! colour override and dropped the font one on the floor: the override was
//! inspector-visible and ignored, because `DrawCommand` carries no font field
//! and `CanvasItem` carries no node id, so nothing connected the node that has
//! the override to the draw item that needs it.
//!
//! The walk now records the override per `CanvasItemId` — the one point where
//! both the node and the item id are in hand — and the flattened
//! `ControlDrawItem::Text` carries it through.
//!
//! ⚠️ This pins the *plumbing*. The pixel-diff clause of the bead needs a second
//! font face in the repo; every `.ttf` here today is byte-identical Cantarell
//! (`shasum` of `gdrender2d/assets/default_font.ttf`,
//! `gdrender2d/test_assets/Cantarell-Regular.ttf` and
//! `gdresource/test_assets/Cantarell-Regular.ttf` all match), so a face
//! comparison written today would assert nothing.

use gdcore::math::Vector2;
use gdscene::control_render::{control_draw_items, ControlDrawItem};
use gdscene::node::Node;
use gdscene::theme::Theme;
use gdscene::SceneTree;
use gdvariant::Variant;

const FONT: &str = "res://fonts/custom.ttf";

/// Builds `Root -> Label`, optionally with a font override, and returns the
/// flattened draw stream.
fn draw_items(font_override: Option<(&str, &str)>) -> Vec<ControlDrawItem> {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Control"))
        .unwrap();
    let label = tree.add_child(root, Node::new("Label", "Label")).unwrap();
    {
        let node = tree.get_node_mut(label).unwrap();
        node.set_property("text", Variant::String("Patina".to_string()));
        if let Some((key, value)) = font_override {
            node.set_property(key, Variant::String(value.to_string()));
        }
    }
    let theme = Theme::default();
    control_draw_items(&mut tree, root, &theme, Vector2::new(200.0, 60.0))
}

fn text_font_path(items: &[ControlDrawItem]) -> Option<Option<String>> {
    items.iter().find_map(|i| match i {
        ControlDrawItem::Text {
            text, font_path, ..
        } if text == "Patina" => Some(font_path.clone()),
        _ => None,
    })
}

/// **The clause under test.** An authored `theme_override_fonts/font` arrives at
/// the draw item; without one the item asks for the default face.
#[test]
fn label_font_override_reaches_draw() {
    // Control first: no override → no font request, so unrelated text keeps the
    // built-in face and nothing else in the editor changes appearance.
    let plain = draw_items(None);
    assert_eq!(
        text_font_path(&plain),
        Some(None),
        "a Label with no override must emit a Text item asking for no specific \
         font: {plain:?}"
    );

    // With the override the path reaches the draw stream.
    let overridden = draw_items(Some(("theme_override_fonts/font", FONT)));
    assert_eq!(
        text_font_path(&overridden),
        Some(Some(FONT.to_string())),
        "the authored theme_override_fonts/font must reach ControlDrawItem::Text"
    );
}

/// A `.tscn` writes the slot as `ExtResource("id")`; the loader resolves that
/// into an `__extres__<prop>` companion holding the real path. The companion is
/// what the draw path must follow — the raw `ExtResource(...)` token is not a
/// loadable path.
#[test]
fn extresource_companion_is_preferred_over_the_raw_token() {
    let items = draw_items(Some(("__extres__theme_override_fonts/font", FONT)));
    assert_eq!(
        text_font_path(&items),
        Some(Some(FONT.to_string())),
        "the __extres__ companion must supply the resolved path"
    );

    // A node carrying only the unresolved token asks for no font rather than
    // handing the renderer a string it cannot open.
    let raw = draw_items(Some((
        "theme_override_fonts/font",
        "ExtResource(\"1_abc\")",
    )));
    assert_eq!(
        text_font_path(&raw),
        Some(None),
        "an unresolved ExtResource token must not reach the renderer as a path"
    );
}
