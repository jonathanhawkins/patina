//! pat-s3rrv.173 — a SplitContainer divider can be dragged with the pointer.
//!
//! Ground truth: `upstream/godot/scene/gui/split_container.cpp`.
//!
//! # What this file covers, and what it does not
//!
//! The bead's acceptance names four clauses. **This file covers the first two:
//! the divider drag, and the ScrollContainer scroll modes / custom step.** `control::drag_split` and `control::clamp_split_offset`
//! already existed, already clamped, and already emitted `dragged` — with
//! **zero non-test callers**. The whole feature was one missing input path, so
//! that is what was added, in `SceneTree::gui_input_stage`.
//!
//! The other three clauses are not covered here:
//!
//! - **AspectRatioContainer** — needs `arrange_aspect` and an entry in
//!   `is_container_class`, both in `control.rs`, which is under another agent's
//!   exclusive lease.
//! - **TabContainer `tabs_position`** — needs `control::tab_rects` and
//!   `tab_at_point`, same leased file.
//!
//! Every assertion below drives the real input path — `push_input` ->
//! `gui_input_stage` -> property write + signal emit — never `drag_split`
//! directly, since "the helper works" was already true and was never the gap.

use std::sync::{Arc, Mutex};

use gdcore::id::ObjectId;
use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::control::{arrange_container, get_position, get_scroll, get_size, get_split_offset};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: f32 = 200.0;
const H: f32 = 100.0;

/// An `HSplitContainer` of `W x H` with two plain Control panes, laid out.
///
/// With `separation` unset (0) and no child minimums, the divider starts at the
/// midpoint — x = 100.
fn split_scene() -> (SceneTree, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let split = tree
        .add_child(root, Node::new("Split", "HSplitContainer"))
        .unwrap();
    let a = tree.add_child(split, Node::new("A", "Control")).unwrap();
    let b = tree.add_child(split, Node::new("B", "Control")).unwrap();
    arrange_container(&mut tree, split, Vector2::new(W, H));
    (tree, split, a, b)
}

fn press(tree: &mut SceneTree, at: Vector2) {
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: at,
    });
}

fn motion(tree: &mut SceneTree, to: Vector2) {
    tree.push_input(&InputEvent::MouseMotion {
        position: to,
        relative: Vector2::ZERO,
    });
}

fn release(tree: &mut SceneTree, at: Vector2) {
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: at,
    });
}

/// Records every `dragged(offset)` payload.
fn watch_dragged(tree: &mut SceneTree, node: NodeId) -> Arc<Mutex<Vec<f64>>> {
    let log: Arc<Mutex<Vec<f64>>> = Arc::default();
    let sink = log.clone();
    tree.connect_signal(
        node,
        "dragged",
        Connection::with_callback(ObjectId::from_raw(91), "on_dragged", move |args| {
            if let Some(Variant::Float(f)) = args.first() {
                sink.lock().unwrap().push(*f);
            }
            Variant::Nil
        }),
    );
    log
}

/// 🔑 THE ACCEPTANCE (clause 1): press the divider, drag 40px right.
#[test]
fn container_split_drag_scroll_modes_and_aspect() {
    let (mut tree, split, a, b) = split_scene();

    assert_eq!(
        get_size(&tree, a),
        Vector2::new(100.0, H),
        "the divider starts at the midpoint"
    );
    let log = watch_dragged(&mut tree, split);

    // The divider occupies the gap after pane A, i.e. x = 100.
    press(&mut tree, Vector2::new(100.0, 50.0));
    motion(&mut tree, Vector2::new(140.0, 50.0));

    assert_eq!(
        get_split_offset(&tree, split),
        40.0,
        "dragging 40px right moves split_offset by 40"
    );
    assert_eq!(
        *log.lock().unwrap(),
        vec![40.0],
        "and emits dragged(40) exactly once"
    );

    // ⚠️ The panes must have been re-laid out. `drag_split` only writes the
    // offset and emits; without an explicit re-sort the divider moves in the
    // data and not on screen.
    assert_eq!(
        get_size(&tree, a),
        Vector2::new(140.0, H),
        "pane A grew to the new divider position"
    );
    assert_eq!(get_position(&tree, b), Vector2::new(140.0, 0.0));
    assert_eq!(
        get_size(&tree, b),
        Vector2::new(60.0, H),
        "and pane B shrank to match"
    );

    release(&mut tree, Vector2::new(140.0, 50.0));
    motion(&mut tree, Vector2::new(180.0, 50.0));
    assert_eq!(
        get_split_offset(&tree, split),
        40.0,
        "motion after release must not keep dragging"
    );
}

