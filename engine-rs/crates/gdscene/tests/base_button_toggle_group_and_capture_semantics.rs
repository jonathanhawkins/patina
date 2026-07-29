//! Acceptance test for pat-s3rrv.12: `BaseButton` toggle / action-mode /
//! button-mask / button-group semantics, and pointer capture on press.
//!
//! Godot ground truth: `scene/gui/base_button.cpp` — `button_mask` defaults to
//! `MOUSE_BUTTON_MASK_LEFT` (any other button is ignored outright),
//! `action_mode` picks which edge fires, `toggle_mode` flips `button_pressed`
//! and emits `toggled(bool)`, and a `ButtonGroup` makes toggles mutually
//! exclusive. Separately `Viewport` captures `gui.mouse_focus` on press
//! (`viewport.cpp:1916`) and routes later motion and the release to THAT control
//! (`viewport.cpp:2070-2075`), which is what makes drag-off / drag-back work.
//!
//! Patina registered every one of those properties in ClassDB and drew
//! `button_pressed` (`control_render.rs:1469`), but nothing ever wrote them:
//! `gui_input_stage` emitted `button_down` / `button_up` / `pressed` for ANY
//! mouse button, had no toggle, no action_mode, and dropped its press target on
//! release without capturing motion. `toggled`, `button_group` and
//! `keep_pressed_outside` had zero writers workspace-wide.
//!
//! Every assertion below drives the real input path —
//! `SceneTree::push_input` -> `gui_input_stage` -> property write + signal emit
//! — and observes signals through connected callbacks, never by calling the
//! helpers directly.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gdcore::id::ObjectId;
use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Records every `toggled(bool)` payload the button emits, in order.
#[derive(Clone, Default)]
struct ToggleLog(Arc<Mutex<Vec<bool>>>);

impl ToggleLog {
    fn values(&self) -> Vec<bool> {
        self.0.lock().unwrap().clone()
    }
}

/// Connects a recorder to `toggled` that pushes each emission's bool payload.
///
/// `tag` only has to be unique per connection — the connection carries the
/// callback, so the target object id is never dispatched against.
fn watch_toggled(tree: &mut SceneTree, node: NodeId, tag: u64) -> ToggleLog {
    let log = ToggleLog::default();
    let sink = log.clone();
    tree.connect_signal(
        node,
        "toggled",
        Connection::with_callback(ObjectId::from_raw(tag), "on_toggled", move |args| {
            let on = matches!(args.first(), Some(Variant::Bool(true)));
            sink.0.lock().unwrap().push(on);
            Variant::Nil
        }),
    );
    log
}

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

fn button_at(
    tree: &mut SceneTree,
    parent: NodeId,
    name: &str,
    class: &str,
    pos: Vector2,
    size: Vector2,
) -> NodeId {
    let id = tree.add_child(parent, Node::new(name, class)).unwrap();
    if let Some(n) = tree.get_node_mut(id) {
        n.set_property("position", Variant::Vector2(pos));
        n.set_property("size", Variant::Vector2(size));
    }
    id
}

fn press(tree: &mut SceneTree, button: MouseButton, at: Vector2) {
    tree.push_input(&InputEvent::MouseButton {
        button,
        pressed: true,
        position: at,
    });
}

fn release(tree: &mut SceneTree, button: MouseButton, at: Vector2) {
    tree.push_input(&InputEvent::MouseButton {
        button,
        pressed: false,
        position: at,
    });
}

fn click(tree: &mut SceneTree, at: Vector2) {
    press(tree, MouseButton::Left, at);
    release(tree, MouseButton::Left, at);
}

