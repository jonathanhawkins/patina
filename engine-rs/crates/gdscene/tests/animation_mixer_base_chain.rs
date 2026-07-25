//! pat-0q2kx.60 acceptance: `AnimationMixer` sits between `Node` and the two
//! mixers that derive from it.
//!
//! Godot: `AnimationMixer <- Node`, `AnimationPlayer <- AnimationMixer`,
//! `AnimationTree <- AnimationMixer` (`extension_api.json`,
//! `scene/animation/animation_mixer.h`). The base owns the animation-library,
//! callback-mode, root-motion and capture API both players share, so
//! `player is AnimationMixer` is true in GDScript and the editor's Create-Node
//! dialog groups both under it.
//!
//! Patina registered both classes directly under `Node` and had no
//! `AnimationMixer` at all, so the `is`-check was false and the shared
//! properties had no home. The registration is in place; this pins it, because
//! a parent chain is exactly the kind of thing a later edit re-flattens without
//! anyone noticing.
//!
//! Lives in `gdscene` rather than `gdobject` so it can assert the part that
//! actually matters to a game: a real `SceneTree` node answering
//! `is_class("AnimationMixer")`, which routes through `Node::is_class` →
//! `gdobject::is_parent_class`.

use gdscene::node::Node;
use gdscene::SceneTree;

/// The animation classes are registered by the **3D** table.
///
/// Guarded because `AnimationMixer`'s `register_class` call is NOT wrapped in an
/// `if !class_exists(..)` the way its neighbours (e.g. `Material`) are, and
/// class_db documents that an unguarded re-registration mints a fresh `ClassId`
/// and leaves a stale `by_id` alias behind. With a per-test-process runner that
/// would not matter; sharing one process, the second `register_3d_classes()`
/// would re-register the very class under test.
fn register() {
    if !gdobject::class_db::class_exists("AnimationMixer") {
        gdobject::class_db::register_2d_classes();
        gdobject::class_db::register_3d_classes();
    }
}

/// **The acceptance test.**
#[test]
fn animation_mixer_base_chain() {
    register();

    // -- the base exists, and is abstract like Godot's ------------------------
    assert!(
        gdobject::class_db::class_exists("AnimationMixer"),
        "AnimationMixer must be registered — both players derive from it"
    );
    assert!(
        gdobject::class_db::is_abstract("AnimationMixer"),
        "AnimationMixer is abstract in Godot: it is a base, never instanced directly"
    );

    // -- both mixers derive from it ------------------------------------------
    assert!(
        gdobject::class_db::is_parent_class("AnimationPlayer", "AnimationMixer"),
        "AnimationPlayer derives from AnimationMixer"
    );
    assert!(
        gdobject::class_db::is_parent_class("AnimationTree", "AnimationMixer"),
        "AnimationTree derives from AnimationMixer"
    );

    // -- and the chain still reaches Node ------------------------------------
    //
    // ⚠️ The regression this guards against is inserting the base and severing
    // the tail: `AnimationPlayer` must still be a `Node`, transitively.
    assert!(
        gdobject::class_db::is_parent_class("AnimationMixer", "Node"),
        "AnimationMixer derives from Node"
    );
    assert!(
        gdobject::class_db::is_parent_class("AnimationPlayer", "Node"),
        "AnimationPlayer is still a Node through AnimationMixer"
    );
    assert!(
        gdobject::class_db::is_parent_class("AnimationTree", "Node"),
        "AnimationTree is still a Node through AnimationMixer"
    );

    // -- the part a game observes: a live node answers the `is` check ---------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player = tree
        .add_child(root, Node::new("AnimationPlayer", "AnimationPlayer"))
        .unwrap();
    let node = tree.get_node(player).expect("player node in the tree");
    assert!(
        node.is_class("AnimationMixer"),
        "a scene AnimationPlayer must satisfy `is AnimationMixer` in GDScript"
    );
    assert!(
        node.is_class("Node"),
        "…and still satisfy `is Node` through the inserted base"
    );
    assert!(
        !node.is_class("AnimationTree"),
        "siblings under the same base are NOT each other"
    );
}

/// The shared API Godot puts on `AnimationMixer` lives on the base, not copied
/// onto each subclass — that is the point of inserting it.
#[test]
fn animation_mixer_owns_the_shared_properties() {
    register();

    let info =
        gdobject::class_db::get_class_info("AnimationMixer").expect("AnimationMixer is registered");
    for prop in [
        "active",
        "deterministic",
        "root_node",
        "callback_mode_process",
        "callback_mode_method",
        "audio_max_polyphony",
    ] {
        assert!(
            info.properties.iter().any(|p| p.name == prop),
            "AnimationMixer must own the shared property '{prop}', got {:?}",
            info.properties.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }

    // Inherited, not redeclared: the subclass sees the base's property through
    // the chain.
    assert!(
        gdobject::class_db::class_has_property("AnimationPlayer", "callback_mode_process"),
        "AnimationPlayer inherits the mixer's callback_mode_process"
    );
}
