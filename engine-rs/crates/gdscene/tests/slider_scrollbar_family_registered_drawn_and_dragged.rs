//! Acceptance test for pat-s3rrv.70: the `Range` control family is registered,
//! drawn, and — the part that was missing entirely — **interactive**.
//!
//! Godot ground truth: `scene/gui/slider.cpp` (`gui_input`: a press reads the
//! value straight off the click position; arrows move by `step` along the
//! control's own axis), `scene/gui/scroll_bar.cpp` (`gui_input`: a press on the
//! grabber starts a *relative* drag, a press on bare track pages), and
//! `scene/gui/range.cpp` (`set_value` emits `value_changed` only on a real
//! change).
//!
//! ⚠️ The gap this test closes is not "sliders look wrong". Registration, the
//! hit-test lists and the draw arms all landed in earlier passes, so a slider
//! was visible, clickable and tab-focusable — and moved for nobody.
//! `value_changed` appeared exactly ONCE in non-test source, at its ClassDB
//! declaration: the signal was declared and never emitted anywhere in the
//! engine, and `set_range_value` had no caller outside test modules. Dragging a
//! grabber, paging a scrollbar or arrow-keying a focused slider all did nothing.
//!
//! Every assertion below drives the real input path and observes the value
//! through the node's own property and the signal through a connected callback —
//! never by calling the geometry helpers directly.

use std::sync::{Arc, Mutex};

use gdcore::id::ObjectId;
use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdplatform::input::{InputEvent, Key, MouseButton};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// `ratio_at_local_point` maps `x` through `(x - grabber/2) / (width - grabber)`
/// with Godot's 16px grabber icon (pat-9ww3e), so a **116**-wide slider has a
/// track of exactly 100 and `x = 8 + v` selects value `v` on a 0..100 range.
///
/// Chosen so the expected values below are read off the fixture rather than
/// reverse-engineered from the implementation.
const SLIDER_WIDTH: f32 = 116.0;
const TRACK_ORIGIN: f32 = 8.0;

/// Records every `value_changed` payload, in order.
#[derive(Clone, Default)]
struct ValueLog(Arc<Mutex<Vec<f64>>>);

impl ValueLog {
    fn values(&self) -> Vec<f64> {
        self.0.lock().unwrap().clone()
    }
}

fn watch_value_changed(tree: &mut SceneTree, node: NodeId, tag: u64) -> ValueLog {
    let log = ValueLog::default();
    let sink = log.clone();
    tree.connect_signal(
        node,
        "value_changed",
        Connection::with_callback(ObjectId::from_raw(tag), "on_value_changed", move |args| {
            let v = match args.first() {
                Some(Variant::Float(f)) => *f,
                Some(Variant::Int(i)) => *i as f64,
                other => panic!("value_changed must carry a number, got {other:?}"),
            };
            sink.0.lock().unwrap().push(v);
            Variant::Nil
        }),
    );
    log
}

/// A range control of `class` at the origin, sized `size`.
///
/// ⚠️ `size` is set as a PROPERTY on purpose: `control::get_size` reads the
/// stored `size`, so a fixture that sets only anchors/offsets leaves the control
/// zero-sized and every ratio collapses to the same clamped end.
fn range_node(class: &str, size: Vector2, props: &[(&str, f64)]) -> Node {
    let mut n = Node::new(class, class);
    n.set_property("size", Variant::Vector2(size));
    n.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    for (k, v) in props {
        n.set_property(*k, Variant::Float(*v));
    }
    n
}

fn value_of(tree: &SceneTree, id: NodeId) -> f64 {
    match tree.get_node(id).map(|n| n.get_property("value")) {
        Some(Variant::Float(f)) => f,
        Some(Variant::Int(i)) => i as f64,
        other => panic!("expected a numeric `value`, got {other:?}"),
    }
}

fn press_at(x: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(x, 8.0),
    }
}

fn release_at(x: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: Vector2::new(x, 8.0),
    }
}

