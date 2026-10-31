//! pat-mgixk.25 acceptance: a pointer event is mapped out of window pixels into
//! canvas coordinates before anything sees it.
//!
//! # What was wrong
//!
//! Godot transforms every pointer event by the inverse of the window's
//! content-scale transform inside `Viewport::push_input`
//! (`scene/main/viewport.cpp`), which is what makes a click land on what was
//! drawn.
//!
//! Patina DREW through the stretch transform —
//! `gdeditor/src/scene_renderer.rs` applies it — but never mapped input back.
//! `RootWindow::window_to_canvas` carried the comment *"Input MUST go through
//! this, or every click lands in the wrong place the moment a project turns
//! stretching on"* and had **no caller outside its own unit tests**. So the
//! comment was right and nothing acted on it: with `canvas_items` stretching at
//! 2×, every click arrived at twice its true canvas position.
//!
//! # Why a Button, and why the signal
//!
//! The press is posted at window (300, 240). Mapped, that is canvas (150, 120)
//! — inside a Button occupying (100, 100)..(200, 140). **Unmapped it is (300,
//! 240), which is outside.** So `pressed` firing is a binary discriminator for
//! the mapping having happened, not a proxy for it.
//!
//! The signal is observed through a connected callback with an atomic counter —
//! the pattern `base_button_toggle_group_and_capture_semantics` uses — so the
//! assertion needs no script interpreter and no `event.position` support.
//!
//! # The path driven
//!
//! ```text
//! MainLoop::push_event   <- the line under test (window -> canvas)
//!   -> MainLoop::step -> SceneTree::push_input -> gui_input_stage
//!     -> BaseButton -> `pressed`
//! ```
//!
//! ⚠️ It must go through `MainLoop::push_event`, NOT `SceneTree::push_input`
//! directly: the mapping lives in the loop, which is the path `patina-runner`
//! and editor Play both take. A test calling `tree.push_input` would bypass the
//! change entirely and pass either way.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use gdcore::content_scale::{ContentScaleAspect, ContentScaleMode};
use gdcore::id::ObjectId;
use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Godot's `canvas_items` default: 640x360 drawn into a 1280x720 window, i.e.
/// an exact 2x scale, so the expected canvas point is unambiguous.
const BASE: Vector2 = Vector2 { x: 640.0, y: 360.0 };
const WINDOW_W: u32 = 1280;
const WINDOW_H: u32 = 720;

/// Counts emissions of a no-arg signal.
fn count_signal(tree: &mut SceneTree, node: NodeId, signal: &str, tag: u64) -> Arc<AtomicUsize> {
    let n = Arc::new(AtomicUsize::new(0));
    let sink = n.clone();
    tree.connect_signal(
        node,
        signal,
        Connection::with_callback(ObjectId::from_raw(tag), "on_signal", move |_args| {
            sink.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );
    n
}

/// A scene whose root window stretches `BASE` up to the window size, with one
/// Button at canvas (100,100)..(200,140).
fn stretched_scene() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let button = tree.add_child(root, Node::new("Ok", "Button")).unwrap();
    if let Some(n) = tree.get_node_mut(button) {
        n.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        n.set_property("size", Variant::Vector2(Vector2::new(100.0, 40.0)));
    }

    // Godot seeds these from `display/window/stretch/*` at boot; set them
    // directly so the test states the configuration it depends on instead of
    // relying on a project file to imply it.
    {
        let window = tree.root_window_mut();
        window.config.mode = ContentScaleMode::CanvasItems;
        window.config.aspect = ContentScaleAspect::Ignore;
        window.config.base_size = BASE;
    }
    tree.publish_root_window_state();

    let mut main_loop = MainLoop::new(tree);
    // The window is LARGER than the base size — that difference is the whole
    // point. `set_viewport_size` is the resize path the editor and the runner
    // use, so the transform is recomputed the way it is in a real session.
    main_loop.set_viewport_size(WINDOW_W, WINDOW_H);
    (main_loop, button)
}

/// **The acceptance test.** A press posted in window pixels reaches the Button
/// that occupies the corresponding canvas rect.
#[test]
fn input_stretch_coordinate_mapping_test() {
    let (mut main_loop, button) = stretched_scene();

    // Fixture check first: if the window were not actually stretching, every
    // assertion below would hold for the trivial reason.
    assert!(
        main_loop.tree().root_window().is_stretching(),
        "the fixture must have stretching ON, or this test proves nothing"
    );
    let drawn = main_loop
        .tree()
        .root_window()
        .stretch_transform()
        .xform(Vector2::new(150.0, 120.0));
    assert!(
        (drawn.x - 300.0).abs() < 0.01 && (drawn.y - 240.0).abs() < 0.01,
        "canvas (150,120) must DRAW at window (300,240) for this scale; got {drawn:?}"
    );

    let pressed = count_signal(main_loop.tree_mut(), button, "pressed", 9001);
    let down = count_signal(main_loop.tree_mut(), button, "button_down", 9002);

    // Window pixels, as a browser or the editor's game view reports them.
    main_loop.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(300.0, 240.0),
    });
    main_loop.step(1.0 / 60.0);
    assert_eq!(
        down.load(Ordering::SeqCst),
        1,
        "the press at window (300,240) must reach the Button drawn over that \
         pixel. Unmapped it stays (300,240), which is OUTSIDE the Button's \
         canvas rect (100,100)..(200,140), so nothing fires — which is exactly \
         what happened before `push_event` applied `window_to_canvas`."
    );

    main_loop.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: Vector2::new(300.0, 240.0),
    });
    main_loop.step(1.0 / 60.0);
    assert_eq!(
        pressed.load(Ordering::SeqCst),
        1,
        "`pressed` fires on release, so the release must map into the Button too"
    );
}

