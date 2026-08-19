//! pat-s3rrv.71 — the acceptance clause nothing covered: a `.tscn` declaring
//! each Control/Window node type **loads with its ClassDB defaults populated**.
//!
//! The bead's two existing test files (`gui_control_classes_registered_with_godot_defaults`
//! and `control_directional_variants_registered`) both assert against ClassDB
//! directly — `class_exists`, the inheritance chain, the registered default
//! values. That proves the registry is right and says nothing about whether a
//! scene actually instantiates one.
//!
//! Those are different failures. `add_packed_scene_to_tree` seeds each node from
//! `class_db::get_property_list` (`packed_scene.rs:1689`), so a class that is
//! registered but whose chain does not resolve — or that resolves to a base with
//! no properties — loads as a bare node carrying nothing, while every
//! ClassDB-only assertion stays green. That is the same registered-but-inert
//! shape this audit keeps turning up, one layer out.
//!
//! ⚠️ The assertions here are derived, not hand-written per class: for each
//! type the test asks ClassDB what it should carry and checks the loaded node
//! against exactly that. Hand-picking 23 expected values would restate the
//! registry rather than test the path between them.

use gdobject::class_db;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// Every type the bead's acceptance names.
const CLASSES: &[&str] = &[
    "AspectRatioContainer",
    "CenterContainer",
    "CheckButton",
    "FlowContainer",
    "HFlowContainer",
    "VFlowContainer",
    "FoldableContainer",
    "LinkButton",
    "NinePatchRect",
    "TextureButton",
    "TextureProgressBar",
    "Slider",
    "HSlider",
    "VSlider",
    "ScrollBar",
    "HScrollBar",
    "VScrollBar",
    "SpinBox",
    "Tree",
    "FileDialog",
    "PopupPanel",
    "HSeparator",
    "VSeparator",
];

/// A `.tscn` with one node of every class above, all children of a root.
fn scene_text() -> String {
    let mut s = String::from("[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n");
    for (i, class) in CLASSES.iter().enumerate() {
        s.push_str(&format!(
            "\n[node name=\"N{i}\" type=\"{class}\" parent=\".\"]\n"
        ));
    }
    s
}

/// Returns the tree and **the instanced scene's root**, which is NOT the tree's
/// root.
///
/// 🔑 THIS DISTINCTION IS THE BUG THIS TEST SHIPPED WITH. `add_packed_scene_to_tree`
/// adds the scene's own root (`Root`, a Node2D) as a child of the parent it is
/// given and returns that node — so `tree.root_id()` has exactly ONE child, the
/// scene root, and the 23 typed nodes hang off THAT. Reading children from
/// `tree.root_id()` therefore found 1 where it wanted 23, and the failure
/// ("the scene must instance one node per declared type") reads like the LOADER
/// dropped 22 nodes rather than like the test looked one level too high.
fn load() -> (SceneTree, NodeId) {
    // ⚠️ Without this every `type=` resolves to nothing: an unpopulated ClassDB
    // makes `get_property_list` return empty for all 23, so the whole test would
    // pass vacuously against a scene that loaded no properties at all.
    //
    // 🔑 `register_all_classes()`, NOT `register_2d_classes()`. The Control side
    // resolves fine under the 2D table alone, but `FileDialog` goes
    // ConfirmationDialog -> AcceptDialog -> Window, and those two are registered
    // in `register_root_window_chain()`, which `register_2d_classes` does not
    // call. Under the narrower call `inheritance_chain("FileDialog")` stops dead
    // at `ConfirmationDialog` — an unregistered base silently severs the chain
    // rather than erroring.
    class_db::register_all_classes();
    let scene = PackedScene::from_tscn(&scene_text()).expect("scene parses");
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let scene_root =
        add_packed_scene_to_tree(&mut tree, tree_root, &scene).expect("scene instances");
    (tree, scene_root)
}