/// A press that is not on the divider starts no drag.
///
/// ⚠️ The control for the acceptance. Without it, an implementation that began a
/// drag on ANY press inside the container would pass every assertion above — and
/// would then move the divider whenever a user clicked anywhere in either pane.
#[test]
fn a_press_inside_a_pane_does_not_drag_the_divider() {
    let (mut tree, split, _a, _b) = split_scene();
    let log = watch_dragged(&mut tree, split);

    // Well inside pane A, far from the x = 100 divider.
    press(&mut tree, Vector2::new(20.0, 50.0));
    motion(&mut tree, Vector2::new(60.0, 50.0));

    assert_eq!(
        get_split_offset(&tree, split),
        0.0,
        "a click in a pane must leave the divider alone"
    );
    assert!(log.lock().unwrap().is_empty(), "and must not emit dragged");
}

/// The drag clamps, and the offset keeps tracking the pointer on the way back.
///
/// ⚠️ This is why the grab stores the offset AT PRESS rather than accumulating
/// per-motion deltas. `drag_split` clamps, so an accumulating implementation
/// loses however far the pointer overshot past the clamp, and the divider then
/// lags the cursor by that much when the user drags back.
#[test]
fn dragging_past_the_end_clamps_and_still_tracks_on_the_way_back() {
    let (mut tree, split, a, _b) = split_scene();

    // ⚠️ The assertions below are STAGED, and the staging is the point
    // (pat-s3rrv.173). This test failed with "pane A = 100, expected 200", and
    // that final symptom is consistent with three different broken links: the
    // press not arming the drag, the motion not reaching the container, or the
    // re-layout not applying the new offset. Asserting only the pane width names
    // none of them.
    //
    // Each stage below is checked before the one that depends on it, so the
    // FIRST failure names the broken link.
    press(&mut tree, Vector2::new(100.0, 50.0));
    // (1) The press must ARM the drag. The offset is still 0 here — the grab is
    // recorded, not applied — so this asserts the divider was found at all by
    // checking a later motion can move it, below.
    assert_eq!(
        get_split_offset(&tree, split),
        0.0,
        "a press alone must not move the divider, only grab it"
    );

    // Way past the right edge.
    motion(&mut tree, Vector2::new(1000.0, 50.0));
    let clamped = get_split_offset(&tree, split);

    // (2) The motion must MOVE the divider to its clamp. With separation 0 and
    // two bare Controls the valid range is [0, 200] and the default position is
    // 100, so dragging 900px right clamps the divider to 200 — an offset of
    // exactly 100.
    //
    // ⚠️ This replaces a `clamped <= 100.0` check that passed at the buggy
    // value: the offset stays 0 when the drag never takes effect, and 0 is
    // comfortably "<= 100". An upper bound cannot detect a stuck lower value.
    assert_eq!(
        clamped, 100.0,
        "the drag must move the divider to its clamp: offset is {clamped}. 0 \
         means the drag never took effect at all (press did not arm it, or the \
         motion did not reach the container); a value between 0 and 100 means \
         the clamp is wrong"
    );
    assert_eq!(
        get_size(&tree, a).x,
        W,
        "pane A fills the container at the clamp"
    );

    // Back to 40px right of the grab point: the offset must be 40 again, not
    // 40 short of wherever the clamp stopped.
    motion(&mut tree, Vector2::new(140.0, 50.0));
    assert_eq!(
        get_split_offset(&tree, split),
        40.0,
        "the offset follows the pointer's absolute distance from the grab, so a \
         clamped overshoot is not remembered"
    );
}

