//! The common **2D node catalog** offered by the Create Node dialog.
//!
//! When the user creates a node, the dialog surfaces the everyday 2D building
//! blocks — `Node2D`, `Sprite2D`, `AnimatedSprite2D`, `CollisionShape2D`,
//! `Area2D`, `CharacterBody2D`, `Camera2D` — plus a handful of helper nodes.
//! This module defines that catalog and guarantees the classes are registered
//! in [`ClassDB`](gdobject::class_db) so they instantiate correctly.
//!
//! It reads the catalog from `class_db` rather than editing the dialog itself,
//! keeping it independent of the larger `create_dialog` module.

use gdobject::class_db;

/// The core 2D nodes every project reaches for first.
pub const CORE_2D_NODES: &[&str] = &[
    "Node2D",
    "Sprite2D",
    "AnimatedSprite2D",
    "CollisionShape2D",
    "Area2D",
    "CharacterBody2D",
    "Camera2D",
];

/// Additional commonly-used 2D helper nodes shown alongside the core set.
pub const HELPER_2D_NODES: &[&str] = &[
    "StaticBody2D",
    "RigidBody2D",
    "Path2D",
    "Line2D",
    "TileMapLayer",
];

/// Ensures the 2D node classes are registered in `ClassDB`. Idempotent — safe
/// to call repeatedly.
pub fn ensure_registered() {
    class_db::register_2d_classes();
}

/// The full common 2D catalog (core nodes first, then helpers), in display
/// order.
pub fn common_2d_nodes() -> Vec<&'static str> {
    CORE_2D_NODES
        .iter()
        .chain(HELPER_2D_NODES.iter())
        .copied()
        .collect()
}

/// Whether `class_name` is a registered class that can be instantiated.
pub fn can_instantiate(class_name: &str) -> bool {
    class_db::instantiate(class_name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    // `get_class` is a `GodotObject` trait method; the trait must be in scope.
    use gdobject::GodotObject;

    /// Acceptance (pat-k562f): the catalog includes the core 2D nodes and they
    /// instantiate correctly.
    #[test]
    fn create_node_2d_catalog() {
        ensure_registered();

        // Every core 2D node is registered and instantiates as itself.
        for name in CORE_2D_NODES {
            assert!(class_db::class_exists(name), "{name} is registered");
            let obj = class_db::instantiate(name)
                .unwrap_or_else(|| panic!("{name} instantiates"));
            assert_eq!(obj.get_class(), *name, "{name} instantiates as itself");
        }

        // The catalog surfaces exactly those core nodes (plus helpers).
        let catalog = common_2d_nodes();
        for core in CORE_2D_NODES {
            assert!(catalog.contains(core), "catalog includes {core}");
        }
        assert!(catalog.contains(&"Node2D"));
        assert!(catalog.contains(&"Camera2D"));

        // The convenience predicate agrees with direct instantiation.
        assert!(can_instantiate("CharacterBody2D"));
        assert!(!can_instantiate("ZzNotARealClassZz"));

        // Helper nodes are registered and instantiable too.
        for name in HELPER_2D_NODES {
            assert!(can_instantiate(name), "helper {name} instantiates");
        }
    }

    /// `ensure_registered` is idempotent: calling it twice doesn't change the
    /// catalog's instantiability.
    #[test]
    fn ensure_registered_is_idempotent() {
        ensure_registered();
        ensure_registered();
        assert!(can_instantiate("Sprite2D"));
        assert!(can_instantiate("Area2D"));
    }
}
