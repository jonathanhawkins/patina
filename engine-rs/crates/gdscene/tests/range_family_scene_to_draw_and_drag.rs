//! Acceptance test for pat-s3rrv.73: the Range family end to end — a `.tscn`
//! loads, draws a grabber at the right place, and a drag moves the value and
//! reaches a **GDScript** handler.
//!
//! Godot ground truth: `scene/gui/range.cpp` (`value_changed` fires only on a
//! real change), `scene/gui/slider.cpp` (a press reads the value off the click
//! position; the wheel moves by `step` when `scrollable`), `scene/gui/scroll_bar.cpp`
//! (a wheel notch moves `page / PAGE_DIVISOR`, floored at `step`; PAGE_DIVISOR
//! is 8 — scroll_bar.h:116).
//!
//! ⚠️ This bead's own description is stale, and the stale part is the
//! interesting part. It claims the classes are unregistered, undrawn, and that
//! `set_range_value` has zero runtime callers with `value_changed` emitted
//! nowhere. All four were true when the audit ran and are not true now
//! (pat-s3rrv.69/.70 landed registration, the draw arms, the drag and the
//! emission). What remains is the part no earlier pass covered: the journey from
//! a **scene file** to a moved value, and the wheel.
//!
//! The ordering below is not incidental. `control::get_size` reads the stored
//! `size` property, and a `.tscn` Control carries `offset_right`/`offset_bottom`
//! instead — `size` is only written when the layout solver runs, inside the
//! render pass (`control_render.rs:472` -> `resolve_control_layout`). So a scene
//! must be **drawn before it can be dragged**, which is exactly the order a real
//! frame does it in, and exactly what this test exercises.

use gdcore::math::{Color, Vector2};
use gdobject::signal::Connection;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::control;
use gdscene::control_render::render_control_tree;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::range_controls::{self, SliderAxis};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdscene::GDScriptNodeInstance;
// `list_properties` is a trait method — the trait must be in scope to call it.
use gdscript_interop::bindings::ScriptInstance;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// A slider 110 wide has a track of exactly 100 (`width - GRABBER_EXTENT`), so
/// value 25 puts the grabber's near edge at x = 25 and a press at x = 5 + 75
/// selects 75. The numbers below are read off that, not tuned to the output.
const SCENE: &str = "[gd_scene format=3]\n\
\n\
[node name=\"Root\" type=\"Control\"]\n\
offset_right = 300.0\n\
offset_bottom = 120.0\n\
\n\
[node name=\"Bar\" type=\"HSlider\" parent=\".\"]\n\
offset_right = 110.0\n\
offset_bottom = 16.0\n\
min_value = 0.0\n\
max_value = 100.0\n\
step = 1.0\n\
value = 25.0\n";

/// Records every payload `value_changed` delivers, so "exactly once per change"
/// is checked as a count and not merely as a final value.
const ROOT_GD: &str = "\
extends Control
var calls = 0
var last = -1.0
func _on_value_changed(v):
    self.calls = self.calls + 1
    self.last = v
