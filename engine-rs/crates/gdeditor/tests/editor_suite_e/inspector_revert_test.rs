//! pat-9eee6: revert-to-default affordance.
//!
//! A property whose value differs from its object/class default exposes a
//! revert affordance; activating it restores the default value and clears the
//! override. Exercises the public `InspectorPanel` revert API end to end.

use gdeditor::inspector::{InspectorPanel, PropertyDefaults};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

#[test]
fn inspector_revert_to_default() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("Sprite", "Node2D")).unwrap();

    // Node2D class defaults (e.g. rotation defaults to 0.0).
    let defaults = PropertyDefaults::node2d_defaults();

    let mut panel = InspectorPanel::new();
    panel.inspect(id);

    // A property equal to its default shows no revert affordance.
    panel.set_property(&mut tree, "rotation", Variant::Float(0.0));
    assert!(
        !panel.is_property_modified(&tree, "rotation", &defaults),
        "a property equal to its default exposes no revert affordance"
    );

    // Overriding the property so it differs from the default surfaces the
    // revert affordance.
    panel.set_property(&mut tree, "rotation", Variant::Float(1.5));
    assert!(
        panel.is_property_modified(&tree, "rotation", &defaults),
        "a property differing from its default exposes the revert affordance"
    );

    // Activating revert restores the default value and clears the override.
    let previous = panel.revert_to_default(&mut tree, "rotation", &defaults);
    assert_eq!(
        previous,
        Variant::Float(1.5),
        "revert returns the previous (overridden) value"
    );
    assert_eq!(
        panel.get_property(&tree, "rotation"),
        Variant::Float(0.0),
        "revert restores the class default value"
    );
    assert!(
        !panel.is_property_modified(&tree, "rotation", &defaults),
        "after revert the property matches its default and the affordance disappears"
    );
}
