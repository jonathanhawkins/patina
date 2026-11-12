//! pat-s3rrv.68: `CheckButton` and `CenterContainer` exist in ClassDB.
//!
//! Both were fully implemented on the gdscene side and invisible to the engine:
//! `CheckButton` is in `BUTTON_CLASSES` and has a draw arm
//! (`"CheckBox" | "CheckButton" =>` in `control_render.rs`) and theme
//! styleboxes; `CenterContainer` is in the container list with a working
//! `arrange_center`. Neither was ever registered, and the editor's Create Node
//! dialog enumerates `class_db::get_class_list()` — so neither node could be
//! added to a scene from the editor at all, and `is_class("Button")` /
//! `is_class("Container")` resolved only through gdscene's hardcoded fallback
//! lists rather than the real inheritance chain.
//!
//! Godot ground truth: `CheckButton : Button` with **no members of its own**
//! (extension_api.json; `scene/gui/check_button.cpp` — everything that differs
//! from CheckBox is theme), and `CenterContainer : Container` with
//! `use_top_left` (bool, default false; `scene/gui/center_container.cpp`).
//!
//! ⚠️ Every test here seeds ClassDB with `register_2d_classes()` first. ClassDB
//! is a process-global that starts EMPTY, so an unseeded test would see
//! `class_exists` answer false for everything and pass vacuously against the
//! hardcoded fallback lists — the exact blind spot that let this gap survive.

use std::sync::{Arc, Mutex};

use gdcore::id::ObjectId;
use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::control::{arrange_container, get_position, get_size, set_custom_minimum_size};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Adds a control of `class` under `parent` at `pos` with `size`.
fn control_at(
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

fn click(tree: &mut SceneTree, at: Vector2) {
    for pressed in [true, false] {
        tree.push_input(&InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed,
            position: at,
        });
    }
}

fn is_pressed(tree: &SceneTree, node: NodeId) -> bool {
    matches!(
        tree.get_node(node)
            .map(|n| n.get_property("button_pressed")),
        Some(Variant::Bool(true))
    )
}

/// Records every `toggled(bool)` payload, in order.
fn watch_toggled(tree: &mut SceneTree, node: NodeId, tag: u64) -> Arc<Mutex<Vec<bool>>> {
    let log: Arc<Mutex<Vec<bool>>> = Arc::default();
    let sink = log.clone();
    tree.connect_signal(
        node,
        "toggled",
        Connection::with_callback(ObjectId::from_raw(tag), "on_toggled", move |args| {
            sink.lock()
                .unwrap()
                .push(matches!(args.first(), Some(Variant::Bool(true))));
            Variant::Nil
        }),
    );
    log
}

/// A `CenterContainer` sized `total` holding one child with a fixed minimum.
fn centered_child(use_top_left: bool, total: Vector2, min: Vector2) -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cc = tree
        .add_child(root, Node::new("CC", "CenterContainer"))
        .expect("CenterContainer");
    if use_top_left {
        if let Some(n) = tree.get_node_mut(cc) {
            n.set_property("use_top_left", Variant::Bool(true));
        }
    }
    let kid = tree
        .add_child(cc, Node::new("Kid", "Control"))
        .expect("child");
    set_custom_minimum_size(&mut tree, kid, min);
    arrange_container(&mut tree, cc, total);
    (tree, cc, kid)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// **The acceptance test.** Registration, the editor's class list, the
