//! pat-0q2kx.111 — the Sprite3D family is ClassDB-registered with Godot's
//! parents, defaults, enums and signals.
//!
//! Godot ground truth: `upstream/godot/scene/3d/sprite_3d.cpp` and
//! `doc/classes/{SpriteBase3D,Sprite3D,AnimatedSprite3D}.xml`. `SpriteBase3D` is
//! abstract and carries the shared quad surface (centered/offset/flip/modulate/
//! pixel_size/axis/billboard/alpha_cut/…); `Sprite3D` adds the region and frame
//! grid; `AnimatedSprite3D` adds SpriteFrames playback.
//!
//! `gdserver3d::sprite3d` already builds the textured quad and
//! `render_server_3d.rs` syncs it every frame — but none of these three classes
//! had a ClassDB entry at all. `Node::is_class` resolves through ClassDB, so an
//! authored `Sprite3D` could not be recognised as a `GeometryInstance3D`: it
//! dropped out of class walks and never appeared in the Create-Node dialog.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_enum, class_has_signal, get_property_list, instantiate,
    is_parent_class, register_3d_classes,
};
use gdvariant::Variant;

fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

/// **The acceptance test.** All three register, inherit correctly, and only the
/// two concrete ones instantiate.
#[test]
fn sprite3d_renders_in_scene() {
    register_3d_classes();

    for class in ["SpriteBase3D", "Sprite3D", "AnimatedSprite3D"] {
        assert!(class_exists(class), "{class} must be registered");
    }

    // The chain that matters: these must read as GeometryInstance3D so the 3D
    // class walks and the render sync pick them up.
    assert!(is_parent_class("SpriteBase3D", "GeometryInstance3D"));
    for class in ["Sprite3D", "AnimatedSprite3D"] {
        assert!(
            is_parent_class(class, "SpriteBase3D"),
            "{class} must inherit SpriteBase3D"
        );
        assert!(
            is_parent_class(class, "GeometryInstance3D"),
            "{class} must resolve as a GeometryInstance3D — this is what
             `is_class` walks depend on"
        );
        assert!(
            is_parent_class(class, "Node3D"),
            "{class} must resolve as a Node3D"
        );
    }

    // SpriteBase3D is abstract in Godot: it holds the shared surface but is
    // never instantiated on its own.
    assert!(
        is_abstract("SpriteBase3D"),
        "SpriteBase3D is abstract in Godot and must not be instantiable"
    );
    for class in ["Sprite3D", "AnimatedSprite3D"] {
        assert!(can_instantiate(class), "{class} must be instantiable");
        assert!(
            instantiate(class).is_some(),
            "ClassDB must hand back an instance of {class}"
        );
    }
}

/// Defaults come from the doc XML. Several of these are non-obvious and would
/// be wrong if guessed — `pixel_size` is 0.01, not 1.0; `axis` is Z (2), not X;
/// `texture_filter` is LINEAR_WITH_MIPMAPS (3), not 0.
#[test]
fn sprite3d_family_carries_its_documented_defaults() {
    register_3d_classes();

    assert_eq!(
        default_of("SpriteBase3D", "pixel_size"),
        Variant::Float(0.01)
    );
    assert_eq!(default_of("SpriteBase3D", "axis"), Variant::Int(2));
    assert_eq!(
        default_of("SpriteBase3D", "texture_filter"),
        Variant::Int(3)
    );
    assert_eq!(default_of("SpriteBase3D", "centered"), Variant::Bool(true));
    assert_eq!(
        default_of("SpriteBase3D", "double_sided"),
        Variant::Bool(true)
    );
    assert_eq!(
        default_of("SpriteBase3D", "transparent"),
        Variant::Bool(true)
    );
    assert_eq!(default_of("SpriteBase3D", "shaded"), Variant::Bool(false));
    assert_eq!(
        default_of("SpriteBase3D", "alpha_scissor_threshold"),
        Variant::Float(0.5)
    );

    // The frame grid defaults to a single cell, not zero — a 0 here would make
    // every unconfigured Sprite3D compute an empty region.
    assert_eq!(default_of("Sprite3D", "hframes"), Variant::Int(1));
    assert_eq!(default_of("Sprite3D", "vframes"), Variant::Int(1));
    assert_eq!(
        default_of("Sprite3D", "region_enabled"),
        Variant::Bool(false)
    );

    assert_eq!(
        default_of("AnimatedSprite3D", "speed_scale"),
        Variant::Float(1.0)
    );
    assert_eq!(
        default_of("AnimatedSprite3D", "frame_progress"),
        Variant::Float(0.0)
    );

    // Inherited properties resolve through the chain, so a Sprite3D answers for
    // the whole SpriteBase3D surface too.
    assert_eq!(default_of("Sprite3D", "pixel_size"), Variant::Float(0.01));
}

/// Signals and enums, so `connect("frame_changed", …)` resolves and
/// `SpriteBase3D.ALPHA_CUT_*` is addressable.
#[test]
fn sprite3d_family_exposes_signals_and_enums() {
    register_3d_classes();

    assert!(class_has_signal("Sprite3D", "frame_changed"));
    assert!(class_has_signal("Sprite3D", "texture_changed"));
    for signal in [
        "frame_changed",
        "animation_changed",
        "animation_finished",
        "animation_looped",
        "sprite_frames_changed",
    ] {
        assert!(
            class_has_signal("AnimatedSprite3D", signal),
            "AnimatedSprite3D must declare `{signal}`"
        );
    }

    assert!(class_has_enum("SpriteBase3D", "AlphaCutMode"));
    assert!(class_has_enum("SpriteBase3D", "DrawFlags"));
}
