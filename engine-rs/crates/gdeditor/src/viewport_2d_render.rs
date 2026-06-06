//! Building the 2D viewport's render list from CanvasItem (Node2D) content.
//!
//! Godot's 2D editor draws the actual visuals of each `CanvasItem` — a
//! `Sprite2D` shows its texture at the node's transform, not an abstract
//! position marker. This module turns the scene's visible 2D nodes into
//! concrete draw items the renderer can mount: a **sprite** (texture + offset +
//! flip) for `Sprite2D` nodes that carry a texture, or a **marker** fallback for
//! every other 2D node (cameras, empties, and shape/text nodes handled by later
//! sub-tasks) so they stay visible and pickable in the editor.
//!
//! It lives in its own module (over the public `viewport_2d` API) to compose
//! with the viewport without growing the already-large `viewport_2d.rs`.

use gdcore::math::{Color, Transform2D, Vector2};
use gdscene::control::TextAlign;
use gdscene::{control, node2d, SceneTree};
use gdvariant::Variant;

use crate::viewport_2d::Viewport2D;

/// The kind of content drawn for a 2D canvas node.
#[derive(Debug, Clone, PartialEq)]
pub enum Render2DKind {
    /// A `Sprite2D` texture to render, with its draw offset and flip flags.
    Sprite {
        /// The texture resource path.
        texture: String,
        /// The sprite's local draw offset (Godot's `offset` property).
        offset: Vector2,
        /// Whether the texture is mirrored horizontally.
        flip_h: bool,
        /// Whether the texture is mirrored vertically.
        flip_v: bool,
    },
    /// A `Polygon2D`'s filled polygon: its vertices (local space) and fill color.
    Polygon {
        /// The polygon vertices, in node-local space.
        points: Vec<Vector2>,
        /// The fill color.
        color: Color,
    },
    /// A `Line2D`'s polyline: its points (local space), stroke width, and color.
    Line {
        /// The polyline points, in node-local space.
        points: Vec<Vector2>,
        /// The stroke width in pixels.
        width: f32,
        /// The stroke color.
        color: Color,
    },
    /// A `ColorRect`'s solid rectangle: its size and fill color.
    ColorRect {
        /// The rectangle size (Control's `size`).
        size: Vector2,
        /// The fill color.
        color: Color,
    },
    /// A `Label`'s text: its string, font size, and horizontal alignment.
    Text {
        /// The label's displayed text.
        text: String,
        /// The font size (theme override, defaulting to Godot's 16).
        font_size: i64,
        /// The horizontal text alignment.
        h_align: TextAlign,
    },
    /// A position marker drawn for a 2D node with no own visual yet (cameras,
    /// empty `Node2D`s, and text nodes that later sub-tasks render) so it stays
    /// visible and pickable in the editor.
    Marker {
        /// The node's class name (drives the marker glyph).
        class: String,
    },
}

/// One drawable item in the 2D viewport's render list.
#[derive(Debug, Clone, PartialEq)]
pub struct Render2DItem {
    /// The source node's raw id.
    pub node_id: u64,
    /// The node's global (canvas-space) transform — the composed parent chain,
    /// so a child is drawn relative to its parent's transform.
    pub global_transform: Transform2D,
    /// The node's `modulate` tint (defaulting to white), applied to its visual.
    pub modulate: Color,
    /// The node's `z_index`; items are returned sorted by it ascending so lower
    /// z draws first (behind) and higher z draws last (in front).
    pub z_index: i64,
    /// What to draw for this node.
    pub kind: Render2DKind,
}

