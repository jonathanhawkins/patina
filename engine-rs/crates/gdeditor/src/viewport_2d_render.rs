//! Editor chrome over the shared 2D canvas builder (pat-mgixk.28).
//!
//! The builder itself — `collect_canvas_items`, `render_kind_for`,
//! `tilemap_quads`, the camera and parallax transforms, the rasterizer and the
//! light shading pass — now lives in [`gdscene::canvas_render`], so the
//! runtime can draw a 2D scene without `gdeditor` in the path. It is
//! re-exported in full below, so every existing call site and test in this
//! crate keeps working against `crate::viewport_2d_render::*`.
//!
//! What remains here is what only the EDITOR has: the viewport's own overlay
//! settings, and the navmesh debug draw that composes over the scene.
//!
//! ⚠️ `Viewport2D::build_render_list` stays here for a hard reason, not a
//! stylistic one: `Viewport2D` is this crate's type, and Rust will not let a
//! crate add an inherent method to a type it does not own. The scene half of
//! that method is `gdscene::canvas_render::collect_canvas_items`; this one adds
//! the overlays to it.

pub use gdscene::canvas_render::*;

use gdcore::math::{Color, Transform2D};
use gdscene::SceneTree;
// Used by the `Render2DItem` literal below (`TextureFilter::Nearest`,
// `TextureRepeat::Disabled`). The `use` line these came in on left with the
// module body when the canvas builder moved to `gdscene::canvas_render`
// (pat-mgixk.28), while these two call sites stayed — so `gdeditor` stopped
// compiling. The `canvas_render` glob above does not re-export them.
//
// Restored by TurquoiseCastle to unblock the verify lane; pat-mgixk.28 owns
// this file and should fold it into its own import block.
use gdserver2d::material::{TextureFilter, TextureRepeat};

use crate::viewport_2d::Viewport2D;

impl Viewport2D {
    /// Builds the 2D viewport's render list from `tree`: each **visible**
    /// `CanvasItem` node (the Node2D and Control families) becomes a
    /// [`Render2DItem`] — a sprite for texture-bearing `Sprite2D` nodes, a
    /// polygon/line/color-rect for the corresponding shape nodes, or a marker
    /// otherwise. Nodes hidden by their own or an ancestor's `visible` flag are
    /// skipped, and items appear in scene-tree order (the back-to-front draw
    /// order).
    pub fn build_render_list(&self, tree: &SceneTree) -> Vec<Render2DItem> {
        let mut items = collect_canvas_items(tree);
        // Editor chrome draws over the scene (pat-s3rrv.82). `self.overlays` is
        // the LIVE overlay set — this method is the seam where it is already in
        // scope, which is why the overlay hangs here rather than off the
        // tree-only collectors.
        items.extend(navigation_overlay_items(tree, &self.overlays));
        items
    }
}

