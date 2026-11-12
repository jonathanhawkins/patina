//! Viewport/Window tree queries and the root window's content scale
//! (pat-nzgdu.87).
//!
//! In Godot the `SceneTree` root is a `Window`, which is a `Viewport`. Every
//! node can therefore answer `get_viewport()` and `get_window()` by climbing
//! its ancestors, and the root window owns the *stretch* transform that maps
//! the project's base resolution onto the real window.
//!
//! The stretch math itself lives in [`gdcore::content_scale`]; this module is
//! the scene-tree half — which node is the viewport, how big it is, and what
//! transform the 2D draw path must apply.

use gdcore::content_scale::{
    compute_content_scale, ContentScaleConfig, ContentScaleMode, ContentScaleResult,
};
use gdcore::math::{Transform2D, Vector2};

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// The nearest `Viewport` at or above `node` — Godot's `Node.get_viewport()`.
///
/// Returns the node itself when it is a viewport, then walks up. Falls back to
/// the tree root, which is always a `Window` and therefore always a viewport,
/// so this never returns `None` for a node that is in the tree.
///
/// ⚠️ Matches Godot in checking the node *itself* first: a `SubViewport` asking
/// for its viewport gets itself, not its parent.
pub fn get_viewport(tree: &SceneTree, node: NodeId) -> NodeId {
    climb_to_class(tree, node, "Viewport")
}

/// The nearest `Window` at or above `node` — Godot's `Node.get_window()`.
///
/// A `SubViewport` is a `Viewport` but NOT a `Window`, so a node beneath one
/// gets the enclosing window rather than the sub-viewport. That difference is
/// the entire reason Godot has both calls.
pub fn get_window(tree: &SceneTree, node: NodeId) -> NodeId {
    climb_to_class(tree, node, "Window")
}

fn climb_to_class(tree: &SceneTree, node: NodeId, class: &str) -> NodeId {
    let mut current = Some(node);
    while let Some(id) = current {
        match tree.get_node(id) {
            Some(n) => {
                if n.is_class(class) {
                    return id;
                }
                current = n.parent();
            }
            // A stale id: fall back to the root rather than inventing an
            // answer, since every node in a live tree has the root above it.
            None => break,
        }
    }
    tree.root_id()
}

/// The root window's live content-scale state.
///
/// Godot keeps this on the root `Window` itself; Patina hangs it off the tree
/// because the root node is a plain property bag with no per-class storage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RootWindow {
    /// The real window size in pixels (the OS window, or the editor's Play
    /// viewport rect).
    pub size: Vector2,
    /// How the project's base resolution maps onto that window.
    pub config: ContentScaleConfig,
}

impl Default for RootWindow {
    /// Godot's default root window: `1152×648`, no stretching.
    fn default() -> Self {
        Self {
            size: Vector2::new(1152.0, 648.0),
            config: ContentScaleConfig::default(),
        }
    }
}

impl RootWindow {
    /// Lays the base resolution out inside the current window size.
    pub fn content_scale(&self) -> ContentScaleResult {
        compute_content_scale(self.size, &self.config)
    }

    /// Canvas space → window pixels, the transform the 2D draw path must apply
    /// to every `CanvasItem` before rasterizing.
    pub fn stretch_transform(&self) -> Transform2D {
        self.content_scale().window_transform
    }

    /// The size of the canvas coordinate space nodes lay themselves out in —
    /// Godot's `Viewport.get_visible_rect().size`.
    ///
    /// ⚠️ This is NOT the window size once stretching is on: a 640×360 base
    /// resolution in a 1920×1080 window still reports 640×360, which is exactly
    /// why anchoring a Control to the screen edge keeps working.
    pub fn visible_size(&self) -> Vector2 {
        self.content_scale().size_override
    }

    /// Whether any stretching is configured at all.
    ///
    /// A cheap early-out for draw paths that would otherwise multiply every
    /// item by an identity transform.
    pub fn is_stretching(&self) -> bool {
        self.config.mode != ContentScaleMode::Disabled
            && self.config.base_size.x > 0.0
            && self.config.base_size.y > 0.0
    }