impl Viewport2D {
    /// Builds the 2D viewport's render list from `tree`: each **visible**
    /// `CanvasItem` node (the Node2D and Control families) becomes a
    /// [`Render2DItem`] — a sprite for texture-bearing `Sprite2D` nodes, a
    /// polygon/line/color-rect for the corresponding shape nodes, or a marker
    /// otherwise. Nodes hidden by their own or an ancestor's `visible` flag are
    /// skipped, and items appear in scene-tree order (the back-to-front draw
    /// order).
    pub fn build_render_list(&self, tree: &SceneTree) -> Vec<Render2DItem> {
        let mut items: Vec<Render2DItem> = tree
            .all_nodes_in_tree_order()
            .into_iter()
            .filter_map(|id| {
                let node = tree.get_node(id)?;
                // Only canvas items (Node2D and Control families) participate in
                // the 2D render list; spatial (Node3D) nodes never do.
                if !node.is_class("CanvasItem") {
                    return None;
                }
                // Hidden nodes (own flag or inherited) draw nothing.
                if !node2d::is_visible_in_tree(tree, id) {
                    return None;
                }
                Some(Render2DItem {
                    node_id: id.raw(),
                    global_transform: node2d::get_global_transform(tree, id),
                    modulate: read_color(tree, id, "modulate"),
                    z_index: node2d::get_z_index(tree, id),
                    kind: render_kind_for(tree, id, node.class_name()),
                })
            })
            .collect();
        // Godot draws by ascending z_index; a stable sort keeps scene-tree order
        // as the tiebreaker within the same z.
        items.sort_by_key(|i| i.z_index);
        items
    }
}

/// Chooses the draw kind for a visible canvas node: a sprite for a textured
/// `Sprite2D`, a polygon/line/color-rect for the shape nodes, otherwise a
/// class-tagged marker.
fn render_kind_for(tree: &SceneTree, id: gdscene::NodeId, class: &str) -> Render2DKind {
    match class {
        "Sprite2D" => {
            if let Some(texture) = node2d::get_texture_path(tree, id) {
                return Render2DKind::Sprite {
                    texture,
                    offset: read_vector2(tree, id, "offset"),
                    flip_h: read_bool(tree, id, "flip_h"),
                    flip_v: read_bool(tree, id, "flip_v"),
                };
            }
        }
        "Polygon2D" => {
            return Render2DKind::Polygon {
                points: read_points(tree, id, "polygon"),
                color: read_color(tree, id, "color"),
            };
        }
        "Line2D" => {
            return Render2DKind::Line {
                points: read_points(tree, id, "points"),
                width: read_f32(tree, id, "width", 0.0),
                color: read_color(tree, id, "default_color"),
            };
        }
        "ColorRect" => {
            return Render2DKind::ColorRect {
                size: control::get_size(tree, id),
                color: read_color(tree, id, "color"),
            };
        }
        "Label" => {
            return Render2DKind::Text {
                text: control::get_label_text(tree, id),
                font_size: control::get_font_size(tree, id),
                h_align: control::get_h_align(tree, id),
            };
        }
        _ => {}
    }
    Render2DKind::Marker {
        class: class.to_owned(),
    }
}

/// Reads a `Vector2` property, defaulting to [`Vector2::ZERO`] when absent.
fn read_vector2(tree: &SceneTree, id: gdscene::NodeId, key: &str) -> Vector2 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Vector2(v)) => v,
        _ => Vector2::ZERO,
    }
}

/// Reads a `bool` property, defaulting to `false` when absent.
fn read_bool(tree: &SceneTree, id: gdscene::NodeId, key: &str) -> bool {
    matches!(
        tree.get_node(id).map(|n| n.get_property(key)),
        Some(Variant::Bool(true))
    )
}

/// Reads a `Color` property, defaulting to white (Godot's default tint) when
/// absent.
fn read_color(tree: &SceneTree, id: gdscene::NodeId, key: &str) -> Color {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Color(c)) => c,
        _ => Color::WHITE,
    }
}