// ---------------------------------------------------------------------------
// Clause 2: ScrollContainer scroll modes and custom step
// ---------------------------------------------------------------------------

/// `ScrollContainer(200x100)` holding content 300 tall, so 200px of the content
/// is off-screen and there is somewhere to scroll to.
fn scroll_scene() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scroll = tree
        .add_child(root, Node::new("Scroll", "ScrollContainer"))
        .unwrap();
    gdscene::control::set_position(&mut tree, scroll, Vector2::ZERO);
    gdscene::control::set_size(&mut tree, scroll, Vector2::new(W, H));

    let content = tree
        .add_child(scroll, Node::new("Content", "Control"))
        .unwrap();
    // Patina does not derive a container's minimum from its children, so the
    // content extent is authored — the same accommodation `control_render_test`
    // makes for its scroll fixture.
    if let Some(n) = tree.get_node_mut(content) {
        n.set_property(
            "custom_minimum_size",
            Variant::Vector2(Vector2::new(W, 300.0)),
        );
    }
    arrange_container(&mut tree, scroll, Vector2::new(W, H));
    (tree, scroll)
}

fn wheel_down(tree: &mut SceneTree, at: Vector2) {
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::WheelDown,
        pressed: true,
        position: at,
    });
}

fn set_prop(tree: &mut SceneTree, id: NodeId, key: &str, value: Variant) {
    tree.get_node_mut(id).unwrap().set_property(key, value);
}

/// The default notch is `view.y / 8` — the baseline the two tests below change.
#[test]
fn a_wheel_notch_scrolls_by_an_eighth_of_the_view_by_default() {
    let (mut tree, scroll) = scroll_scene();

    wheel_down(&mut tree, Vector2::new(100.0, 50.0));

    assert_eq!(
        get_scroll(&tree, scroll).y,
        H / 8.0,
        "one notch moves page/8 (PAGE_DIVISOR = 8, scroll_bar.cpp:84)"
    );
}

/// 🔑 `vertical_scroll_mode = SCROLL_MODE_DISABLED` takes the container out of
/// the wheel's path.
#[test]
fn a_disabled_vertical_scroll_mode_does_not_scroll() {
    let (mut tree, scroll) = scroll_scene();
    set_prop(&mut tree, scroll, "vertical_scroll_mode", Variant::Int(0));

    wheel_down(&mut tree, Vector2::new(100.0, 50.0));

    assert_eq!(
        get_scroll(&tree, scroll).y,
        0.0,
        "SCROLL_MODE_DISABLED means the wheel moves nothing"
    );
}

/// ⚠️ The enum-name trap, and the control for the test above. `SHOW_NEVER` (3)
/// hides the scrollbar and **still scrolls** — only `DISABLED` (0) blocks the
/// wheel (`scroll_container.cpp:540-544`). An implementation that treated any
/// non-AUTO mode as "no scrolling" would pass the disabled test and break every
/// container that merely hides its bars.
#[test]
fn show_never_hides_the_bar_but_still_scrolls() {
    let (mut tree, scroll) = scroll_scene();
    set_prop(&mut tree, scroll, "vertical_scroll_mode", Variant::Int(3));

    wheel_down(&mut tree, Vector2::new(100.0, 50.0));

    assert_eq!(
        get_scroll(&tree, scroll).y,
        H / 8.0,
        "SHOW_NEVER is about the bar's visibility, not about scrolling"
    );
}

