//! pat-nzgdu.87 — the SceneTree root is a `Window`/`Viewport`, and the
//! project's stretch settings reach the transform the 2D draw path applies.
//!
//! Godot ground truth: `SceneTree.root` is a `Window` (a `Viewport`), every
//! node reaches it through `get_viewport()`/`get_window()`, and
//! `Window::_update_viewport_size()` turns
//! `display/window/stretch/{mode,aspect,scale,scale_mode}` into the transform
//! the whole canvas is drawn through.
//!
//! Patina created the root as a bare `Node` with no viewport semantics, had no
//! `get_viewport`/`get_window` at all, and stored content-scale only as an
//! unused project-settings string. This test walks the wired chain:
//!
//! ```text
//! project.godot (display/window/stretch/*)
//!   -> ProjectSettings::content_scale_config
//!   -> SceneTree::root_window_mut().config
//!   -> RootWindow::stretch_transform  (gdcore::content_scale)
//! ```

use gdcore::content_scale::{ContentScaleAspect, ContentScaleMode, ContentScaleStretch};
use gdcore::math::Vector2;
use gdcore::project_settings::ProjectSettings;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::viewport::{get_viewport, get_window};

fn write_project(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("project.godot");
    std::fs::write(&path, body).expect("write project.godot");
    path
}

/// The root is a Window, a Viewport, and still a Node — the ClassDB chain has
/// to be registered by `SceneTree::new` itself, since nothing else guarantees
/// it.
#[test]
fn scene_tree_root_is_a_window_viewport_and_node() {
    let tree = SceneTree::new();
    let root = tree.get_node(tree.root_id()).expect("root exists");

    assert_eq!(root.name(), "root", "Godot names the root 'root'");
    assert_eq!(root.class_name(), "Window");
    assert!(root.is_class("Viewport"), "Window derives from Viewport");
    assert!(
        root.is_class("Node"),
        "the root must remain a Node — if this fails, SceneTree::new stopped \
         registering the Object->Node->Viewport->Window chain and every \
         is_class check on the root is now wrong"
    );
}

/// Every node in the tree can reach the root window, however deep it sits.
#[test]
fn get_viewport_and_get_window_resolve_for_every_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(a, Node::new("B", "Sprite2D")).unwrap();
    let c = tree.add_child(b, Node::new("C", "Label")).unwrap();

    for node in [a, b, c] {
        assert_eq!(get_viewport(&tree, node), root);
        assert_eq!(get_window(&tree, node), root);
    }
}

/// A project with no stretch settings must render 1:1 — introducing the
/// pipeline cannot change how an existing project looks.
#[test]
fn a_project_without_stretch_settings_does_not_scale() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_project(
        dir.path(),
        "config_version=5\n\n[display]\nwindow/size/viewport_width=800\n\
         window/size/viewport_height=600\n",
    );
    let settings = ProjectSettings::load_godot_project(&path).expect("load project");

    let mut tree = SceneTree::new();
    tree.root_window_mut().config = settings.content_scale_config();
    tree.root_window_mut().size = Vector2::new(1600.0, 1200.0);

    assert!(
        !tree.root_window().is_stretching(),
        "stretch/mode defaults to disabled"
    );
    assert_eq!(
        tree.root_window().stretch_transform(),
        gdcore::math::Transform2D::IDENTITY
    );
    assert_eq!(
        tree.root_window().visible_size(),
        Vector2::new(1600.0, 1200.0),
        "with stretching off the canvas IS the window"
    );
}

/// The full wired path: a `canvas_items` project stretches its base resolution
/// over the window while the canvas keeps reporting the authored size.
#[test]
fn stretch_settings_from_project_godot_reach_the_draw_transform() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_project(
        dir.path(),
        "config_version=5\n\n[display]\nwindow/size/viewport_width=640\n\
         window/size/viewport_height=360\nwindow/stretch/mode=\"canvas_items\"\n\
         window/stretch/aspect=\"keep\"\n",
    );
    let settings = ProjectSettings::load_godot_project(&path).expect("load project");
    let cfg = settings.content_scale_config();
    assert_eq!(cfg.mode, ContentScaleMode::CanvasItems);
    assert_eq!(cfg.aspect, ContentScaleAspect::Keep);
    assert_eq!(cfg.base_size, Vector2::new(640.0, 360.0));

    let mut tree = SceneTree::new();
    tree.root_window_mut().config = cfg;
    tree.root_window_mut().size = Vector2::new(1920.0, 1080.0);

    let w = tree.root_window();
    assert!(w.is_stretching());
    assert_eq!(
        w.visible_size(),
        Vector2::new(640.0, 360.0),
        "game code still sees a 640x360 canvas"
    );

    // A sprite authored at the canvas centre lands at the window centre, 3x out.
    let drawn = w.stretch_transform().xform(Vector2::new(320.0, 180.0));
    assert!((drawn.x - 960.0).abs() < 0.01, "{drawn:?}");
    assert!((drawn.y - 540.0).abs() < 0.01, "{drawn:?}");
}