/// The navigation overlay as viewport draw items: a filled [`Render2DKind::Polygon`]
/// per navmesh face and a [`Render2DKind::Line`] per edge.
///
/// Returns empty when the `NavigationRegions` overlay is off —
/// [`navigation_overlay_2d`] does that check itself, so the toggle costs nothing
/// rather than merely hiding geometry that was still built.
///
/// ⚠️ `global_transform` is IDENTITY and the points go in as-authored.
/// `navigation_overlay_2d` has already offset every point into world space using
/// the region's own position; composing a transform on top would move the overlay
/// twice and put it at double the region's offset.
///
/// ⚠️ Drawn at [`CONTROL_Z_BASE`] so the overlay sits above the scene it
/// describes. Faces are pushed before edges so the outline reads on top of its
/// own fill rather than being half-covered by it.
pub fn navigation_overlay_items(
    tree: &SceneTree,
    overlays: &crate::viewport_2d::OverlaySettings,
) -> Vec<Render2DItem> {
    let overlay = crate::navigation_overlay::navigation_overlay_2d(tree, overlays);
    let mut out = Vec::with_capacity(overlay.faces.len() + overlay.edges.len());
    // Editor chrome: not a scene node, so `node_id` is 0 ("not pickable"), the
    // item tint stays neutral because each face carries its own colour, and the
    // default light/visibility layers keep it visible to a default cull mask.
    let mut push = |kind: Render2DKind, z: i64| {
        out.push(Render2DItem {
            node_id: 0,
            global_transform: Transform2D::IDENTITY,
            modulate: Color::new(1.0, 1.0, 1.0, 1.0),
            z_index: z,
            canvas_layer: 0,
            canvas_follow: CanvasFollow::DefaultCanvas,
            clip_rect: None,
            light_mask: 1,
            visibility_layer: 1,
            canvas_material: None,
            shader_material: None,
            texture_filter: TextureFilter::Nearest,
            texture_repeat: TextureRepeat::Disabled,
            kind,
        });
    };
    for face in &overlay.faces {
        push(
            Render2DKind::Polygon {
                points: face.points.clone(),
                color: face.color,
            },
            CONTROL_Z_BASE,
        );
    }
    for edge in &overlay.edges {
        push(
            Render2DKind::Line {
                points: vec![edge.from, edge.to],
                width: 1.0,
                color: edge.color,
            },
            CONTROL_Z_BASE + 1,
        );
    }
    out
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
        let node3d_id = tree
            .add_child(root, Node::new("Spatial", "Node3D"))
            .unwrap();

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
                ..
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

        let poly = tree
            .add_child(root, Node::new("Poly", "Polygon2D"))
            .unwrap();
        let line = tree.add_child(root, Node::new("Stroke", "Line2D")).unwrap();
        let rect = tree.add_child(root, Node::new("Bg", "ColorRect")).unwrap();

        // Polygon2D: a triangle with a red fill.
        let tri = [
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(0.0, 10.0),
        ];
        set_prop(&mut tree, poly, "polygon", points_variant(&tri));
        set_prop(
            &mut tree,
            poly,
            "color",
            Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );

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
        set_prop(
            &mut tree,
            rect,
            "color",
            Variant::Color(Color::new(0.0, 0.0, 1.0, 1.0)),
        );

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
        let front = tree
            .add_child(root, Node::new("Front", "Sprite2D"))
            .unwrap();
        let mid = tree.add_child(root, Node::new("Mid", "Sprite2D")).unwrap();
        node2d::set_z_index(&mut tree, back, 0);
        node2d::set_z_index(&mut tree, front, 10);
        node2d::set_z_index(&mut tree, mid, 5);

        // Mid carries a half-transparent red modulate.
        let tint = Color::new(1.0, 0.0, 0.0, 0.5);
        node2d::set_modulate(&mut tree, mid, tint);

        // A hidden node must be skipped entirely.
        let ghost = tree
            .add_child(root, Node::new("Ghost", "Sprite2D"))
            .unwrap();
        node2d::set_visible(&mut tree, ghost, false);

        // Child under a translated parent inherits the parent's transform.
        let parent = tree.add_child(root, Node::new("Parent", "Node2D")).unwrap();
        node2d::set_position(&mut tree, parent, Vector2::new(100.0, 0.0));
        let child = tree
            .add_child(parent, Node::new("Child", "Sprite2D"))
            .unwrap();
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
                ..
            } => {
                assert_eq!(text, "Hello, Patina");
                assert_eq!(*font_size, 24);
                assert_eq!(*h_align, TextAlign::Center);
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    /// 🔑 THE EDITOR RENDER LIST CARRIES THE NAVMESH — AND ONLY WHEN ASKED.
    ///
    /// This is the seam the bead is about: `navigation_overlay_2d` existed with
    /// tests, and nothing called it, so the geometry was computed and never
    /// drawn. `Viewport2D::build_render_list` is where the live overlay set is
    /// already in scope.
    ///
    /// ⚠️ The toggle-off half is asserted against the SAME scene, so a pass
    /// cannot come from the region failing to bake — the on/off pair differ by
    /// exactly one flag.
    #[test]
    fn the_editor_render_list_carries_navmesh_faces_and_edges_when_enabled() {
        use crate::viewport_2d::CanvasOverlay;

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut region = Node::new("Region", "NavigationRegion2D");
        region.set_property(
            "__navmesh_vertices__",
            Variant::PackedVector2Array(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(10.0, 0.0),
                Vector2::new(10.0, 10.0),
                Vector2::new(0.0, 10.0),
            ]),
        );
        region.set_property(
            "__navmesh_polygons__",
            Variant::Array(vec![Variant::PackedInt32Array(vec![0, 1, 2, 3])].into()),
        );
        tree.add_child(root, region).expect("region attaches");

        let mut vp = Viewport2D::new(800, 600);
        let baseline = vp.build_render_list(&tree).len();

        vp.overlays.enable(CanvasOverlay::NavigationRegions);
        let items = vp.build_render_list(&tree);

        let polys = items
            .iter()
            .filter(|i| matches!(i.kind, Render2DKind::Polygon { .. }))
            .count();
        let lines = items
            .iter()
            .filter(|i| matches!(i.kind, Render2DKind::Line { .. }))
            .count();
        assert_eq!(polys, 1, "the baked quad must become one filled face");
        assert_eq!(lines, 4, "and a closed quad is four edge segments");
        assert!(
            items.len() > baseline,
            "enabling the overlay must ADD items: {baseline} -> {}",
            items.len()
        );

        // ⚠️ Same scene, toggle off: back to the baseline exactly. A region that
        // simply failed to bake would also produce no overlay items, which is
        // why this compares against the enabled run rather than against zero.
        vp.overlays.disable(CanvasOverlay::NavigationRegions);
        assert_eq!(
            vp.build_render_list(&tree).len(),
            baseline,
            "turning the overlay off must cost nothing, not merely hide it"
        );
    }

    /// 🔑 THE OVERLAY IS NOT OFFSET TWICE.
    ///
    /// ⚠️ `navigation_overlay_2d` has already placed every point in world space
    /// using the region's own position, so the emitted item must carry an
    /// IDENTITY transform. Composing the region's transform again would draw the
    /// overlay at double the offset — a bug that is invisible at the origin,
    /// which is why this region sits at (100, 50).
    #[test]
    fn the_overlay_is_placed_in_world_space_exactly_once() {
        use crate::viewport_2d::CanvasOverlay;

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut region = Node::new("Region", "NavigationRegion2D");
        region.set_property("position", Variant::Vector2(Vector2::new(100.0, 50.0)));
        region.set_property(
            "__navmesh_vertices__",
            Variant::PackedVector2Array(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(10.0, 0.0),
                Vector2::new(10.0, 10.0),
            ]),
        );
        region.set_property(
            "__navmesh_polygons__",
            Variant::Array(vec![Variant::PackedInt32Array(vec![0, 1, 2])].into()),
        );
        tree.add_child(root, region).expect("region attaches");

        let mut vp = Viewport2D::new(800, 600);
        vp.overlays.enable(CanvasOverlay::NavigationRegions);
        let items = vp.build_render_list(&tree);
        let face = items
            .iter()
            .find(|i| matches!(i.kind, Render2DKind::Polygon { .. }))
            .expect("the region contributes a face");

        assert_eq!(
            face.global_transform,
            Transform2D::IDENTITY,
            "the points are already world-space; a transform here offsets twice"
        );
        let Render2DKind::Polygon { points, .. } = &face.kind else {
            unreachable!("filtered on Polygon above");
        };
        assert_eq!(
            points[0],
            Vector2::new(100.0, 50.0),
            "the first vertex is the region's own origin, offset once"
        );
    }
}