/// **The acceptance clause**: every declared type instantiates carrying the
/// defaults ClassDB says it has.
#[test]
fn every_control_type_loads_from_a_tscn_with_its_classdb_defaults() {
    let (tree, scene_root) = load();
    let children = tree
        .get_node(scene_root)
        .expect("the instanced scene root exists")
        .children()
        .to_vec();
    assert_eq!(
        children.len(),
        CLASSES.len(),
        "the scene must instance one node per declared type",
    );

    let mut checked = 0usize;
    for class in CLASSES {
        // ⚠️ Looked up BY CLASS, not by position. Zipping the child list
        // against `CLASSES` would bake in an assumption about the loader's
        // ordering, and a reordering there would fail this test for a reason
        // that has nothing to do with what it is checking.
        let child = children
            .iter()
            .find(|c| {
                tree.get_node(**c)
                    .map(|n| n.class_name() == *class)
                    .unwrap_or(false)
            })
            .unwrap_or_else(|| panic!("no node of type `{class}` was instanced"));
        let node = tree.get_node(*child).expect("child exists");

        // Ask the registry what this class should carry, then check the node
        // against it. A property with a Nil default carries no information, so
        // it is skipped rather than asserted.
        //
        // 🔑 DEDUPED BY NAME, KEEPING THE LAST. `get_property_list` concatenates
        // the inheritance chain base-first WITHOUT deduping, so an overridden
        // member appears twice with different values — `Range` declares
        // `step = 0.01` and `Slider` overrides it to `1.0`; `Control` and
        // `Window` both declare `size`. `node_with_class_defaults` seeds in that
        // same order, so the node ends up holding the LAST (most-derived) value,
        // which is also what `property_get_revert` reports.
        //
        // Asserting against every entry would therefore fail on the base's value
        // for five of these classes (Slider, HSlider, SpinBox,
        // TextureProgressBar, FileDialog) while the engine is behaving exactly
        // right.
        let mut expected: Vec<gdobject::class_db::PropertyInfo> = Vec::new();
        for prop in class_db::get_property_list(class) {
            if prop.default_value == Variant::Nil {
                continue;
            }
            match expected.iter_mut().find(|e| e.name == prop.name) {
                Some(slot) => *slot = prop,
                None => expected.push(prop),
            }
        }
        assert!(
            !expected.is_empty(),
            "{class} resolves to NO non-nil properties — either it is not \
             registered or its inheritance chain is broken, and a ClassDB-only \
             test cannot tell the difference",
        );

        for prop in expected {
            assert_eq!(
                node.get_property(&prop.name),
                prop.default_value,
                "{class}.{} must load with the ClassDB default {:?}",
                prop.name,
                prop.default_value,
            );
            checked += 1;
        }
    }

    // A floor, so a future change that quietly empties the registry cannot make
    // this test pass by checking nothing.
    assert!(
        checked > 100,
        "expected well over 100 property assertions across {} classes, made \
         {checked} — the registry has probably been emptied",
        CLASSES.len(),
    );
}

/// 🔑 The chain has to actually resolve, not merely exist.
///
/// `class_exists` is true for a class registered with a bogus or missing
/// parent, and `get_property_list` walks the chain — so a broken parent link
/// shows up as a node that loads with only its own handful of properties and
/// none of its base's.
///
/// ⚠️ NOT every class here descends from `Control`. `FileDialog` goes
/// `ConfirmationDialog -> AcceptDialog -> Window` and `PopupPanel` goes
/// `Popup -> Window`, and `Window` is a **Viewport**, not a Control — which is
/// why the bead is titled "Control/Window classes". Asserting `Control` for all
/// 23 would fail against a correct engine, so the invariant is: terminate at
/// `Object`, and pass through Control **or** Window.
#[test]
fn every_inheritance_chain_resolves_to_object() {
    class_db::register_all_classes();

    for class in CLASSES {
        let chain = class_db::inheritance_chain(class);
        assert!(
            chain.iter().any(|c| c == "Control" || c == "Window"),
            "{class}'s chain must pass through Control or Window; got {chain:?}",
        );
        assert_eq!(
            chain.last().map(String::as_str),
            Some("Object"),
            "{class}'s chain must terminate at Object — a missing or bogus \
             parent link stops it early; got {chain:?}",
        );
        assert!(
            chain.len() >= 3,
            "{class} resolves to a suspiciously short chain {chain:?}; a leaf \
             registered directly under Object would pass the checks above",
        );
    }
}

/// The directional variants must not collapse onto their shared base.
///
/// `HSeparator`/`VSeparator`, `HSlider`/`VSlider`, `HScrollBar`/`VScrollBar`
/// and `HFlowContainer`/`VFlowContainer` are separate Godot classes, and a
/// registration that pointed both at the base would still satisfy
/// `class_exists` and still load a node.
#[test]
fn the_directional_variants_keep_their_own_class() {
    let (tree, scene_root) = load();
    let children = tree
        .get_node(scene_root)
        .expect("the instanced scene root exists")
        .children()
        .to_vec();

    for (h, v) in [
        ("HSeparator", "VSeparator"),
        ("HSlider", "VSlider"),
        ("HScrollBar", "VScrollBar"),
        ("HFlowContainer", "VFlowContainer"),
    ] {
        for name in [h, v] {
            let found = children.iter().any(|c| {
                tree.get_node(*c)
                    .map(|n| n.class_name() == name)
                    .unwrap_or(false)
            });
            assert!(found, "{name} must load as its own class, not its base");
        }
    }
}