fn is_pressed(tree: &SceneTree, node: NodeId) -> bool {
    matches!(
        tree.get_node(node)
            .map(|n| n.get_property("button_pressed")),
        Some(Variant::Bool(true))
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// **The acceptance test.** Every clause the bead names, on the real input path.
#[test]
fn base_button_toggle_group_and_capture_semantics() {
    // -- (a) a toggle_mode CheckBox flips and reports both edges ------------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let check = button_at(
        &mut tree,
        root,
        "Check",
        "CheckBox",
        Vector2::new(0.0, 0.0),
        Vector2::new(100.0, 40.0),
    );
    if let Some(n) = tree.get_node_mut(check) {
        n.set_property("toggle_mode", Variant::Bool(true));
    }
    let toggles = watch_toggled(&mut tree, check, 11);

    assert!(
        !is_pressed(&tree, check),
        "a fresh CheckBox starts unpressed, or the flip below proves nothing"
    );
    click(&mut tree, Vector2::new(20.0, 20.0));
    assert!(
        is_pressed(&tree, check),
        "one click on a toggle_mode CheckBox must set button_pressed"
    );
    assert_eq!(
        toggles.values(),
        vec![true],
        "and must emit toggled(true) exactly once"
    );

    click(&mut tree, Vector2::new(20.0, 20.0));
    assert!(
        !is_pressed(&tree, check),
        "a second click must un-toggle it"
    );
    assert_eq!(
        toggles.values(),
        vec![true, false],
        "and report toggled(false); got {:?}",
        toggles.values()
    );

    // -- (b) a right-click on a Button emits nothing ------------------------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let btn = button_at(
        &mut tree,
        root,
        "Btn",
        "Button",
        Vector2::new(0.0, 0.0),
        Vector2::new(100.0, 40.0),
    );
    let pressed_n = count_signal(&mut tree, btn, "pressed", 15);
    let down_n = count_signal(&mut tree, btn, "button_down", 16);

    press(&mut tree, MouseButton::Right, Vector2::new(20.0, 20.0));
    release(&mut tree, MouseButton::Right, Vector2::new(20.0, 20.0));
    assert_eq!(
        pressed_n.load(Ordering::SeqCst),
        0,
        "button_mask defaults to LEFT, so a right-click must not fire `pressed`"
    );
    assert_eq!(
        down_n.load(Ordering::SeqCst),
        0,
        "a masked-out button is ignored entirely — not even button_down"
    );
    // The same button still works with the left button, so (b) is not passing
    // because the button is inert.
    click(&mut tree, Vector2::new(20.0, 20.0));
    assert_eq!(
        pressed_n.load(Ordering::SeqCst),
        1,
        "a LEFT click on the very same button must still fire `pressed`"
    );

    // -- (c) action_mode = BUTTON_PRESS fires on the down edge --------------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let btn = button_at(
        &mut tree,
        root,
        "Btn",
        "Button",
        Vector2::new(0.0, 0.0),
        Vector2::new(100.0, 40.0),
    );
    if let Some(n) = tree.get_node_mut(btn) {
        n.set_property("action_mode", Variant::Int(0)); // ACTION_MODE_BUTTON_PRESS
    }
    let pressed_n = count_signal(&mut tree, btn, "pressed", 17);
    press(&mut tree, MouseButton::Left, Vector2::new(20.0, 20.0));
    assert_eq!(
        pressed_n.load(Ordering::SeqCst),
        1,
        "action_mode=BUTTON_PRESS must fire `pressed` on the DOWN edge"
    );
    release(&mut tree, MouseButton::Left, Vector2::new(20.0, 20.0));
    assert_eq!(
        pressed_n.load(Ordering::SeqCst),
        1,
        "and must not fire a second time on the release"
    );

    // -- (d) press, drag off, release outside -------------------------------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let btn = button_at(
        &mut tree,
        root,
        "Btn",
        "Button",
        Vector2::new(0.0, 0.0),
        Vector2::new(100.0, 40.0),
    );
    let pressed_n = count_signal(&mut tree, btn, "pressed", 18);
    let up_n = count_signal(&mut tree, btn, "button_up", 19);
    // `gui_input` is the capture-sensitive one: with the pointer outside the
    // rect these motions only reach the button if it holds mouse focus.
    let gui_n = count_signal(&mut tree, btn, "gui_input", 20);

    press(&mut tree, MouseButton::Left, Vector2::new(20.0, 20.0));
    let after_press = gui_n.load(Ordering::SeqCst);
    for x in [150.0f32, 200.0, 250.0] {
        tree.push_input(&InputEvent::MouseMotion {
            position: Vector2::new(x, 200.0),
            relative: Vector2::ZERO,
        });
    }
    assert_eq!(
        gui_n.load(Ordering::SeqCst) - after_press,
        3,
        "all 3 motions outside the rect must be delivered to the PRESSED control \
         (Godot's gui.mouse_focus capture) — without capture they hit nothing"
    );
    release(&mut tree, MouseButton::Left, Vector2::new(250.0, 200.0));
    assert_eq!(
        up_n.load(Ordering::SeqCst),
        1,
        "releasing outside still reports the physical release as button_up"
    );
    assert_eq!(
        pressed_n.load(Ordering::SeqCst),
        0,
        "but the ACTION must not fire — the release landed outside the rect"
    );

    // -- (e) two CheckBoxes sharing a button_group are mutually exclusive ---
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = button_at(
        &mut tree,
        root,
        "A",
        "CheckBox",
        Vector2::new(0.0, 0.0),
        Vector2::new(80.0, 40.0),
    );
    let b = button_at(
        &mut tree,
        root,
        "B",
        "CheckBox",
        Vector2::new(0.0, 100.0),
        Vector2::new(80.0, 40.0),
    );
    for id in [a, b] {
        if let Some(n) = tree.get_node_mut(id) {
            n.set_property("toggle_mode", Variant::Bool(true));
            // Both name the SAME group resource, which is the comparison Godot's
            // `Ref<ButtonGroup>` equality makes.
            n.set_property(
                "button_group",
                Variant::String("SubResource(\"Group_1\")".to_string()),
            );
        }
    }
    let a_toggles = watch_toggled(&mut tree, a, 12);
    let b_toggles = watch_toggled(&mut tree, b, 13);

    click(&mut tree, Vector2::new(20.0, 20.0)); // A
    assert!(is_pressed(&tree, a) && !is_pressed(&tree, b), "A on, B off");

    click(&mut tree, Vector2::new(20.0, 120.0)); // B
    assert!(
        is_pressed(&tree, b),
        "clicking B must turn it on: a={} b={}",
        is_pressed(&tree, a),
        is_pressed(&tree, b)
    );
    assert!(
        !is_pressed(&tree, a),
        "and must turn A off — a button_group is mutually exclusive"
    );
    assert_eq!(
        a_toggles.values(),
        vec![true, false],
        "A must report being turned off by the group, not just go quiet"
    );
    assert_eq!(b_toggles.values(), vec![true], "B reports being turned on");

    // Clicking the already-on member keeps it on (Godot keeps one member of a
    // group active) and emits no further toggles.
    click(&mut tree, Vector2::new(20.0, 120.0));
    assert!(
        is_pressed(&tree, b),
        "re-clicking the active group member must not un-press it"
    );
    assert_eq!(
        b_toggles.values(),
        vec![true],
        "and must not emit a redundant toggled: {:?}",
        b_toggles.values()
    );
}

