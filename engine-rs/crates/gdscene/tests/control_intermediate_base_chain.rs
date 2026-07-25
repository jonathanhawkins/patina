//! pat-0q2kx.62 acceptance: Godot's GUI intermediate bases exist and the leaves
//! inherit through them.
//!
//! Godot ground truth (`extension_api.json`, `scene/gui/{base_button,range,
//! box_container}.h`): `Button <- BaseButton <- Control`,
//! `ProgressBar <- Range <- Control`,
//! `HBoxContainer`/`VBoxContainer <- BoxContainer <- Container`.
//!
//! Patina parented every leaf straight onto `Control`/`Container` and never
//! registered the three intermediates — which is precisely why
//! `gdscene::control` carries a hardcoded `BUTTON_CLASSES` list to recognise a
//! clickable button. This lives in `gdscene` rather than `gdobject` because the
//! acceptance also requires a real `SceneTree` node to answer
//! `is_class("BaseButton")`.

use gdobject::class_db::{class_exists, is_parent_class, register_2d_classes};
use gdscene::control::node_is_base_button;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;

fn registered() {
    register_2d_classes();
}

/// **The acceptance test.**
#[test]
fn control_intermediate_base_chain() {
    registered();

    for base in ["BaseButton", "Range", "BoxContainer"] {
        assert!(class_exists(base), "{base} must be registered");
    }

    // -- the chains the bead names ----------------------------------------
    assert!(is_parent_class("Button", "BaseButton"));
    assert!(
        is_parent_class("CheckBox", "BaseButton"),
        "CheckBox reaches BaseButton through Button"
    );
    assert!(is_parent_class("ProgressBar", "Range"));
    assert!(is_parent_class("HBoxContainer", "BoxContainer"));
    assert!(is_parent_class("VBoxContainer", "BoxContainer"));

    // -- ⚠️ and the chain ABOVE each new base is still intact ---------------
    //
    // `inheritance_chain` resolves by name at query time and stops at the first
    // unregistered name, so inserting a link without registering it would sever
    // these. A severed chain is worse than the missing base this bead fixes.
    assert!(is_parent_class("Button", "Control"));
    assert!(is_parent_class("Button", "Object"));
    assert!(is_parent_class("CheckBox", "Control"));
    assert!(is_parent_class("ProgressBar", "Control"));
    assert!(is_parent_class("ProgressBar", "Object"));
    assert!(is_parent_class("HBoxContainer", "Container"));
    assert!(is_parent_class("HBoxContainer", "Control"));
    assert!(is_parent_class("VBoxContainer", "Container"));
    assert!(is_parent_class("VBoxContainer", "Object"));

    // The bases themselves sit where Godot puts them.
    assert!(is_parent_class("BaseButton", "Control"));
    assert!(is_parent_class("Range", "Control"));
    assert!(is_parent_class("BoxContainer", "Container"));

    // -- a real scene node answers is_class ---------------------------------
    //
    // This is what `control.rs` button routing asks. Before this bead it was
    // false for a Button and only the hardcoded fallback list saved the click.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let button = tree.add_child(root, Node::new("Play", "Button")).unwrap();
    let checkbox = tree.add_child(root, Node::new("Mute", "CheckBox")).unwrap();
    let label = tree.add_child(root, Node::new("Title", "Label")).unwrap();

    assert!(
        tree.get_node(button).unwrap().is_class("BaseButton"),
        "a Button node must report is_class(BaseButton) — the ClassDB path, \
         not the BUTTON_CLASSES fallback"
    );
    assert!(tree.get_node(checkbox).unwrap().is_class("BaseButton"));
    assert!(
        !tree.get_node(label).unwrap().is_class("BaseButton"),
        "a Label is not a button"
    );

    // The routing helper agrees, which is the behaviour the fallback existed to
    // guarantee.
    assert!(node_is_base_button(&tree, button));
    assert!(node_is_base_button(&tree, checkbox));
    assert!(!node_is_base_button(&tree, label));
}

/// The three intermediates are abstract in Godot — categories, not widgets a
/// scene instantiates.
///
/// ⚠️ Asserted via `is_abstract`, not by expecting `instantiate` to fail:
/// Patina's `instantiate` does not honour the abstract flag, so an `is_none()`
/// assertion would fail for an unrelated reason.
#[test]
fn the_intermediate_bases_are_abstract() {
    registered();
    for base in ["BaseButton", "Range", "BoxContainer"] {
        assert!(
            gdobject::class_db::is_abstract(base),
            "{base} is an abstract base in Godot"
        );
    }
    for leaf in ["Button", "ProgressBar", "HBoxContainer", "VBoxContainer"] {
        assert!(
            !gdobject::class_db::is_abstract(leaf),
            "{leaf} stays instantiable"
        );
    }
}

/// The bases carry the properties their subclasses inherit — a `Button` reads
/// `disabled`/`toggle_mode` from `BaseButton`, and a `ProgressBar` reads
/// `min_value`/`max_value` from `Range`.
#[test]
fn the_bases_carry_their_inherited_properties() {
    registered();

    let button_props = gdobject::class_db::get_property_list("Button");
    for p in ["disabled", "toggle_mode", "button_pressed"] {
        assert!(
            button_props.iter().any(|info| info.name == p),
            "Button must inherit `{p}` from BaseButton; got {button_props:?}"
        );
    }

    let bar_props = gdobject::class_db::get_property_list("ProgressBar");
    for p in ["min_value", "max_value", "value", "step"] {
        assert!(
            bar_props.iter().any(|info| info.name == p),
            "ProgressBar must inherit `{p}` from Range; got {bar_props:?}"
        );
    }
}