/// 🔑 `scroll_vertical_custom_step = 50` makes one notch move exactly 50px.
#[test]
fn a_custom_step_sets_the_notch_distance() {
    let (mut tree, scroll) = scroll_scene();
    set_prop(
        &mut tree,
        scroll,
        "scroll_vertical_custom_step",
        Variant::Float(50.0),
    );

    wheel_down(&mut tree, Vector2::new(100.0, 50.0));

    assert_eq!(
        get_scroll(&tree, scroll).y,
        50.0,
        "the custom step replaces page/8 — 12.5 here would mean it was ignored"
    );
}

/// ⚠️ `custom_step >= 0`, not `> 0` (`scroll_bar.cpp:112`). The unset value is
/// **-1.0**, and a step of exactly 0 is a real setting meaning "the wheel moves
/// nothing". Folding 0 in with unset would quietly restore default scrolling to
/// a container that asked for none.
#[test]
fn a_zero_custom_step_is_a_setting_not_an_absence() {
    let (mut tree, scroll) = scroll_scene();
    set_prop(
        &mut tree,
        scroll,
        "scroll_vertical_custom_step",
        Variant::Float(0.0),
    );

    wheel_down(&mut tree, Vector2::new(100.0, 50.0));

    assert_eq!(
        get_scroll(&tree, scroll).y,
        0.0,
        "a custom step of 0 moves nothing; page/8 here would mean 0 was read as \
         'unset'"
    );
}

// ---------------------------------------------------------------------------
// `ScrollContainer.follow_focus`
// ---------------------------------------------------------------------------

/// `ScrollContainer(200x100)` whose 200x300 content holds a focusable band at
/// y = 250 — well below the visible area.
fn follow_focus_scene(follow: bool) -> (SceneTree, NodeId, NodeId) {
    let (mut tree, scroll) = scroll_scene();
    set_prop(&mut tree, scroll, "follow_focus", Variant::Bool(follow));

    let content = tree
        .get_node(scroll)
        .and_then(|n| n.children().first().copied())
        .expect("the fixture's content child");
    let target = tree
        .add_child(content, Node::new("Target", "Control"))
        .unwrap();
    gdscene::control::set_position(&mut tree, target, Vector2::new(0.0, 250.0));
    gdscene::control::set_size(&mut tree, target, Vector2::new(W, 50.0));
    (tree, scroll, target)
}

/// Focusing an off-screen child scrolls it into view.
#[test]
fn follow_focus_scrolls_a_focused_child_into_view() {
    let (mut tree, scroll, target) = follow_focus_scene(true);
    assert_eq!(get_scroll(&tree, scroll).y, 0.0, "starts unscrolled");

    tree.grab_focus(target);

    // `MAX(MIN(near, 0), far - view)` = max(min(250,0), 250+50-100) = 200,
    // which is also the full scroll range (300 content - 100 view).
    assert_eq!(
        get_scroll(&tree, scroll).y,
        200.0,
        "the container scrolled just far enough to bring the child's bottom edge \
         into view"
    );
}

/// ⚠️ The control. Godot's `follow_focus` defaults to **false**, so a container
/// that has not opted in must not move when a descendant takes focus —
/// otherwise every focus change in a scrolled panel yanks the view.
#[test]
fn without_follow_focus_the_container_does_not_move() {
    let (mut tree, scroll, target) = follow_focus_scene(false);

    tree.grab_focus(target);

    assert_eq!(
        get_scroll(&tree, scroll).y,
        0.0,
        "follow_focus is opt-in; an unset container stays where it was"
    );
}