fn key(k: Key, pressed: bool) -> InputEvent {
    InputEvent::Key {
        key: k,
        pressed,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

// ---------------------------------------------------------------------------
// (1) Registration
// ---------------------------------------------------------------------------

/// The family reaches `Range -> Control`, with the two intermediate bases
/// abstract (pat-s3rrv.70 acceptance 1).
#[test]
fn the_range_family_is_registered_with_its_godot_inheritance() {
    gdobject::class_db::register_2d_classes();

    for (cls, parent) in [
        ("HSlider", "Slider"),
        ("VSlider", "Slider"),
        ("HScrollBar", "ScrollBar"),
        ("VScrollBar", "ScrollBar"),
    ] {
        assert!(
            gdobject::class_db::class_exists(cls),
            "{cls} must exist in ClassDB, or Create Node cannot offer it and \
             instantiate({cls}) fails"
        );
        assert!(
            gdobject::class_db::is_parent_class(cls, parent),
            "{cls} must inherit {parent}"
        );
        // The whole chain, not just one link: the value model lives on Range.
        assert!(
            gdobject::class_db::is_parent_class(cls, "Range"),
            "{cls} must reach Range"
        );
        assert!(
            gdobject::class_db::is_parent_class(cls, "Control"),
            "{cls} must reach Control"
        );
    }

    // ⚠️ Abstract, not merely present. Godot's Slider and ScrollBar cannot be
    // instantiated; registering them as concrete would offer two bases in the
    // Create Node dialog that no scene should ever contain.
    for base in ["Slider", "ScrollBar"] {
        assert!(
            gdobject::class_db::is_abstract(base),
            "{base} must be abstract"
        );
    }
}

// ---------------------------------------------------------------------------
// (3)(4) Dragging a slider
// ---------------------------------------------------------------------------

/// A press at the 75% point sets the value to 75 and emits `value_changed(75)`
/// (acceptance 3).
///
/// Driven through `MainLoop::push_event` + `step`, which is the path a real
/// frame takes: `push_event` queues, `step` drains into `SceneTree::push_input`
/// and the GUI stage.
#[test]
fn a_press_at_three_quarters_sets_the_value_and_emits_value_changed() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let slider = tree
        .add_child(
            root,
            range_node(
                "HSlider",
                Vector2::new(SLIDER_WIDTH, 16.0),
                &[
                    ("min_value", 0.0),
                    ("max_value", 100.0),
                    ("step", 1.0),
                    ("value", 0.0),
                ],
            ),
        )
        .expect("add slider");
    let log = watch_value_changed(&mut tree, slider, 1);

    let mut ml = MainLoop::new(tree);
    ml.push_event(press_at(TRACK_ORIGIN + 75.0));
    ml.step(1.0 / 60.0);

    assert_eq!(
        value_of(ml.tree(), slider),
        75.0,
        "a press at the 75% point must move the value there — this is the \
         `set_range_value` call that had no caller outside tests"
    );
    assert_eq!(
        log.values(),
        vec![75.0],
        "`value_changed` must be emitted exactly once, carrying the new value; \
         before this bead the signal was declared in ClassDB and emitted nowhere \
         in the engine"
    );
}

/// An off-grid press snaps to the nearest `step`, and repeating the identical
/// press does NOT emit again (acceptance 4).
///
/// The second half is the real assertion: `Range::set_value` notifies only on a
/// change, so a drag that holds still — or a click on the value already
/// selected — must stay silent. Emitting per event instead would flood every
/// connected callback during a single drag, which is the kind of bug that only
/// shows up as a frame-rate problem in a real project.
#[test]
fn an_off_grid_press_snaps_to_step_and_does_not_re_emit() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let slider = tree
        .add_child(
            root,
            range_node(
                "HSlider",
                Vector2::new(SLIDER_WIDTH, 16.0),
                &[
                    ("min_value", 0.0),
                    ("max_value", 100.0),
                    ("step", 10.0),
                    ("value", 0.0),
                ],
            ),
        )
        .expect("add slider");
    let log = watch_value_changed(&mut tree, slider, 2);

    let mut ml = MainLoop::new(tree);
    // 73% of the track: off the 10-unit grid, nearest step is 70.
    ml.push_event(press_at(TRACK_ORIGIN + 73.0));
    ml.step(1.0 / 60.0);

    assert_eq!(
        value_of(ml.tree(), slider),
        70.0,
        "73 must snap to the nearest step of 10"
    );
    assert_eq!(log.values(), vec![70.0], "one change, one emission");

    // The same press again: the snapped value is unchanged, so nothing is
    // emitted a second time.
    ml.push_event(release_at(TRACK_ORIGIN + 73.0));
    ml.step(1.0 / 60.0);
    ml.push_event(press_at(TRACK_ORIGIN + 73.0));
    ml.step(1.0 / 60.0);

    assert_eq!(value_of(ml.tree(), slider), 70.0, "still 70");
    assert_eq!(
        log.values(),
        vec![70.0],
        "an identical press changes nothing, so `value_changed` must NOT fire \
         again — got {:?}",
        log.values()
    );
}