/// Reads a packed `Vector2` array property (a polygon/polyline point list),
/// dropping any non-`Vector2` entries and defaulting to empty when absent.
fn read_points(tree: &SceneTree, id: gdscene::NodeId, key: &str) -> Vec<Vector2> {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Array(items)) => items
            .into_iter()
            .filter_map(|v| match v {
                Variant::Vector2(p) => Some(p),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Reads a numeric (`Float`/`Int`) property as `f32`, falling back to `default`
/// when absent or non-numeric.
fn read_f32(tree: &SceneTree, id: gdscene::NodeId, key: &str, default: f32) -> f32 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Float(f)) => f as f32,
        Some(Variant::Int(i)) => i as f32,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_2d::Viewport2D;
    use gdscene::node::Node;
    use gdscene::node3d;

    fn find(items: &[Render2DItem], node_id: u64) -> Option<&Render2DItem> {
        items.iter().find(|i| i.node_id == node_id)
    }

    /// Acceptance (pat-1p53i.1): a `Sprite2D` with a texture renders that
    /// texture at the node's transform (not an abstract marker), honoring offset
    /// and flip; hidden and non-2D nodes are excluded; texture-less and other 2D
    /// nodes fall back to a marker.
    #[test]
    fn editor2d_renders_sprite2d_texture() {
        gdobject::class_db::register_2d_classes();
        gdobject::class_db::register_3d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let sprite = tree.add_child(root, Node::new("Hero", "Sprite2D")).unwrap();
        let bare = tree.add_child(root, Node::new("Empty", "Node2D")).unwrap();
        let untextured = tree
            .add_child(root, Node::new("Blank", "Sprite2D"))
            .unwrap();
        let hidden = tree
            .add_child(root, Node::new("Ghost", "Sprite2D"))
            .unwrap();
        // A non-2D (spatial) node must never appear in the 2D render list.
        let node3d_id = tree.add_child(root, Node::new("Spatial", "Node3D")).unwrap();

        // The hero sprite: textured, positioned, with an offset and a flip.
        node2d::set_position(&mut tree, sprite, Vector2::new(120.0, 80.0));
        node2d::set_texture_path(&mut tree, sprite, "res://hero.png");
        node2d::set_offset(&mut tree, sprite, Vector2::new(-16.0, -32.0));
        node2d::set_flip_h(&mut tree, sprite, true);

        node2d::set_texture_path(&mut tree, hidden, "res://ghost.png");
        node2d::set_visible(&mut tree, hidden, false);
        node3d::set_position(&mut tree, node3d_id, gdcore::math::Vector3::ZERO);

        let vp = Viewport2D::new(800, 600);
        let items = vp.build_render_list(&tree);
        let ids: Vec<u64> = items.iter().map(|i| i.node_id).collect();

        // Hidden and non-2D nodes are excluded.
        assert!(!ids.contains(&hidden.raw()), "hidden sprite is not drawn");
        assert!(!ids.contains(&node3d_id.raw()), "3D node is not drawn");

        // The textured sprite renders its texture at its global transform,
        // carrying the offset and flip.
        let hero = find(&items, sprite.raw()).expect("sprite item present");
        assert_eq!(hero.global_transform.origin, Vector2::new(120.0, 80.0));
        match &hero.kind {
            Render2DKind::Sprite {
                texture,
                offset,
                flip_h,
                flip_v,
            } => {
                assert_eq!(texture, "res://hero.png");
                assert_eq!(*offset, Vector2::new(-16.0, -32.0));
                assert!(*flip_h, "flip_h honored");
                assert!(!*flip_v, "flip_v defaults off");
            }
            other => panic!("expected a Sprite, got {other:?}"),
        }

        // A texture-less Sprite2D and a plain Node2D both fall back to markers.
        assert_eq!(
            find(&items, untextured.raw()).unwrap().kind,
            Render2DKind::Marker {
                class: "Sprite2D".to_string()
            }
        );
        assert_eq!(
            find(&items, bare.raw()).unwrap().kind,
            Render2DKind::Marker {
                class: "Node2D".to_string()
            }
        );
    }

    /// Sets a raw property on a node (test helper for shape data that has no
    /// dedicated typed setter).
    fn set_prop(tree: &mut SceneTree, id: gdscene::NodeId, key: &str, value: Variant) {
        if let Some(n) = tree.get_node_mut(id) {
            n.set_property(key, value);
        }
    }

    fn points_variant(points: &[Vector2]) -> Variant {
        Variant::Array(points.iter().map(|p| Variant::Vector2(*p)).collect())
    }

    /// Acceptance (pat-1p53i.2): `Polygon2D`, `Line2D`, and `ColorRect` render
    /// their real shape/vector visuals — polygon vertices + fill, polyline
    /// points + width + color, and a sized colored rectangle at its transform —
    /// rather than abstract markers.
    #[test]
    fn editor2d_renders_shape_nodes() {
        gdobject::class_db::register_2d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let poly = tree.add_child(root, Node::new("Poly", "Polygon2D")).unwrap();
        let line = tree.add_child(root, Node::new("Stroke", "Line2D")).unwrap();
        let rect = tree.add_child(root, Node::new("Bg", "ColorRect")).unwrap();

        // Polygon2D: a triangle with a red fill.
        let tri = [
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(0.0, 10.0),
        ];
        set_prop(&mut tree, poly, "polygon", points_variant(&tri));
        set_prop(&mut tree, poly, "color", Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)));

        // Line2D: a two-point green stroke of width 4.
        let pts = [Vector2::new(0.0, 0.0), Vector2::new(20.0, 5.0)];
        set_prop(&mut tree, line, "points", points_variant(&pts));
        set_prop(&mut tree, line, "width", Variant::Float(4.0));
        set_prop(
            &mut tree,
            line,
            "default_color",
            Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)),
        );

        // ColorRect: a 100x40 blue rectangle positioned at (50, 60).
        node2d::set_position(&mut tree, rect, Vector2::new(50.0, 60.0));
        control::set_size(&mut tree, rect, Vector2::new(100.0, 40.0));
        set_prop(&mut tree, rect, "color", Variant::Color(Color::new(0.0, 0.0, 1.0, 1.0)));

        let vp = Viewport2D::new(800, 600);
        let items = vp.build_render_list(&tree);

        // Polygon2D draws its vertices and fill color.
        match &find(&items, poly.raw()).expect("polygon item present").kind {
            Render2DKind::Polygon { points, color } => {
                assert_eq!(points.len(), 3);
                assert_eq!(points[1], Vector2::new(10.0, 0.0));
                assert_eq!(*color, Color::new(1.0, 0.0, 0.0, 1.0));
            }
            other => panic!("expected a Polygon, got {other:?}"),
        }

        // Line2D draws its points, width, and color.
        match &find(&items, line.raw()).expect("line item present").kind {
            Render2DKind::Line {
                points,
                width,
                color,
            } => {
                assert_eq!(points.len(), 2);
                assert_eq!(points[1], Vector2::new(20.0, 5.0));
                assert_eq!(*width, 4.0);
                assert_eq!(*color, Color::new(0.0, 1.0, 0.0, 1.0));
            }
            other => panic!("expected a Line, got {other:?}"),
        }

        // ColorRect draws a sized colored rect at its global transform.
        let rect_item = find(&items, rect.raw()).expect("color-rect item present");
        assert_eq!(rect_item.global_transform.origin, Vector2::new(50.0, 60.0));
        match &rect_item.kind {
            Render2DKind::ColorRect { size, color } => {
                assert_eq!(*size, Vector2::new(100.0, 40.0));
                assert_eq!(*color, Color::new(0.0, 0.0, 1.0, 1.0));
            }
            other => panic!("expected a ColorRect, got {other:?}"),
        }
    }

    /// Acceptance (pat-1p53i.3): the render list honors CanvasItem state —
    /// hidden nodes are skipped, each item carries its `modulate` tint, items are
    /// ordered by `z_index` (lower draws first), and a child's transform composes
    /// its parent's (CanvasItem transform inheritance).
    #[test]
    fn editor2d_honors_visibility_modulate_zindex() {
        gdobject::class_db::register_2d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Three sprites added back, front, mid — but z_index decides draw order.
        let back = tree.add_child(root, Node::new("Back", "Sprite2D")).unwrap();
        let front = tree.add_child(root, Node::new("Front", "Sprite2D")).unwrap();
        let mid = tree.add_child(root, Node::new("Mid", "Sprite2D")).unwrap();
        node2d::set_z_index(&mut tree, back, 0);
        node2d::set_z_index(&mut tree, front, 10);
        node2d::set_z_index(&mut tree, mid, 5);

        // Mid carries a half-transparent red modulate.
        let tint = Color::new(1.0, 0.0, 0.0, 0.5);
        node2d::set_modulate(&mut tree, mid, tint);

        // A hidden node must be skipped entirely.
        let ghost = tree.add_child(root, Node::new("Ghost", "Sprite2D")).unwrap();
        node2d::set_visible(&mut tree, ghost, false);

        // Child under a translated parent inherits the parent's transform.
        let parent = tree.add_child(root, Node::new("Parent", "Node2D")).unwrap();
        node2d::set_position(&mut tree, parent, Vector2::new(100.0, 0.0));
        let child = tree.add_child(parent, Node::new("Child", "Sprite2D")).unwrap();
        node2d::set_position(&mut tree, child, Vector2::new(10.0, 20.0));

        let vp = Viewport2D::new(800, 600);
        let items = vp.build_render_list(&tree);
        let ids: Vec<u64> = items.iter().map(|i| i.node_id).collect();

        // Hidden node excluded.
        assert!(!ids.contains(&ghost.raw()), "hidden node is skipped");

        // The three z-sprites are ordered back (0) -> mid (5) -> front (10).
        let z_order: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| [back.raw(), mid.raw(), front.raw()].contains(id))
            .collect();
        assert_eq!(z_order, vec![back.raw(), mid.raw(), front.raw()]);

        // z_index is recorded on each item.
        assert_eq!(find(&items, front.raw()).unwrap().z_index, 10);
        assert_eq!(find(&items, back.raw()).unwrap().z_index, 0);

        // Modulate is recorded; an untinted node defaults to white.
        assert_eq!(find(&items, mid.raw()).unwrap().modulate, tint);
        assert_eq!(find(&items, back.raw()).unwrap().modulate, Color::WHITE);

        // The child's global transform composes the parent's: (100,0)+(10,20).
        assert_eq!(
            find(&items, child.raw()).unwrap().global_transform.origin,
            Vector2::new(110.0, 20.0)
        );
    }

    /// Acceptance (pat-1p53i.4): a `Label` renders its text at the node's
    /// transform, carrying the displayed string, font size, and horizontal
    /// alignment — not an abstract marker.
    #[test]
    fn editor2d_renders_label_text() {
        gdobject::class_db::register_2d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let label = tree.add_child(root, Node::new("Title", "Label")).unwrap();
        control::set_label_text(&mut tree, label, "Hello, Patina");
        control::set_font_size(&mut tree, label, 24);
        control::set_h_align(&mut tree, label, TextAlign::Center);
        node2d::set_position(&mut tree, label, Vector2::new(40.0, 50.0));

        let vp = Viewport2D::new(800, 600);
        let items = vp.build_render_list(&tree);

        let item = find(&items, label.raw()).expect("label item present");
        assert_eq!(item.global_transform.origin, Vector2::new(40.0, 50.0));
        match &item.kind {
            Render2DKind::Text {
                text,
                font_size,
                h_align,
            } => {
                assert_eq!(text, "Hello, Patina");
                assert_eq!(*font_size, 24);
                assert_eq!(*h_align, TextAlign::Center);
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }
}
