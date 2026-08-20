//! Inline editing of an embedded resource property's sub-properties.
//!
//! Godot lets a resource-typed property (e.g. a `Sprite2D`'s `material`) be
//! expanded inline so its sub-properties are editable without navigating into
//! the sub-resource. This module lists an embedded resource's sub-properties
//! and commits edits to them, persisting the change back onto the parent node's
//! resource value.

use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdvariant::Variant;

/// Expands a resource-typed property: returns its sub-properties as
/// `(name, value)` pairs sorted by name, or `None` if `property` on `node` is
/// not a resource (so it cannot be expanded inline).
pub fn expand_subresource(
    tree: &SceneTree,
    node: NodeId,
    property: &str,
) -> Option<Vec<(String, Variant)>> {
    let node = tree.get_node(node)?;
    match node.get_property(property) {
        Variant::Resource(res) => {
            let mut subs: Vec<(String, Variant)> = res
                .properties
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            subs.sort_by(|a, b| a.0.cmp(&b.0));
            Some(subs)
        }
        _ => None,
    }
}

/// Reads a single sub-property of an embedded resource property, or `None` if
/// the property is not a resource or has no such sub-property.
pub fn subresource_property(
    tree: &SceneTree,
    node: NodeId,
    property: &str,
    sub_property: &str,
) -> Option<Variant> {
    let node = tree.get_node(node)?;
    match node.get_property(property) {
        Variant::Resource(res) => res.properties.get(sub_property).cloned(),
        _ => None,
    }
}

/// Commits an inline edit to a sub-property of an embedded resource: updates the
/// resource's sub-property to `value` and writes the resource back onto `node`
/// so the change persists. Returns `false` if `property` on `node` is not a
/// resource (or the node is missing).
pub fn commit_subresource_property(
    tree: &mut SceneTree,
    node: NodeId,
    property: &str,
    sub_property: &str,
    value: Variant,
) -> bool {
    let node = match tree.get_node_mut(node) {
        Some(n) => n,
        None => return false,
    };
    let mut resource = match node.get_property(property) {
        Variant::Resource(res) => *res,
        _ => return false,
    };
    resource.properties.insert(sub_property.to_string(), value);
    node.set_property(property, Variant::Resource(Box::new(resource)));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;
    use gdvariant::ResourceRef;
    use std::collections::HashMap;

    fn material_with_subprops() -> ResourceRef {
        let mut properties = HashMap::new();
        properties.insert("albedo".to_string(), Variant::Int(0));
        properties.insert("roughness".to_string(), Variant::Float(0.5));
        ResourceRef {
            path: "res://m.tres".to_string(),
            class_name: "Material".to_string(),
            properties,
        }
    }

    /// Acceptance (pat-yb478): expanding an embedded resource property shows its
    /// editable sub-properties inline, and edits persist to the sub-resource.
    /// 🔑 **This module does NOT understand the representation real scenes use.**
    ///
    /// An inline resource has two forms in this tree, and `editor_server.rs`
    /// spells the difference out where it has to handle both:
    ///
    ///   (a) `Variant::Resource(ResourceRef)` — a resource loaded from a file
    ///       and then edited in place. This is what the test below constructs.
    ///   (b) `SubResource("id")` on the property, `__resref__<prop>` =
    ///       `Class::id`, and the body flattened into `<prefix><key>`
    ///       companions. That is what `.tscn` loading produces and what the
    ///       inspector's "New <Type>" picker writes — *"the COMMON case: every
    ///       resource the picker creates lands here, never in (a)"*.
    ///
    /// `expand_subresource` matches only `Variant::Resource`, so on the common
    /// form it answers `None` — "not expandable" — for exactly the
    /// sub-resources the inspector exists to edit. That is almost certainly why
    /// this module has never had a caller.
    ///
    /// ⚠️ This test asserts the CURRENT limitation on purpose. It is a
    /// signpost, not an endorsement: anyone about to build the inspector's
    /// inline expander on `expand_subresource` (pat-s3rrv.152) will trip over
    /// it here in seconds rather than after wiring a UI that reports every real
    /// sub-resource as unexpandable. If you teach the module form (b), this
    /// test should fail — change it deliberately, and say so.
    ///
    /// ⚠️ Note how the sibling test below builds its fixture: it constructs a
    /// `ResourceRef` directly, i.e. form (a). A green suite proved nothing
    /// about real scenes, which is how the gap survived.
    #[test]
    fn expand_subresource_does_not_understand_the_common_subresource_form() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree
            .add_child(root, Node::new("Body", "StaticBody2D"))
            .unwrap();
        {
            let node = tree.get_node_mut(id).unwrap();
            // Exactly the shape `.tscn` loading and the "New <Type>" picker
            // write: a marker string, the class/id sidecar, and the resource
            // body flattened into `__shape__`-prefixed companions.
            node.set_property("shape", Variant::String("SubResource(\"1\")".to_string()));
            node.set_property(
                "__resref__shape",
                Variant::String("RectangleShape2D::1".to_string()),
            );
            node.set_property("__shape__size_x", Variant::Int(32));
            node.set_property("__shape__size_y", Variant::Int(16));
        }

        assert!(
            expand_subresource(&tree, id, "shape").is_none(),
            "🔑 the node plainly carries an inline RectangleShape2D — marker, \
             __resref__ sidecar and two flattened companions — and \
             `expand_subresource` still reports it as not expandable, because \
             it only matches `Variant::Resource`. Build the inspector expander \
             on this and every real sub-resource reads as empty."
        );
    }

    #[test]
    fn inspector_subresource_inline_edit_persists() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree
            .add_child(root, Node::new("Sprite", "Sprite2D"))
            .unwrap();
        tree.get_node_mut(id).unwrap().set_property(
            "material",
            Variant::Resource(Box::new(material_with_subprops())),
        );

        // Expanding the resource property exposes its sub-properties inline.
        let subs = expand_subresource(&tree, id, "material").expect("resource expands inline");
        let names: Vec<&str> = subs.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains(&"albedo"));
        assert!(names.contains(&"roughness"));

        // A non-resource property cannot be expanded.
        tree.get_node_mut(id)
            .unwrap()
            .set_property("z_index", Variant::Int(0));
        assert!(expand_subresource(&tree, id, "z_index").is_none());

        // Editing a sub-property inline persists to the embedded resource.
        assert!(commit_subresource_property(
            &mut tree,
            id,
            "material",
            "roughness",
            Variant::Float(0.9)
        ));
        assert_eq!(
            subresource_property(&tree, id, "material", "roughness"),
            Some(Variant::Float(0.9)),
            "the inline edit persists to the sub-resource"
        );
        // The other sub-property is untouched.
        assert_eq!(
            subresource_property(&tree, id, "material", "albedo"),
            Some(Variant::Int(0))
        );

        // The change is stored back on the node's resource value.
        match tree.get_node(id).unwrap().get_property("material") {
            Variant::Resource(res) => {
                assert_eq!(res.properties.get("roughness"), Some(&Variant::Float(0.9)))
            }
            other => panic!("expected a resource value, got {other:?}"),
        }
    }

    #[test]
    fn commit_to_non_resource_is_noop() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree.add_child(root, Node::new("N", "Node2D")).unwrap();
        tree.get_node_mut(id)
            .unwrap()
            .set_property("z_index", Variant::Int(1));

        assert!(!commit_subresource_property(
            &mut tree,
            id,
            "z_index",
            "x",
            Variant::Int(2)
        ));
        // The plain property is unchanged.
        assert_eq!(
            tree.get_node(id).unwrap().get_property("z_index"),
            Variant::Int(1)
        );
    }
}