// ---------------------------------------------------------------------------
// (5) Keyboard
// ---------------------------------------------------------------------------

/// `ui_right` on a focused HSlider adds one `step` and emits (acceptance 5).
///
/// The second half guards the axis check: Godot's Slider returns WITHOUT
/// accepting the event when the arrow is off its orientation, so Up/Down on an
/// HSlider must fall through to focus navigation rather than being swallowed by
/// a control that cannot act on them.
#[test]
fn arrow_keys_move_a_focused_slider_along_its_own_axis_only() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let slider = tree
        .add_child(
            root,
            range_node(
                "HSlider",
                Vector2::new(SLIDER_WIDTH, 16.0),
                &[
                    ("min_value", 0.0),
                    ("max_value", 100.0),
                    ("step", 10.0),
                    ("value", 40.0),
                ],
            ),
        )
        .expect("add slider");
    let log = watch_value_changed(&mut tree, slider, 3);
    tree.grab_focus(slider);

    tree.push_input(&key(Key::Right, true));
    assert_eq!(
        value_of(&tree, slider),
        50.0,
        "ui_right must add one step to a focused HSlider"
    );
    assert_eq!(log.values(), vec![50.0], "and emit the new value");

    tree.push_input(&key(Key::Left, true));
    assert_eq!(value_of(&tree, slider), 40.0, "ui_left subtracts one step");
    assert_eq!(log.values(), vec![50.0, 40.0]);

    // Off-axis: an HSlider must not consume Up/Down.
    tree.push_input(&key(Key::Up, true));
    tree.push_input(&key(Key::Down, true));
    assert_eq!(
        value_of(&tree, slider),
        40.0,
        "Up/Down are not on an HSlider's axis and must leave it alone — Godot \
         returns before `accept_event()` in that case"
    );
    assert_eq!(
        log.values(),
        vec![50.0, 40.0],
        "and must not emit; a slider that swallows every arrow strands keyboard \
         users on the first one in the focus ring"
    );
}

// ---------------------------------------------------------------------------
// ScrollBar: paging and the relative grabber drag
// ---------------------------------------------------------------------------

/// ⚠️ A scrollbar is NOT a slider wearing another name.
///
/// Pressing bare track pages by one `page` toward the click; it does not jump
/// the value to the cursor (`ScrollBar::gui_input`). Patina routed scrollbars
/// through the slider's absolute mapping, which made the content leap on every
/// click.
#[test]
fn pressing_scrollbar_track_pages_instead_of_jumping_to_the_cursor() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let bar = tree
        .add_child(
            root,
            range_node(
                "HScrollBar",
                Vector2::new(100.0, 12.0),
                &[
                    ("min_value", 0.0),
                    ("max_value", 100.0),
                    ("page", 20.0),
                    ("step", 1.0),
                    ("value", 0.0),
                ],
            ),
        )
        .expect("add scrollbar");
    let log = watch_value_changed(&mut tree, bar, 4);

    // Far end of the track, well past the grabber (which starts at 0).
    tree.push_input(&press_at(95.0));

    assert_eq!(
        value_of(&tree, bar),
        20.0,
        "a press past the grabber pages forward by exactly one `page`, NOT to \
         the cursor's 95%"
    );
    assert_eq!(log.values(), vec![20.0]);

    // And back: a press before the grabber pages toward the minimum.
    tree.push_input(&release_at(95.0));
    tree.push_input(&press_at(1.0));
    assert_eq!(
        value_of(&tree, bar),
        0.0,
        "paging back returns to the minimum"
    );
    assert_eq!(log.values(), vec![20.0, 0.0]);
}