/// The other half: a press whose MAPPED point falls outside the Button must not
/// fire, or the test above would pass for any mapping at all — including one
/// that clamps every event into the control.
#[test]
fn a_press_outside_the_mapped_rect_does_not_fire() {
    let (mut main_loop, button) = stretched_scene();
    let down = count_signal(main_loop.tree_mut(), button, "button_down", 9003);

    // Window (100,100) maps to canvas (50,50) — outside (100,100)..(200,140).
    // ⚠️ This is the point that WOULD hit if the mapping were skipped, so it
    // pins the direction of the transform, not merely its presence.
    main_loop.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(100.0, 100.0),
    });
    main_loop.step(1.0 / 60.0);
    assert_eq!(
        down.load(Ordering::SeqCst),
        0,
        "window (100,100) maps to canvas (50,50), which is outside the Button — \
         if this fires, the event was used unmapped"
    );
}

/// With stretching off the transform is the identity, so an event must arrive
/// unchanged. This is what keeps the fix from becoming a regression for every
/// project that does not stretch.
#[test]
fn an_unstretched_window_leaves_coordinates_alone() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let button = tree.add_child(root, Node::new("Ok", "Button")).unwrap();
    if let Some(n) = tree.get_node_mut(button) {
        n.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        n.set_property("size", Variant::Vector2(Vector2::new(100.0, 40.0)));
    }
    let mut main_loop = MainLoop::new(tree);
    main_loop.set_viewport_size(WINDOW_W, WINDOW_H);
    assert!(
        !main_loop.tree().root_window().is_stretching(),
        "no base size was configured, so nothing should be stretching"
    );

    let down = count_signal(main_loop.tree_mut(), button, "button_down", 9004);
    // Raw canvas coordinates, which with no stretch are also window pixels.
    main_loop.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(150.0, 120.0),
    });
    main_loop.step(1.0 / 60.0);
    assert_eq!(
        down.load(Ordering::SeqCst),
        1,
        "with stretching off, (150,120) must still hit the Button at \
         (100,100)..(200,140) — the mapping must be a no-op, not a scale"
    );
}

/// A motion event's `relative` is a DELTA and must take the basis only.
///
/// Applying the full affine inverse to it would subtract the transform's
/// translation from a difference of two points, which under a letterboxed
/// aspect offsets every mouse delta by the black-bar margin — a drag that
/// drifts further the further from the origin it starts.
#[test]
fn a_motion_delta_is_scaled_without_the_translation() {
    let (mut main_loop, _button) = stretched_scene();
    main_loop.push_event(InputEvent::MouseMotion {
        position: Vector2::new(300.0, 240.0),
        relative: Vector2::new(20.0, 10.0),
    });
    let motion = main_loop.input_state().mouse_motion_this_frame();
    assert!(
        (motion.x - 10.0).abs() < 0.01 && (motion.y - 5.0).abs() < 0.01,
        "a 2x stretch must halve a (20,10) window delta to (10,5) in canvas \
         space, with no translation added; got {motion:?}"
    );
}