/// `keep_pressed_outside` waives the inside-on-release requirement, so the very
/// same drag-off gesture DOES fire the action.
#[test]
fn keep_pressed_outside_lets_a_release_outside_still_fire() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let btn = button_at(
        &mut tree,
        root,
        "Btn",
        "Button",
        Vector2::new(0.0, 0.0),
        Vector2::new(100.0, 40.0),
    );
    if let Some(n) = tree.get_node_mut(btn) {
        n.set_property("keep_pressed_outside", Variant::Bool(true));
    }
    let pressed_n = count_signal(&mut tree, btn, "pressed", 21);

    press(&mut tree, MouseButton::Left, Vector2::new(20.0, 20.0));
    release(&mut tree, MouseButton::Left, Vector2::new(250.0, 200.0));
    assert_eq!(
        pressed_n.load(Ordering::SeqCst),
        1,
        "with keep_pressed_outside the action fires even though the release \
         landed outside the rect"
    );
}

/// Dragging off and back ON again still fires — the press survives the excursion
/// because the control held capture the whole time.
#[test]
fn drag_off_and_back_on_still_fires_the_action() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let btn = button_at(
        &mut tree,
        root,
        "Btn",
        "Button",
        Vector2::new(0.0, 0.0),
        Vector2::new(100.0, 40.0),
    );
    let pressed_n = count_signal(&mut tree, btn, "pressed", 22);

    press(&mut tree, MouseButton::Left, Vector2::new(20.0, 20.0));
    tree.push_input(&InputEvent::MouseMotion {
        position: Vector2::new(300.0, 300.0),
        relative: Vector2::ZERO,
    });
    tree.push_input(&InputEvent::MouseMotion {
        position: Vector2::new(20.0, 20.0),
        relative: Vector2::ZERO,
    });
    release(&mut tree, MouseButton::Left, Vector2::new(20.0, 20.0));
    assert_eq!(
        pressed_n.load(Ordering::SeqCst),
        1,
        "a drag that returns to the button before release is still a click"
    );
}

/// A disabled toggle button neither flips nor emits — the mask/disabled guards
/// run before any state change.
#[test]
fn a_disabled_toggle_button_does_not_flip() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let check = button_at(
        &mut tree,
        root,
        "Check",
        "CheckBox",
        Vector2::new(0.0, 0.0),
        Vector2::new(100.0, 40.0),
    );
    if let Some(n) = tree.get_node_mut(check) {
        n.set_property("toggle_mode", Variant::Bool(true));
        n.set_property("disabled", Variant::Bool(true));
    }
    let toggles = watch_toggled(&mut tree, check, 14);
    click(&mut tree, Vector2::new(20.0, 20.0));
    assert!(
        !is_pressed(&tree, check),
        "a disabled button must not toggle"
    );
    assert!(
        toggles.values().is_empty(),
        "and must not emit toggled: {:?}",
        toggles.values()
    );
}