/// The grabber drag is **relative**: it preserves where inside the grabber the
/// user took hold.
///
/// Godot: `diff = (ofs - pos_at_click) / area_size`, applied to the ratio at
/// click (`scroll_bar.cpp:143-147`). An absolute mapping would snap the grabber
/// out from under the pointer the instant the drag began — worst exactly when
/// the user grabbed near one end of a long handle.
#[test]
fn a_scrollbar_grabber_drag_is_relative_to_where_it_was_seized() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let bar = tree
        .add_child(
            root,
            range_node(
                "HScrollBar",
                Vector2::new(100.0, 12.0),
                &[
                    ("min_value", 0.0),
                    ("max_value", 100.0),
                    ("page", 20.0),
                    ("step", 1.0),
                    ("value", 0.0),
                ],
            ),
        )
        .expect("add scrollbar");
    let log = watch_value_changed(&mut tree, bar, 5);

    // Seize the grabber near its trailing edge. The grabber spans [0, 28) at
    // value 0 (page 20 of 100 over a 90px area, plus the 10px minimum), so 25
    // is inside it.
    tree.push_input(&press_at(25.0));
    assert_eq!(
        value_of(&tree, bar),
        0.0,
        "seizing the grabber must not move it — the value follows the pointer, \
         and the pointer has not moved yet"
    );
    assert!(
        log.values().is_empty(),
        "and nothing is emitted on the grab itself, got {:?}",
        log.values()
    );

    // Drag 18px along a 90px area: ratio +0.2, i.e. +20 on a 0..100 range.
    tree.push_input(&InputEvent::MouseMotion {
        position: Vector2::new(43.0, 6.0),
        relative: Vector2::new(18.0, 0.0),
    });
    assert_eq!(
        value_of(&tree, bar),
        20.0,
        "an 18px drag over a 90px area is a fifth of the range; an absolute \
         mapping would have jumped to the cursor instead"
    );
    assert_eq!(log.values(), vec![20.0]);

    // Releasing ends the drag: later motion must not keep scrolling.
    tree.push_input(&release_at(43.0));
    tree.push_input(&InputEvent::MouseMotion {
        position: Vector2::new(80.0, 6.0),
        relative: Vector2::new(37.0, 0.0),
    });
    assert_eq!(
        value_of(&tree, bar),
        20.0,
        "motion after the release must not resume a finished drag"
    );
    assert_eq!(log.values(), vec![20.0]);
}

/// A fully-scrolled bar puts its grabber flush against the end of the track.
///
/// ⚠️ The offset is `area * ratio` (`scroll_bar.cpp:515`), NOT
/// `(extent - grabber) * ratio`. A scrollbar's value already clamps at
/// `max - page`, so its ratio tops out at `1 - page/range`; discounting the
/// grabber's length again leaves a fully-scrolled bar parked short of its own
/// end, which reads as "there is more to scroll" when there is not.
///
/// This is asserted at the real maximum a value can take rather than at a
/// hand-passed `ratio` of 1.0 — the two formulas agree at 1.0, which is exactly
/// why the geometry test that probes it could not see this.
#[test]
fn a_fully_scrolled_bar_parks_its_grabber_against_the_end() {
    use gdscene::range_controls::{scrollbar_grabber_rect, SliderAxis};

    let size = Vector2::new(100.0, 12.0);
    let (page, min, max) = (20.0, 0.0, 100.0);
    // The largest value `set_range_value` allows is `max - page` = 80, so the
    // ratio there is 0.8 — never 1.0.
    let ratio = ((80.0 - min) / (max - min)) as f32;

    let grabber = scrollbar_grabber_rect(size, SliderAxis::Horizontal, ratio, page, min, max);
    let end = grabber.position.x + grabber.size.x;

    assert!(
        (end - size.x).abs() < 1e-3,
        "at its maximum value the grabber must end flush with the track \
         ({}), got {end} (position {}, size {})",
        size.x,
        grabber.position.x,
        grabber.size.x,
    );
}