    /// Maps a window-space point (a mouse position) back into canvas space.
    ///
    /// ⚠️ Input MUST go through this, or every click lands in the wrong place
    /// the moment a project turns stretching on — the inverse of the transform
    /// the renderer applies.
    pub fn window_to_canvas(&self, window_point: Vector2) -> Vector2 {
        self.content_scale()
            .window_transform
            .affine_inverse()
            .xform(window_point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use gdcore::content_scale::{ContentScaleAspect, ContentScaleMode};

    #[test]
    fn tree_root_is_a_window_and_a_viewport_and_a_node() {
        let tree = SceneTree::new();
        let root = tree.get_node(tree.root_id()).unwrap();
        assert_eq!(root.class_name(), "Window");
        assert!(root.is_class("Window"));
        assert!(
            root.is_class("Viewport"),
            "Window derives from Viewport in Godot"
        );
        // ⚠️ The regression this guards: `SceneTree::new` must register the
        // Object→Node→Viewport→Window chain itself. If it stops doing so, the
        // root silently stops being a Node and every ancestor walk breaks.
        assert!(
            root.is_class("Node"),
            "the root must still be a Node — the ClassDB chain is not registered"
        );
    }

    #[test]
    fn get_viewport_falls_back_to_the_root_window() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = tree.add_child(root, Node::new("Child", "Node2D")).unwrap();
        assert_eq!(get_viewport(&tree, child), root);
        assert_eq!(get_window(&tree, child), root);
    }

    /// A node under a `SubViewport` gets that sub-viewport from
    /// `get_viewport()`, but the root window from `get_window()`.
    #[test]
    fn subviewport_is_a_viewport_but_not_a_window() {
        gdobject::class_db::register_viewport_classes();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let sub = tree
            .add_child(root, Node::new("Sub", "SubViewport"))
            .unwrap();
        let child = tree.add_child(sub, Node::new("Child", "Node2D")).unwrap();

        assert_eq!(
            get_viewport(&tree, child),
            sub,
            "nearest viewport is the sub"
        );
        assert_eq!(
            get_window(&tree, child),
            root,
            "a SubViewport is not a Window, so the window is still the root"
        );
    }

    /// Godot checks the node itself before climbing.
    #[test]
    fn a_viewport_is_its_own_viewport() {
        gdobject::class_db::register_viewport_classes();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let sub = tree
            .add_child(root, Node::new("Sub", "SubViewport"))
            .unwrap();
        assert_eq!(get_viewport(&tree, sub), sub);
        assert_eq!(get_window(&tree, root), root);
    }

    #[test]
    fn default_root_window_does_not_stretch() {
        let w = RootWindow::default();
        assert!(!w.is_stretching());
        assert_eq!(w.stretch_transform(), Transform2D::IDENTITY);
        assert_eq!(w.visible_size(), Vector2::new(1152.0, 648.0));
    }

    /// The point of the whole pipeline: a 640×360 game in a 1920×1080 window
    /// still measures 640×360 to its own code, but draws 3× as big.
    #[test]
    fn stretching_scales_drawing_without_changing_canvas_coordinates() {
        let w = RootWindow {
            size: Vector2::new(1920.0, 1080.0),
            config: ContentScaleConfig {
                mode: ContentScaleMode::CanvasItems,
                aspect: ContentScaleAspect::Keep,
                base_size: Vector2::new(640.0, 360.0),
                ..ContentScaleConfig::default()
            },
        };
        assert!(w.is_stretching());
        assert_eq!(w.visible_size(), Vector2::new(640.0, 360.0));

        let drawn = w.stretch_transform().xform(Vector2::new(320.0, 180.0));
        assert!((drawn.x - 960.0).abs() < 0.01, "{drawn:?}");
        assert!((drawn.y - 540.0).abs() < 0.01, "{drawn:?}");
    }

    /// ⚠️ Input has to travel the transform backwards, or every click lands in
    /// the wrong place once stretching is on.
    #[test]
    fn window_to_canvas_inverts_the_stretch_for_input() {
        let w = RootWindow {
            size: Vector2::new(1440.0, 720.0),
            config: ContentScaleConfig {
                mode: ContentScaleMode::CanvasItems,
                aspect: ContentScaleAspect::Keep,
                base_size: Vector2::new(640.0, 360.0),
                ..ContentScaleConfig::default()
            },
        };
        // This window pillarboxes: 80px bars either side of a 1280x720 image.
        let canvas_point = Vector2::new(320.0, 180.0);
        let window_point = w.stretch_transform().xform(canvas_point);
        let back = w.window_to_canvas(window_point);
        assert!((back.x - canvas_point.x).abs() < 0.01, "{back:?}");
        assert!((back.y - canvas_point.y).abs() < 0.01, "{back:?}");

        // A click inside the left black bar maps to a negative canvas x — it is
        // outside the game's canvas, which is the correct answer, not a clamp.
        let in_the_bar = w.window_to_canvas(Vector2::new(10.0, 360.0));
        assert!(in_the_bar.x < 0.0, "{in_the_bar:?}");
    }
}