/// `keep` on a mismatched aspect letterboxes, and input maps back through the
/// bars correctly — a click in the bar is outside the canvas, not clamped onto
/// its edge.
#[test]
fn letterboxed_project_maps_input_back_through_the_bars() {
    let mut tree = SceneTree::new();
    tree.root_window_mut().config = gdcore::content_scale::ContentScaleConfig {
        mode: ContentScaleMode::CanvasItems,
        aspect: ContentScaleAspect::Keep,
        base_size: Vector2::new(640.0, 360.0),
        ..Default::default()
    };
    // 4:3 window around a 16:9 game: bars top and bottom.
    tree.root_window_mut().size = Vector2::new(640.0, 480.0);

    let w = tree.root_window();
    let scale = w.content_scale();
    assert_eq!(scale.margin, Vector2::new(0.0, 60.0), "(480-360)/2");

    let canvas_centre = w.window_to_canvas(Vector2::new(320.0, 240.0));
    assert!((canvas_centre.x - 320.0).abs() < 0.01, "{canvas_centre:?}");
    assert!((canvas_centre.y - 180.0).abs() < 0.01, "{canvas_centre:?}");

    let in_the_bar = w.window_to_canvas(Vector2::new(320.0, 10.0));
    assert!(
        in_the_bar.y < 0.0,
        "a click in the top bar is above the canvas, not clamped to 0: {in_the_bar:?}"
    );
}

/// `viewport` mode renders at the base resolution instead of the window's —
/// the difference that gives pixel-art projects their look.
#[test]
fn viewport_mode_renders_at_the_base_resolution() {
    let mut tree = SceneTree::new();
    tree.root_window_mut().config = gdcore::content_scale::ContentScaleConfig {
        mode: ContentScaleMode::Viewport,
        aspect: ContentScaleAspect::Keep,
        base_size: Vector2::new(320.0, 180.0),
        ..Default::default()
    };
    tree.root_window_mut().size = Vector2::new(1280.0, 720.0);

    let scale = tree.root_window().content_scale();
    assert_eq!(
        scale.final_size,
        Vector2::new(320.0, 180.0),
        "the frame is rendered small…"
    );
    assert_eq!(
        scale.screen_size,
        Vector2::new(1280.0, 720.0),
        "…then blown up to the window"
    );
}

/// ⚠️ The integer-stretch setting is `scale_mode`, NOT `stretch_mode`. Reading
/// the wrong key leaves every project on fractional scaling with tests that
/// still pass, so the key name is pinned here against project.godot itself.
#[test]
fn integer_scale_mode_is_read_from_the_right_project_key() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = write_project(
        dir.path(),
        "config_version=5\n\n[display]\nwindow/size/viewport_width=320\n\
         window/size/viewport_height=180\nwindow/stretch/mode=\"canvas_items\"\n\
         window/stretch/scale_mode=\"integer\"\n",
    );
    let settings = ProjectSettings::load_godot_project(&path).expect("load project");
    assert_eq!(
        settings.content_scale_config().stretch,
        ContentScaleStretch::Integer
    );

    let mut tree = SceneTree::new();
    tree.root_window_mut().config = settings.content_scale_config();
    // 320x180 into 1000x700: fractional would be 3.12x, integer drops to 3x.
    tree.root_window_mut().size = Vector2::new(1000.0, 700.0);
    assert_eq!(
        tree.root_window().content_scale().screen_size,
        Vector2::new(960.0, 540.0),
        "exactly 3x, with the remainder as bars"
    );
}
