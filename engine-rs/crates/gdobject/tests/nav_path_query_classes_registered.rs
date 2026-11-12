//! pat-s3rrv.181 (step 1): the four `NavigationPathQuery*` classes resolve in
//! ClassDB with Godot's documented defaults.
//!
//! Ground truth: `upstream/godot/doc/classes/NavigationPathQueryParameters2D.xml`
//! and its 3D / Result siblings. Every name, default and accessor pair below is
//! transcribed from those files rather than derived.
//!
//! # Scope
//!
//! This is the first of the bead's six steps. These are the argument and result
//! objects of `NavigationServer2D/3D.query_path()`; **that server method does
//! not exist yet**, and neither does script-side `.new()` dispatch. Registering
//! them is not merely declarative — `ClassDB::instantiate` builds a
//! `GenericObject` from the registered property table, so a caller gets a real
//! object carrying the right defaults — but a script cannot yet run a query
//! through one. The remaining steps live in `navigation.rs`, which was under
//! another agent's lease.

use gdobject::class_db::{class_exists, get_property_list, instantiate, register_all_classes};
use gdobject::is_parent_class;
use gdvariant::Variant;

const PARAMS: [&str; 2] = [
    "NavigationPathQueryParameters2D",
    "NavigationPathQueryParameters3D",
];
const RESULTS: [&str; 2] = ["NavigationPathQueryResult2D", "NavigationPathQueryResult3D"];

fn prop(class_name: &str, property: &str) -> Option<gdobject::class_db::PropertyInfo> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
}

fn default_of(class_name: &str, property: &str) -> Option<Variant> {
    prop(class_name, property).map(|p| p.default_value)
}

#[test]
fn nav_path_query_classes_are_registered_under_refcounted() {
    register_all_classes();

    for class in PARAMS.iter().chain(RESULTS.iter()) {
        assert!(class_exists(class), "{class} must be registered");
        assert!(
            is_parent_class(class, "RefCounted"),
            "{class} inherits RefCounted — it is a query object, not a Resource \
             and not a Node"
        );
        // ⚠️ Not abstract: `ClassDB::instantiate` refuses an abstract class, and
        // these exist precisely to be constructed and handed to `query_path`.
        assert!(
            instantiate(class).is_some(),
            "{class} must be instantiable — an abstract or unregistered class \
             gives None, and the whole point of these is to be created"
        );
    }
}

/// 🔑 The defaults that would silently break a query if copied wrong.
#[test]
fn the_parameter_defaults_match_the_doc_xml() {
    register_all_classes();

    for class in PARAMS {
        // ⚠️ Layer 1, not 0. A zero mask matches no region, so every query
        // through a default-constructed parameters object would return an empty
        // path — and look like a pathfinding bug rather than a default.
        assert_eq!(
            default_of(class, "navigation_layers"),
            Some(Variant::Int(1)),
            "{class}.navigation_layers defaults to bit 0 set"
        );
        // ⚠️ 4096, not 0. Zero means "no limit" for the other three caps below,
        // but this one is a real budget — 0 here would refuse every search.
        assert_eq!(
            default_of(class, "path_search_max_polygons"),
            Some(Variant::Int(4096)),
            "{class}.path_search_max_polygons is a real cap, not an unset 0"
        );
        // All three metadata bits on: TYPES | RIDS | OWNERS. This is why a
        // plain query still fills `path_types`.
        assert_eq!(
            default_of(class, "metadata_flags"),
            Some(Variant::Int(7)),
            "{class}.metadata_flags defaults to all three bits"
        );
        // PATH_POSTPROCESSING_CORRIDORFUNNEL = 0 — the taut line, not the
        // polygon-centroid chain.
        assert_eq!(
            default_of(class, "path_postprocessing"),
            Some(Variant::Int(0))
        );
        assert_eq!(
            default_of(class, "simplify_path"),
            Some(Variant::Bool(false))
        );
        assert_eq!(
            default_of(class, "simplify_epsilon"),
            Some(Variant::Float(0.0))
        );
        // Zero = no limit for these.
        assert_eq!(
            default_of(class, "path_return_max_length"),
            Some(Variant::Float(0.0))
        );
        assert_eq!(
            default_of(class, "path_return_max_radius"),
            Some(Variant::Float(0.0))
        );
        assert_eq!(
            default_of(class, "path_search_max_distance"),
            Some(Variant::Float(0.0))
        );
    }
}

/// ⚠️ `get_simplify_path`, **not** `is_simplify_path`.
///
/// Godot uses the `is_` prefix for a great many bool getters — `is_editable`,
/// `is_allow_unpress`, `is_showing_hidden_files` — and does not use it here.
/// Deriving the name from the convention produces a plausible, wrong string,
/// which is why every accessor in this registration was transcribed from the
/// doc XML instead.
#[test]
fn the_bool_getter_does_not_follow_the_is_convention() {
    register_all_classes();

    for class in PARAMS {
        let p = prop(class, "simplify_path").expect("simplify_path is registered");
        assert_eq!(p.getter, "get_simplify_path", "{class}");
        assert_eq!(p.setter, "set_simplify_path", "{class}");
    }
}

/// The 2D and 3D variants differ exactly where Godot says they do.
///
/// ⚠️ The control for the shared loops above: they assert both dimensions carry
/// the same defaults, which would also pass if 3D had been registered as a copy
/// of 2D with `Vector2` positions.
#[test]
fn the_dimensions_differ_only_in_their_vector_types() {
    register_all_classes();

    assert!(matches!(
        default_of("NavigationPathQueryParameters2D", "start_position"),
        Some(Variant::Vector2(_))
    ));
    assert!(matches!(
        default_of("NavigationPathQueryParameters3D", "start_position"),
        Some(Variant::Vector3(_))
    ));
    assert!(matches!(
        default_of("NavigationPathQueryResult2D", "path"),
        Some(Variant::PackedVector2Array(_))
    ));
    assert!(matches!(
        default_of("NavigationPathQueryResult3D", "path"),
        Some(Variant::PackedVector3Array(_))
    ));
}

/// The result object carries the four out-fields plus the length.
#[test]
fn the_result_objects_carry_their_out_fields() {
    register_all_classes();

    for class in RESULTS {
        assert!(matches!(
            default_of(class, "path_types"),
            Some(Variant::PackedInt32Array(_))
        ));
        // ⚠️ `path_owner_ids` is Int64, not Int32 — object ids, not enum tags.
        assert!(matches!(
            default_of(class, "path_owner_ids"),
            Some(Variant::PackedInt64Array(_))
        ));
        assert!(matches!(
            default_of(class, "path_rids"),
            Some(Variant::Array(_))
        ));
        assert_eq!(default_of(class, "path_length"), Some(Variant::Float(0.0)));
    }
}