/// inheritance chain, layout in both `use_top_left` modes, and that the button
/// still behaves.
#[test]
fn checkbutton_centercontainer_registered_and_behave() {
    gdobject::class_db::register_2d_classes();

    // (1) Both classes exist, under their Godot parents.
    for (cls, parent) in [("CheckButton", "Button"), ("CenterContainer", "Container")] {
        assert!(
            gdobject::class_db::class_exists(cls),
            "{cls} must be registered in ClassDB",
        );
        assert!(
            gdobject::class_db::is_parent_class(cls, parent),
            "{cls} must inherit {parent}",
        );
    }

    // (2) The editor's Create Node dialog enumerates `get_class_list()`, so
    // presence there is exactly what makes the node creatable. Asserting the
    // list rather than driving the dialog keeps this test in gdscene; the
    // dialog reads this same function (create_dialog.rs).
    let classes = gdobject::class_db::get_class_list();
    for cls in ["CheckButton", "CenterContainer"] {
        assert!(
            classes.iter().any(|c| c == cls),
            "{cls} must appear in get_class_list(), which is what the editor's \
             Create Node dialog enumerates — absent here means uncreatable",
        );
    }

    // (3) CheckButton resolves through the ClassDB chain, not just by name.
    // `Node::is_class` short-circuits on an exact name match, so asking for
    // "CheckButton" would pass even with no registration at all; the parent
    // names are what actually exercise inheritance.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let check = control_at(
        &mut tree,
        root,
        "Check",
        "CheckButton",
        Vector2::ZERO,
        Vector2::new(100.0, 40.0),
    );
    let node = tree.get_node(check).expect("CheckButton node");
    for base in ["Button", "BaseButton", "Control"] {
        assert!(
            node.is_class(base),
            "CheckButton must resolve is_class({base}) through ClassDB inheritance",
        );
    }
    assert!(
        !node.is_class("CheckBox"),
        "CheckButton is a SIBLING of CheckBox, not a subclass — if this passes, \
         it was registered under the wrong parent",
    );

    // (4) …and still behaves as a button: one click on a toggle_mode
    // CheckButton flips `button_pressed` and emits `toggled(true)` once.
    if let Some(n) = tree.get_node_mut(check) {
        n.set_property("toggle_mode", Variant::Bool(true));
    }
    let toggles = watch_toggled(&mut tree, check, 68);
    assert!(
        !is_pressed(&tree, check),
        "a fresh CheckButton starts unpressed, or the flip below proves nothing",
    );
    click(&mut tree, Vector2::new(20.0, 20.0));
    assert!(
        is_pressed(&tree, check),
        "one click on a toggle_mode CheckButton must set button_pressed",
    );
    assert_eq!(
        toggles.lock().unwrap().clone(),
        vec![true],
        "and must emit toggled(true) exactly once",
    );

    // (5) CenterContainer centres its child INSIDE the rect by default.
    let total = Vector2::new(200.0, 100.0);
    let min = Vector2::new(40.0, 20.0);
    let (tree, _cc, kid) = centered_child(false, total, min);
    assert_eq!(
        get_size(&tree, kid),
        min,
        "CenterContainer keeps the child at its effective minimum size",
    );
    assert_eq!(
        get_position(&tree, kid),
        Vector2::new((200.0 - 40.0) * 0.5, (100.0 - 20.0) * 0.5),
        "the child must be centred in the container",
    );

    // (6) …and on the container's TOP-LEFT CORNER with `use_top_left`, which
    // means a NEGATIVE origin: the child straddles (0, 0). Clamping to zero
    // here would silently turn the flag into a no-op, so the sign is the
    // assertion that matters.
    let (tree, _cc, kid) = centered_child(true, total, min);
    assert_eq!(
        get_position(&tree, kid),
        Vector2::new(-20.0, -10.0),
        "use_top_left centres the child on the container's top-left corner",
    );
    assert_eq!(
        get_size(&tree, kid),
        min,
        "use_top_left changes placement only, never the child's size",
    );
}

/// A registered `Container` subclass lays out even when it is absent from
/// gdscene's hardcoded list (pat-s3rrv.68).
///
/// `is_container_class_name` used to be a bare name match, so a container that
/// existed in ClassDB but had not been spelled into that list was silently
/// treated as a plain Control and never arranged its children — which is the
/// shape of the bug `CenterContainer` was hiding behind.
#[test]
fn a_classdb_registered_container_subclass_lays_out() {
    gdobject::class_db::register_2d_classes();
    gdobject::class_db::register_class(
        gdobject::class_db::ClassRegistration::new("BespokeCenterContainer")
            .parent("CenterContainer"),
    );

    assert!(
        gdscene::control::is_container_class_name("BespokeCenterContainer"),
        "a ClassDB subclass of Container must be recognised as a container \
         without being added to the hardcoded list",
    );
}

/// The hardcoded fallback list must survive an EMPTY ClassDB (pat-s3rrv.68).
///
/// Plenty of unit tests never call a `register_*_classes()` seeder, so
/// `is_parent_class` answers false for everything there. Deleting the list as
/// "now redundant" would stop those fixtures laying out — it is the documented
/// fallback, not dead code. Names only, so this holds regardless of whether an
/// earlier test in this binary already seeded the global.
#[test]
fn the_hardcoded_container_list_is_still_the_fallback() {
    for cls in [
        "VBoxContainer",
        "HBoxContainer",
        "GridContainer",
        "MarginContainer",
        "CenterContainer",
        "ScrollContainer",
    ] {
        assert!(
            gdscene::control::is_container_class_name(cls),
            "{cls} must be recognised as a container by name alone",
        );
    }
}
