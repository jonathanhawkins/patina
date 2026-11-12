//! pat-s3rrv.98: the 2D physics SERVER singletons are in ClassDB.
//!
//! `PhysicsServer2D` and `PhysicsServer2DManager` are ordinary ClassDB entries
//! in Godot — `Object` subclasses with `inst=false`. Patina could already CALL
//! `PhysicsServer2D` from script (it is in `BUILTIN_SINGLETONS` and has an
//! interpreter dispatch arm), but `ClassDB.class_exists("PhysicsServer2D")`
//! answered false and `get_class_list()` omitted it.
//!
//! ⚠️ That split is the interesting failure: introspection disagreed with what
//! actually worked. The defensive idiom
//! `if ClassDB.class_exists(X): ...` refused a call that would have succeeded,
//! which is the opposite of the usual "registered but inert" problem.
//!
//! 🔑 Both must be ABSTRACT. `create_node_catalog` gates its picker on
//! `class_db::instantiate`, so a non-abstract registration would offer "new
//! PhysicsServer2D" in the editor's Create dialog for a class that cannot be
//! instantiated. Registration without the abstract marker would trade a missing
//! entry for a broken one.

use gdobject::class_db::{class_exists, instantiate, is_parent_class, register_all_classes};

fn setup() {
    register_all_classes();
}

#[test]
fn the_2d_physics_server_singletons_are_registered() {
    setup();

    for cls in ["PhysicsServer2D", "PhysicsServer2DManager"] {
        assert!(
            class_exists(cls),
            "{cls} is a ClassDB class in Godot; a script guarding on \
             ClassDB.class_exists(\"{cls}\") must not be told it is absent"
        );
        assert!(
            is_parent_class(cls, "Object"),
            "{cls} descends from Object in Godot"
        );
    }
}

/// The abstract half, asserted separately so a regression names which half broke.
#[test]
fn the_server_singletons_are_not_instantiable() {
    setup();

    // Only PhysicsServer2D: PhysicsServer2DManager is a plain
    // `GDREGISTER_CLASS(PhysicsServer2DManager)` upstream
    // (servers/register_server_types.cpp:278), so ClassDB can construct it.
    for cls in ["PhysicsServer2D"] {
        assert!(
            instantiate(cls).is_none(),
            "{cls} has inst=false in Godot — ClassDB.instantiate must refuse it. \
             A Some(..) here means the abstract marker was dropped, which would \
             also surface as a bogus entry in the editor's Create dialog"
        );
    }
}

/// CONTROL: a sibling that IS instantiable, proving `instantiate` can return
/// `Some` in this fixture at all.
///
/// Without it, the assertion above would pass just as happily if `instantiate`
/// were broken and returned `None` for everything.
#[test]
fn a_concrete_physics_class_is_still_instantiable() {
    setup();

    assert!(
        instantiate("CircleShape2D").is_some(),
        "fixture: CircleShape2D is concrete and must still instantiate, else \
         the not-instantiable assertions above prove nothing"
    );
}
