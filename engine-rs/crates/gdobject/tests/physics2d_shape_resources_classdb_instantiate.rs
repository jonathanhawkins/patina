//! pat-s3rrv.98 acceptance — the 2D shape resource family in ClassDB.
//!
//! The whole `Shape2D` hierarchy plus `PhysicsMaterial` was absent, while the
//! 3D twins have been registered since pat-nzgdu.96. The names were already
//! known to the `.tscn` shape parser and to the editor's resource picker, so
//! authoring worked while `CircleShape2D.new()`, `ClassDB.instantiate(...)` and
//! every ClassDB-driven surface (Create dialog, inspector schema,
//! `packed_scene`'s default seeding) failed.
//!
//! ⚠️ Defaults are asserted against `doc/classes/*.xml`, the oracle — **not**
//! against what `gdscene::physics_server`'s parser assumes. Two disagree, and
//! the parser is the side that is wrong:
//!
//! | property | Godot XML | parser |
//! |---|---|---|
//! | `CapsuleShape2D.height` | **30.0** | 20.0 |
//! | `SeparationRayShape2D.length` | **20.0** | (no entry) |
//!
//! Recorded on the bead. Changing the parser alters the collision behaviour of
//! every `.tscn` that omits an explicit height, which is a separate decision
//! from registering the class.

use gdobject::class_db::{get_class_info, register_2d_classes};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

/// The eight concrete shapes, which is the set the `.tscn` parser handles.
const CONCRETE_SHAPES: [&str; 8] = [
    "CircleShape2D",
    "RectangleShape2D",
    "CapsuleShape2D",
    "SegmentShape2D",
    "WorldBoundaryShape2D",
    "SeparationRayShape2D",
    "ConvexPolygonShape2D",
    "ConcavePolygonShape2D",
];

fn default_of(class: &str, name: &str) -> Variant {
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
        .clone()
}

/// **The acceptance test.**
#[test]
fn physics2d_shape_resources_classdb_instantiate() {
    // ⚠️ ClassDB is empty in a unit test until a register_* entry point runs;
    // without this every assertion below fails for the wrong reason.
    register_2d_classes();

    assert!(class_exists("Shape2D"), "the Resource base must be known");
    assert!(
        is_parent_class("Shape2D", "Resource"),
        "Shape2D derives Resource, which is what makes a shape assignable to a \
         `shape` property"
    );

    for class in CONCRETE_SHAPES {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "Shape2D"),
            "{class} must derive Shape2D, not Resource directly — the parent \
             chain is what a `shape` property's type check walks"
        );
        assert!(
            is_parent_class(class, "Resource"),
            "{class} must still reach Resource through Shape2D"
        );
    }

    assert!(class_exists("PhysicsMaterial"));
    assert!(is_parent_class("PhysicsMaterial", "Resource"));
}

/// 🔑 THE DEFAULTS ARE GODOT'S, INCLUDING THE TWO THE PARSER GETS WRONG.
///
/// ⚠️ `CapsuleShape2D.height` is the whole point of this test. The bead text and
/// `physics_server`'s parser both say 20.0; `doc/classes/CapsuleShape2D.xml`
/// says **30.0**. A test written from the bead rather than the oracle would pin
/// the bug.
#[test]
fn shape_defaults_match_the_godot_documentation() {
    register_2d_classes();

    assert_eq!(
        default_of("Shape2D", "custom_solver_bias"),
        Variant::Float(0.0)
    );
    assert_eq!(default_of("CircleShape2D", "radius"), Variant::Float(10.0));
    assert_eq!(
        default_of("RectangleShape2D", "size"),
        Variant::Vector2(gdcore::math::Vector2::new(20.0, 20.0))
    );

    assert_eq!(default_of("CapsuleShape2D", "radius"), Variant::Float(10.0));
    assert_eq!(
        default_of("CapsuleShape2D", "height"),
        Variant::Float(30.0),
        "Godot's CapsuleShape2D height is 30.0; 20.0 is the parser's bug"
    );

    assert_eq!(
        default_of("SegmentShape2D", "b"),
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 10.0))
    );
    assert_eq!(
        default_of("WorldBoundaryShape2D", "normal"),
        Variant::Vector2(gdcore::math::Vector2::new(0.0, -1.0)),
        "the default normal points -Y, so the boundary reads as a floor"
    );
    assert_eq!(
        default_of("SeparationRayShape2D", "length"),
        Variant::Float(20.0),
        "Godot's SeparationRayShape2D length is 20.0, not 10.0"
    );
    assert_eq!(
        default_of("SeparationRayShape2D", "slide_on_slope"),
        Variant::Bool(false)
    );

    // The polygon shapes start empty — a shape with no points collides with
    // nothing, which is what an unconfigured resource should do.
    assert_eq!(
        default_of("ConvexPolygonShape2D", "points"),
        Variant::PackedVector2Array(Vec::new())
    );
    assert_eq!(
        default_of("ConcavePolygonShape2D", "segments"),
        Variant::PackedVector2Array(Vec::new())
    );

    assert_eq!(
        default_of("PhysicsMaterial", "friction"),
        Variant::Float(1.0)
    );
    assert_eq!(default_of("PhysicsMaterial", "bounce"), Variant::Float(0.0));
    assert_eq!(default_of("PhysicsMaterial", "rough"), Variant::Bool(false));
    assert_eq!(
        default_of("PhysicsMaterial", "absorbent"),
        Variant::Bool(false)
    );
}

/// 🔑 EVERY NAME THE `.tscn` SHAPE PARSER HANDLES IS NOW IN ClassDB.
///
/// ⚠️ This is the coupling the bead is really about: `gdscene::physics_server`
/// resolves `[sub_resource type="…"]` by class NAME, and `packed_scene` seeds a
/// loaded node's defaults from `class_db::get_property_list`, which returns
/// empty for an unregistered class. A shape the parser understands but ClassDB
/// does not loses every property on load.
#[test]
fn every_parser_known_shape_name_is_registered() {
    register_2d_classes();

    for class in CONCRETE_SHAPES {
        let info = get_class_info(class).unwrap_or_else(|| panic!("{class} registered"));
        assert!(
            !info.properties.is_empty(),
            "{class} must carry a property schema, or the inspector and \
             packed_scene's default seeding have nothing to read"
        );
    }
}