";

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Option<Variant> {
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn as_f64(v: Option<Variant>) -> f64 {
    match v {
        Some(Variant::Float(f)) => f,
        Some(Variant::Int(i)) => i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// Loads `SCENE`, returning the tree, the instanced root and the slider.
fn load_scene() -> (SceneTree, NodeId, NodeId) {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let inst = add_packed_scene_to_tree(&mut tree, tree_root, &scene).expect("scene instances");
    let bar = tree
        .find_child(inst, "Bar", true)
        .expect("the scene's HSlider");
    (tree, inst, bar)
}

/// Draws the tree and returns every filled rect, in paint order.
///
/// Drawing is also what makes the scene draggable: the render walk runs the
/// layout solver, which is what writes `size` from the scene's offsets.
fn draw(tree: &mut SceneTree, root: NodeId) -> Vec<(Vector2, Vector2)> {
    let mut vp = Viewport::new(300, 120, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(tree, root, &theme, &mut vp, Vector2::new(300.0, 120.0));
    vp.get_sorted_items()
        .iter()
        .flat_map(|it| it.commands.clone())
        .filter_map(|c| match c {
            DrawCommand::DrawRect {
                rect, filled: true, ..
            } => Some((rect.position, rect.size)),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Every concrete member of the family exists and reports `Range` in its chain.
#[test]
fn the_whole_range_family_is_registered_under_range() {
    gdobject::class_db::register_2d_classes();

    for cls in [
        "HSlider",
        "VSlider",
        "HScrollBar",
        "VScrollBar",
        "SpinBox",
        "TextureProgressBar",
    ] {
        assert!(
            gdobject::class_db::class_exists(cls),
            "{cls} must exist in ClassDB — otherwise a .tscn naming it cannot \
             even be introspected"
        );
        assert!(
            gdobject::class_db::is_parent_class(cls, "Range"),
            "{cls} must report Range in its inheritance chain; the value model \
             lives there"
        );
    }
}

// ---------------------------------------------------------------------------
// Scene -> draw
// ---------------------------------------------------------------------------

/// A scene-loaded HSlider at value 25 draws its grabber a quarter along the
/// track.
///
/// ⚠️ Asserted against the geometry helper rather than a literal: the point is
/// that the DRAWN grabber agrees with the value, not that it sits at any
/// particular pixel. A hard-coded 25.0 would also pass if both the draw arm and
/// the helper were wrong in the same direction.
#[test]
fn a_scene_loaded_slider_draws_its_grabber_at_the_value() {
    let (mut tree, root, bar) = load_scene();
    let rects = draw(&mut tree, root);

    assert!(
        rects.len() >= 2,
        "a slider draws a track and a grabber; got {} filled rect(s). An empty \
         or single-rect list usually means the control resolved to zero size",
        rects.len()
    );

    let size = control::get_size(&tree, bar);
    assert_eq!(
        size.x, 110.0,
        "the layout solver must turn the scene's `offset_right` into a real \
         width, or every ratio below collapses"
    );

    let expected = range_controls::grabber_rect(size, SliderAxis::Horizontal, 0.25);
    let grabber = rects
        .iter()
        .find(|(_, s)| (s.x - expected.size.x).abs() < 1e-3)
        .unwrap_or_else(|| panic!("no rect the width of a grabber among {rects:?}"));

    assert!(
        (grabber.0.x - expected.position.x).abs() < 1e-3,
        "value 25 of 0..100 must put the grabber a quarter along the track \
         (x = {}), got {}",
        expected.position.x,
        grabber.0.x,
    );
}

// ---------------------------------------------------------------------------
// Scene -> drag -> GDScript
// ---------------------------------------------------------------------------

/// Press-dragging to 75% sets `value == 75` and delivers `value_changed(75)`
/// to a GDScript handler exactly once.
///
/// The connection is the method-name form, which is the shape a `.tscn`
/// `[connection … to="."]` produces and dispatches through the same script
/// path.
#[test]
fn dragging_a_scene_loaded_slider_reaches_a_gdscript_handler_once() {
    let (mut tree, root, bar) = load_scene();
    // Draw first: this is what resolves `size` from the scene's offsets.
    draw(&mut tree, root);

    tree.attach_script(
        root,
        Box::new(GDScriptNodeInstance::from_source(ROOT_GD, root).expect("script parses")),
    );
    tree.connect_signal(
        bar,
        "value_changed",
        Connection::new(root.object_id(), "_on_value_changed"),
    );

    // x = 5 + 75: `ratio_at_local_point` is `(x - GRABBER_EXTENT/2) / track`.
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(80.0, 8.0),
    });

    assert_eq!(
        control::get_range_value(&tree, bar),
        75.0,
        "a press at the 75% point must move the value there"
    );
    assert_eq!(
        as_f64(script_var(&tree, root, "last")),
        75.0,
        "and the GDScript handler must receive the new value"
    );
    assert_eq!(
        as_f64(script_var(&tree, root, "calls")),
        1.0,
        "exactly once"
    );

    // A second identical press changes nothing, so the handler must not run
    // again — `Range::set_value` notifies only on a real change.
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: Vector2::new(80.0, 8.0),
    });
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(80.0, 8.0),
    });

    assert_eq!(
        as_f64(script_var(&tree, root, "calls")),
        1.0,
        "an identical press is not a change; a handler that runs per EVENT \
         rather than per CHANGE floods a real project during one drag"
    );
}

// ---------------------------------------------------------------------------
// Wheel
// ---------------------------------------------------------------------------

/// One wheel notch moves a scrollbar by `page / 8`, floored at `step`.
///
/// ⚠️ A slider and a scrollbar move in OPPOSITE directions for the same notch,
/// and it is not a typo: a scrollbar scrolls *down* on wheel-down so wheel-down
/// **adds** (`scroll_bar.cpp:83-92`), while a slider's maximum is "up" so
/// wheel-**up** adds (`slider.cpp:45-56`). One shared sign would leave one of
/// the two running backwards — fine in a screenshot, immediately wrong in the
/// hand.
#[test]
fn a_wheel_notch_moves_a_scrollbar_by_an_eighth_of_a_page() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut bar = gdscene::node::Node::new("Bar", "VScrollBar");
    bar.set_property("size", Variant::Vector2(Vector2::new(12.0, 100.0)));
    bar.set_property("min_value", Variant::Float(0.0));
    bar.set_property("max_value", Variant::Float(100.0));
    bar.set_property("page", Variant::Float(20.0));
    bar.set_property("step", Variant::Float(1.0));
    bar.set_property("value", Variant::Float(0.0));
    let bar = tree.add_child(root, bar).expect("add scrollbar");

    // page 20 / PAGE_DIVISOR 8 = 2.5, comfortably above the step floor of 1.
    assert_eq!(
        range_controls::wheel_delta(&tree, bar, true),
        Some(2.5),
        "wheel-DOWN on a scrollbar adds page/8"
    );
    assert_eq!(
        range_controls::wheel_delta(&tree, bar, false),
        Some(-2.5),
        "wheel-UP subtracts it"
    );

    // The step floor: a page so small that page/8 < step still moves by a step,
    // or a fine-grained bar would need dozens of notches to move at all.
    if let Some(node) = tree.get_node_mut(bar) {
        node.set_property("page", Variant::Float(4.0));
        node.set_property("step", Variant::Float(3.0));
    }
    assert_eq!(
        range_controls::wheel_delta(&tree, bar, true),
        Some(3.0),
        "page/8 = 0.5 is below the step of 3, so the step wins (Godot's \
         `MAX(change, get_step())`)"
    );

    // A slider moves by one step, and in the OPPOSITE sense.
    let mut slider = gdscene::node::Node::new("S", "HSlider");
    slider.set_property("size", Variant::Vector2(Vector2::new(110.0, 16.0)));
    slider.set_property("step", Variant::Float(5.0));
    let slider = tree.add_child(root, slider).expect("add slider");
    assert_eq!(
        range_controls::wheel_delta(&tree, slider, false),
        Some(5.0),
        "wheel-UP on a slider ADDS a step — the opposite of the scrollbar above"
    );
    assert_eq!(
        range_controls::wheel_delta(&tree, slider, true),
        Some(-5.0),
        "and wheel-down subtracts it"
    );

    // `scrollable = false` opts a slider out of the wheel entirely
    // (`slider.cpp`), while leaving drag and keyboard untouched.
    if let Some(node) = tree.get_node_mut(slider) {
        node.set_property("scrollable", Variant::Bool(false));
    }
    assert_eq!(
        range_controls::wheel_delta(&tree, slider, false),
        None,
        "a non-scrollable slider must ignore the wheel"
    );

    // A control outside the family never claims the wheel, which is what keeps
    // this out of ScrollContainer's way.
    let plain = tree
        .add_child(root, gdscene::node::Node::new("P", "Panel"))
        .expect("add panel");
    assert_eq!(range_controls::wheel_delta(&tree, plain, true), None);
}

