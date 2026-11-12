//! `class_db::godot_class_has_property` answers from Godot's own docs
//! (pat-w0q0w), so a read of a name Godot does not have can raise Godot's
//! "Invalid access to property or key" — and one Godot DOES have never does,
//! even when Patina's ClassDB omits it.

use gdobject::class_db::godot_class_has_property;

#[test]
fn documented_properties_are_found_through_the_inheritance_chain() {
    // `owner` is a Node property Patina's ClassDB does not register.
    assert_eq!(godot_class_has_property("Node", "owner"), Some(true));
    // Inherited: Sprite2D -> Node2D -> CanvasItem -> Node.
    assert_eq!(godot_class_has_property("Sprite2D", "texture"), Some(true));
    assert_eq!(godot_class_has_property("Sprite2D", "position"), Some(true));
    assert_eq!(godot_class_has_property("Sprite2D", "material"), Some(true));
    assert_eq!(
        godot_class_has_property("Sprite2D", "process_mode"),
        Some(true)
    );
    // A module class (modules/*/doc_classes).
    assert_eq!(godot_class_has_property("GridMap", "cell_size"), Some(true));
}

#[test]
fn a_name_no_class_in_the_chain_documents_is_absent() {
    assert_eq!(
        godot_class_has_property("Node2D", "no_such_prop"),
        Some(false)
    );
    // A child's property is not its parent's.
    assert_eq!(godot_class_has_property("Node", "texture"), Some(false));
}

#[test]
fn an_undocumented_class_cannot_be_judged() {
    assert_eq!(
        godot_class_has_property("PatinaOnlyThing", "anything"),
        None
    );
}

#[test]
fn the_table_is_sorted_for_binary_search() {
    let classes = gdobject::godot_doc_properties::CLASSES;
    assert!(classes.len() > 900, "{} classes", classes.len());
    assert!(
        classes.windows(2).all(|w| w[0].0 < w[1].0),
        "classes sorted"
    );
    for (class, _, props) in classes {
        assert!(
            props.windows(2).all(|w| w[0] < w[1]),
            "{class} props sorted"
        );
    }
}
