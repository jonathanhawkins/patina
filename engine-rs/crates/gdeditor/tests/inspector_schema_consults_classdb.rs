//! pat-s3rrv.57 finding (c): the editor inspector consults `gdobject::class_db`
//! instead of being wholly independent of it.
//!
//! # The gap this closes
//!
//! The inspector's schema table is transcribed from captured Godot 4.6.1
//! goldens (`inspector_schema_generated.rs`, 70 classes) and never consulted the
//! runtime registry. Measured against the ~688 classes ClassDB registers:
//!
//! * ~68 have their own seeded schema — unaffected, and deliberately so;
//! * ~185 resolved only to an **ancestor's** schema, so `AudioStreamPlayer` or
//!   `AnimationTree` showed `Node`'s properties and none of their own;
//! * ~435 resolved to **no schema at all** and fell through to stored-only
//!   enumeration — whatever the `.tscn` happened to save. Mostly Resources
//!   (`AStarGrid2D`, every `AnimationNode*`) that no captured scene contained.
//!
//! The merge is strictly additive and a golden always wins, which is what keeps
//! the oracle-parity numbers the goldens exist to defend.
//!
//! ⚠️ Every test here registers the classes first. ClassDB is a global registry
//! populated by `EditorState::new`; without that call `class_exists` answers
//! `false` for everything and each assertion below would pass vacuously against
//! the old behaviour.

use gdeditor::inspector_schema::editor_schema;
use gdobject::class_db::{
    class_exists, register_all_classes, register_class, ClassRegistration, PropertyInfo,
};
use gdvariant::Variant;

fn names(class_name: &str) -> Vec<String> {
    editor_schema(class_name)
        .unwrap_or_default()
        .iter()
        .map(|p| p.name.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// The mechanism, on probe classes — deterministic regardless of what the engine
// registrations happen to contain.
// ---------------------------------------------------------------------------

/// A registered class no golden covers gets a schema built from its properties.
#[test]
fn a_class_only_classdb_knows_gets_a_schema() {
    register_class(
        ClassRegistration::new("InspectorProbeOnlyInClassDb")
            .property(PropertyInfo::new("probe_value", Variant::Int(7)))
            .property(PropertyInfo::new("probe_flag", Variant::Bool(true))),
    );

    let schema = editor_schema("InspectorProbeOnlyInClassDb")
        .expect("a registered class must resolve to a schema, not to None");
    let got: Vec<String> = schema.iter().map(|p| p.name.to_string()).collect();

    assert!(got.contains(&"probe_value".to_string()));
    assert!(got.contains(&"probe_flag".to_string()));

    // The default carries the type, so the two can never disagree.
    let value = schema
        .iter()
        .find(|p| p.name == "probe_value")
        .expect("probe_value present");
    assert_eq!(value.type_name, "int", "Godot's own type vocabulary");
    assert_eq!(value.default, Variant::Int(7));
    assert_eq!(
        value.true_default,
        Variant::Int(7),
        "a ClassDB property's registered default IS the true class default — \
         there is no separate display value to diverge from"
    );
}

/// ⚠️ A `Nil` default with a `hint_string` is an OBJECT slot, not an absent
/// value: that is how a registered object-typed property spells "no instance
/// yet". Reporting the literal variant name `"Nil"` would make the inspector
/// render an empty cell where a resource picker belongs.
#[test]
fn an_object_slot_is_typed_object_not_nil() {
    register_class(
        ClassRegistration::new("InspectorProbeObjectSlot")
            .property(PropertyInfo::new("material", Variant::Nil).with_hint_string("Material")),
    );

    let schema = editor_schema("InspectorProbeObjectSlot").expect("registered");
    let slot = schema
        .iter()
        .find(|p| p.name == "material")
        .expect("material present");
    assert_eq!(slot.type_name, "Object");
}

/// An unregistered class still has no schema — the merge must not manufacture
/// one, or the inspector would show an empty property list as though it were
/// authoritative.
#[test]
fn an_unregistered_class_still_has_no_schema() {
    register_all_classes();
    assert!(!class_exists("TotallyMadeUpProbeClass"));
    assert!(editor_schema("TotallyMadeUpProbeClass").is_none());
}

// ---------------------------------------------------------------------------
// The integration, on real engine classes.
// ---------------------------------------------------------------------------

/// **The acceptance case for the 185.** `AudioStreamPlayer` resolved to `Node`'s
/// seeded schema and showed none of its own properties — no `stream`, no
/// `volume_db`, no `autoplay`. It must now carry both.
#[test]
fn a_class_that_only_reached_an_ancestor_now_carries_its_own_properties() {
    register_all_classes();

    let node = names("Node");
    let player = names("AudioStreamPlayer");

    assert!(!node.is_empty(), "Node is a seeded class");
    // Nothing from the ancestor may be lost — the golden is still the base.
    for p in &node {
        assert!(
            player.contains(p),
            "AudioStreamPlayer must keep the inherited `{p}` from Node's golden"
        );
    }
    // …and its own registered properties are added on top.
    for own in ["stream", "volume_db", "autoplay"] {
        assert!(
            player.contains(&own.to_string()),
            "AudioStreamPlayer must now show its own `{own}`; got {player:?}"
        );
    }
    assert!(
        player.len() > node.len(),
        "a leaf class showing exactly its base's schema is the bug this closes"
    );
}

/// **The acceptance case for the 435.** `AStarGrid2D` derives from `RefCounted`,
/// which no golden covers, so `editor_schema` returned `None` and the inspector
/// fell back to stored-only enumeration.
#[test]
fn a_class_with_no_seeded_ancestor_now_resolves() {
    register_all_classes();

    let schema = editor_schema("AStarGrid2D")
        .expect("a registered class with no seeded ancestor must still resolve");
    let got: Vec<String> = schema.iter().map(|p| p.name.to_string()).collect();

    for own in ["cell_shape", "diagonal_mode", "jumping_enabled"] {
        assert!(
            got.contains(&own.to_string()),
            "AStarGrid2D must show its registered `{own}`; got {got:?}"
        );
    }
}

/// ⚠️ A captured golden is the transcribed inspector truth for its class —
/// property ORDER, hints, and the value Godot actually SHOWS, which for a few
/// properties is a context-derived display value rather than the class default.
/// Merging ClassDB into a seeded class would move the oracle-parity numbers the
/// goldens exist to defend, so the golden is returned untouched.
#[test]
fn a_seeded_class_is_returned_untouched() {
    register_all_classes();

    assert_eq!(editor_schema("Node").unwrap().len(), 8);
    assert_eq!(editor_schema("Node2D").unwrap().len(), 27);
    assert_eq!(editor_schema("Control").unwrap().len(), 72);

    // Order is part of the golden, not an accident of the table.
    let node2d = names("Node2D");
    assert_eq!(node2d.first().map(String::as_str), Some("process_mode"));
    assert_eq!(node2d.last().map(String::as_str), Some("script"));
}
