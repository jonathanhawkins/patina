//! pat-0q2kx.106 — the missing Godot 4.6 2D node/resource classes are
//! ClassDB-registered with the right parents, defaults, signals and enums.
//!
//! This is not cosmetic. `Node::is_class` resolves through ClassDB
//! (gdscene/src/node.rs), so an unregistered node answers `false` to
//! `is_class("CanvasItem")` and silently drops out of every canvas walk —
//! `collect_canvas_items` skips it and the Create-Node dialog never offers it.
//! A `.tscn` naming `RayCast2D` or `TileSet` loaded a node the engine could not
//! classify.
//!
//! Parents are the `inherits` attribute of each `doc/classes/<Name>.xml`;
//! property defaults are that file's `default=`. Both are asserted here rather
//! than merely "the class exists", because a wrong parent is the failure mode
//! that silently breaks the `is_class` walks this bead exists to fix.

use gdobject::class_db::can_instantiate;
use gdobject::{
    class_exists, class_has_enum, class_has_signal, get_property_list, instantiate,
    is_parent_class, register_2d_classes,
};
use gdvariant::Variant;

/// Every class this bead adds, with the parent its doc XML declares.
const ADDED: &[(&str, &str)] = &[
    // Node2D children.
    ("RayCast2D", "Node2D"),
    ("ShapeCast2D", "Node2D"),
    ("CanvasModulate", "Node2D"),
    ("VisibleOnScreenNotifier2D", "Node2D"),
    ("Parallax2D", "Node2D"),
    ("ParallaxLayer", "Node2D"),
    ("CollisionPolygon2D", "Node2D"),
    ("TileMap", "Node2D"),
    // Deeper node chains.
    ("VisibleOnScreenEnabler2D", "VisibleOnScreenNotifier2D"),
    ("ParallaxBackground", "CanvasLayer"),
    ("PhysicalBone2D", "RigidBody2D"),
    // Resources.
    ("Curve2D", "Resource"),
    ("SpriteFrames", "Resource"),
    ("OccluderPolygon2D", "Resource"),
    ("TileSet", "Resource"),
    ("TileSetSource", "Resource"),
    ("TileMapPattern", "Resource"),
    ("TileSetAtlasSource", "TileSetSource"),
    ("TileSetScenesCollectionSource", "TileSetSource"),
    // `TileData` inherits Object, not Resource — the one exception in the
    // family, per doc/classes/TileData.xml.
    ("TileData", "Object"),
];

fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

/// **The acceptance test.** Every class registers, instantiates, and reports the
/// parent its doc XML declares.
#[test]
fn classdb_2d_missing_classes_registered() {
    register_2d_classes();

    for (class, parent) in ADDED {
        assert!(class_exists(class), "{class} must be registered");
        assert!(
            is_parent_class(class, parent),
            "{class} must inherit {parent} (doc/classes/{class}.xml `inherits`)"
        );
        // TileSetSource is GDREGISTER_ABSTRACT_CLASS upstream (its concrete
        // forms are TileSetAtlasSource / TileSetScenesCollectionSource).
        if *class == "TileSetSource" {
            assert!(!can_instantiate(class), "{class}: Godot registers it with GDREGISTER_ABSTRACT_CLASS");
        } else {
            assert!(
                can_instantiate(class),
                "{class} must be instantiable via ClassDB"
            );
        }
        if *class != "TileSetSource" {
            assert!(
                instantiate(class).is_some(),
                "ClassDB must hand back an instance of {class}"
            );
        }
    }

    // The whole point of the parents: the Node2D family must resolve as canvas
    // items, or the render walk drops them. `ParallaxBackground` is deliberately
    // excluded — it is a CanvasLayer, which is a Node, not a CanvasItem.
    for (class, _) in ADDED.iter().filter(|(_, p)| *p == "Node2D") {
        assert!(
            is_parent_class(class, "CanvasItem"),
            "{class} must resolve as a CanvasItem so canvas walks include it"
        );
        assert!(
            is_parent_class(class, "Node"),
            "{class} must resolve as a Node"
        );
    }

    // Chains through an intermediate resolve all the way down.
    assert!(
        is_parent_class("VisibleOnScreenEnabler2D", "CanvasItem"),
        "the enabler inherits the notifier, so it is a CanvasItem too"
    );
    assert!(
        is_parent_class("TileSetAtlasSource", "Resource"),
        "an atlas source is a TileSetSource, which is a Resource"
    );
    assert!(
        is_parent_class("PhysicalBone2D", "CanvasItem"),
        "PhysicalBone2D inherits RigidBody2D, which is a CanvasItem"
    );
}

/// Defaults come from the doc XML, not from whatever `Variant` happens to be
/// zero — a wrong default is invisible until a scene relies on it.
#[test]
fn added_2d_classes_carry_their_documented_defaults() {
    register_2d_classes();

    // RayCast2D's ray points 50px down by default; a (0,0) default would make
    // every authored raycast a no-op.
    assert_eq!(
        default_of("RayCast2D", "target_position"),
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 50.0))
    );
    assert_eq!(default_of("RayCast2D", "enabled"), Variant::Bool(true));
    assert_eq!(
        default_of("RayCast2D", "exclude_parent"),
        Variant::Bool(true)
    );
    assert_eq!(default_of("RayCast2D", "collision_mask"), Variant::Int(1));

    // The notifier's rect is a 20x20 box centred on the origin.
    assert_eq!(
        default_of("VisibleOnScreenNotifier2D", "rect"),
        Variant::Rect2(gdcore::math::Rect2::new(
            gdcore::math::Vector2::new(-10.0, -10.0),
            gdcore::math::Vector2::new(20.0, 20.0),
        ))
    );

    // A ParallaxLayer that defaulted to (0,0) motion_scale would pin the layer
    // to the camera instead of scrolling it.
    assert_eq!(
        default_of("ParallaxLayer", "motion_scale"),
        Variant::Vector2(gdcore::math::Vector2::ONE)
    );

    assert_eq!(
        default_of("CanvasModulate", "color"),
        Variant::Color(gdcore::math::Color::WHITE)
    );
    assert_eq!(
        default_of("CollisionPolygon2D", "one_way_collision_margin"),
        Variant::Float(1.0)
    );
    assert_eq!(
        default_of("TileMap", "rendering_quadrant_size"),
        Variant::Int(16)
    );
    assert_eq!(
        default_of("TileSet", "tile_size"),
        Variant::Vector2(gdcore::math::Vector2::new(16.0, 16.0))
    );
}

/// The signals and enums the bead calls out, so a `connect("screen_entered", …)`
/// resolves and `VisibleOnScreenEnabler2D.ENABLE_MODE_*` is addressable.
#[test]
fn added_2d_classes_expose_their_signals_and_enums() {
    register_2d_classes();

    for signal in ["screen_entered", "screen_exited"] {
        assert!(
            class_has_signal("VisibleOnScreenNotifier2D", signal),
            "VisibleOnScreenNotifier2D must declare `{signal}`"
        );
    }
    assert!(
        class_has_signal("TileData", "changed"),
        "TileData must declare `changed`"
    );

    assert!(
        class_has_enum("VisibleOnScreenEnabler2D", "EnableMode"),
        "VisibleOnScreenEnabler2D must expose its EnableMode enum"
    );
    assert!(
        class_has_enum("CollisionPolygon2D", "BuildMode"),
        "CollisionPolygon2D must expose its BuildMode enum"
    );
    assert!(
        class_has_enum("OccluderPolygon2D", "CullMode"),
        "OccluderPolygon2D must expose its CullMode enum"
    );
}