/// The wheel actually reaches a standalone scrollbar through the input path.
///
/// The test above pins the arithmetic; this one pins the WIRING, and they are
/// different failures. `scroll_under_cursor` only walks up looking for a
/// ScrollContainer, so a bare HScrollBar/VScrollBar in a scene was wheel-deaf —
/// the one control a wheel most obviously belongs to.
///
/// page 40 of a 0..100 range gives `40 / PAGE_DIVISOR` = 5 exactly, so the
/// expected value is not an artefact of step snapping.
#[test]
fn a_wheel_event_scrolls_a_standalone_scrollbar_through_the_input_path() {
    use gdobject::signal::Connection;
    use gdplatform::input::MouseButton;
    use std::sync::{Arc, Mutex};

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut bar = gdscene::node::Node::new("Bar", "VScrollBar");
    bar.set_property("size", Variant::Vector2(Vector2::new(12.0, 100.0)));
    for (k, v) in [
        ("min_value", 0.0),
        ("max_value", 100.0),
        ("page", 40.0),
        ("step", 1.0),
        ("value", 0.0),
    ] {
        bar.set_property(k, Variant::Float(v));
    }
    let bar = tree.add_child(root, bar).expect("add scrollbar");

    let seen: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    tree.connect_signal(
        bar,
        "value_changed",
        Connection::with_callback(
            gdcore::id::ObjectId::from_raw(77),
            "on_changed",
            move |args| {
                if let Some(Variant::Float(f)) = args.first() {
                    sink.lock().unwrap().push(*f);
                }
                Variant::Nil
            },
        ),
    );

    let notch = |down: bool| InputEvent::MouseButton {
        button: if down {
            MouseButton::WheelDown
        } else {
            MouseButton::WheelUp
        },
        pressed: true,
        position: Vector2::new(6.0, 50.0),
    };

    tree.push_input(&notch(true));
    assert_eq!(
        control::get_range_value(&tree, bar),
        5.0,
        "one wheel-down notch must move a standalone scrollbar by page/8"
    );
    assert_eq!(
        seen.lock().unwrap().clone(),
        vec![5.0],
        "and notify, exactly as a drag does"
    );

    tree.push_input(&notch(false));
    assert_eq!(
        control::get_range_value(&tree, bar),
        0.0,
        "and wheel-up brings it back"
    );
}
