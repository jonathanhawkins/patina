//! pat-zszz: Headless mode (no window) for CI.
//!
//! Validates that the engine can run without a display server.

#[test]
fn headless_window_creates_without_display() {
    let window = gdplatform::window::HeadlessWindow::new();
    // Should not panic or require a display server
    let _ = format!("{:?}", window);
}

#[test]
fn headless_platform_creates() {
    let platform = gdplatform::backend::HeadlessPlatform::new(800, 600);
    let _ = format!("{:?}", platform);
}

#[test]
fn scene_tree_runs_without_window() {
    let mut tree = gdscene::scene_tree::SceneTree::new();
    let root = tree.root_id();
    let child = tree
        .add_child(root, gdscene::node::Node::new("Test", "Node"))
        .unwrap();
    assert!(tree.get_node(child).is_some());
}

#[test]
fn software_renderer_works_headless() {
    use gdcore::math::{Color, Rect2, Vector2};
    use gdrender2d::renderer::SoftwareRenderer;
    use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
    use gdserver2d::server::RenderingServer2D;
    use gdserver2d::viewport::Viewport;

    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(5.0, 5.0)),
        color: Color::rgb(0.0, 1.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(frame.width, 10);
    assert!(frame.pixels[0].g > 0.9, "should render green");
}

#[test]
fn patina_runner_exists_as_binary() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let debug = manifest.join("target/debug/patina-runner");
    let release = manifest.join("target/release/patina-runner");
    assert!(
        debug.exists() || release.exists(),
        "patina-runner binary should exist for headless CI"
    );
}