/// A child already inside the view does not move the container.
///
/// ⚠️ `MIN(near, 0)` is what makes this a no-op: without it, focusing a control
/// that is already visible would still scroll it flush to the top edge.
#[test]
fn a_child_already_visible_does_not_scroll() {
    let (mut tree, scroll, _target) = follow_focus_scene(true);
    let content = tree
        .get_node(scroll)
        .and_then(|n| n.children().first().copied())
        .unwrap();
    let near = tree
        .add_child(content, Node::new("Near", "Control"))
        .unwrap();
    gdscene::control::set_position(&mut tree, near, Vector2::new(0.0, 10.0));
    gdscene::control::set_size(&mut tree, near, Vector2::new(W, 20.0));

    tree.grab_focus(near);

    assert_eq!(
        get_scroll(&tree, scroll).y,
        0.0,
        "a control at y=10 in a 100-tall view is already visible"
    );
}

// ---------------------------------------------------------------------------
// Horizontal wheel
// ---------------------------------------------------------------------------

/// `ScrollContainer(200x100)` holding content 400 wide, so there is somewhere
/// to scroll sideways to and nowhere to scroll vertically.
fn wide_scroll_scene() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scroll = tree
        .add_child(root, Node::new("Scroll", "ScrollContainer"))
        .unwrap();
    gdscene::control::set_position(&mut tree, scroll, Vector2::ZERO);
    gdscene::control::set_size(&mut tree, scroll, Vector2::new(W, H));

    let content = tree
        .add_child(scroll, Node::new("Content", "Control"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(content) {
        n.set_property(
            "custom_minimum_size",
            Variant::Vector2(Vector2::new(400.0, H)),
        );
    }
    arrange_container(&mut tree, scroll, Vector2::new(W, H));
    (tree, scroll)
}

fn wheel_right(tree: &mut SceneTree, at: Vector2) {
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::WheelRight,
        pressed: true,
        position: at,
    });
}

/// `WheelRight` scrolls sideways. Before this the whole event was dropped —
/// only `WheelUp`/`WheelDown` were matched at all.
///
/// ⚠️ The notch is sized by the container's WIDTH on this axis. Reusing the
/// height would give the right answer only for a square container, which is
/// exactly the fixture shape that hides the bug.
#[test]
fn a_horizontal_wheel_scrolls_sideways_by_an_eighth_of_the_width() {
    let (mut tree, scroll) = wide_scroll_scene();

    wheel_right(&mut tree, Vector2::new(100.0, 50.0));

    assert_eq!(
        get_scroll(&tree, scroll).x,
        W / 8.0,
        "one horizontal notch moves width/8 = 25; 12.5 would mean the HEIGHT \
         was used"
    );
    assert_eq!(
        get_scroll(&tree, scroll).y,
        0.0,
        "and must not disturb the vertical offset"
    );
}

/// `horizontal_scroll_mode` gates the horizontal axis, independently of the
/// vertical one.
///
/// ⚠️ The axis-independence is the point: a container that disables horizontal
/// scrolling must still scroll vertically, and vice versa. Reading one mode
/// property for both axes would pass a single-axis test and break every
/// container that disables just one.
#[test]
fn horizontal_scroll_mode_gates_only_the_horizontal_axis() {
    let (mut tree, scroll) = wide_scroll_scene();
    set_prop(&mut tree, scroll, "horizontal_scroll_mode", Variant::Int(0));

    wheel_right(&mut tree, Vector2::new(100.0, 50.0));
    assert_eq!(
        get_scroll(&tree, scroll).x,
        0.0,
        "SCROLL_MODE_DISABLED on the horizontal axis stops the sideways wheel"
    );

    // The vertical axis is untouched by that setting. This fixture has no
    // vertical overflow, so assert through the mode that DOES apply: a vertical
    // notch on a taller container still works.
    let (mut tall, tall_scroll) = scroll_scene();
    set_prop(
        &mut tall,
        tall_scroll,
        "horizontal_scroll_mode",
        Variant::Int(0),
    );
    wheel_down(&mut tall, Vector2::new(100.0, 50.0));
    assert_eq!(
        get_scroll(&tall, tall_scroll).y,
        H / 8.0,
        "disabling the horizontal axis must not disable the vertical one"
    );
}
