//! pat-s3rrv.67 — a lambda visibility filter can be removed again.
//!
//! Godot ground truth: `Callable` equality for a lambda is identity —
//! *"Lambda callables are only compared by reference"*, `compare_equal` is
//! literally `return p_a == p_b;`
//! (`upstream/godot/modules/gdscript/gdscript_lambda_callable.cpp:37-40`).
//! `MultiplayerSynchronizer`'s filter list is a set keyed by that equality
//! (`multiplayer_synchronizer.h:59`), so handing back the same Callable removes
//! it.
//!
//! ⚠️ **The bug this covers was silent and permanent.** `CallableRef`'s
//! `PartialEq` ended `_ => false` with the note "lambdas are never equal", so
//! `f == f` was FALSE for a lambda. `remove_visibility_filter` does
//! `filters.retain(|x| x != &filter)`, which therefore never matched — a script
//! that installed a lambda filter kept filtering forever, with no error and no
//! way to take it back. It was recorded as a known gap in a comment at the call
//! site rather than discovered in a game; this test is what makes it stay fixed.
//!
//! ⚠️ **Why this drives the tree API directly instead of a script.** Two
//! independent reasons, both checked before writing it this way:
//!   * Godot types `add_visibility_filter` / `remove_visibility_filter` as
//!     **void**, and our script bridge matches — both arms return
//!     `Variant::Nil` (`gdscene/src/scripting.rs:2300-2323`). A script therefore
//!     cannot observe whether a removal matched, which is the whole assertion
//!     here.
//!   * The other obvious probe — have the filter tick a counter and watch it
//!     stop — cannot work either: GDScript lambdas capture by value, and
//!     capturing `self` is precisely the part of pat-s3rrv.67 that is NOT
//!     implemented, so a counter incremented inside the lambda never reaches the
//!     script.
//!
//! `SceneTree::synchronizer_{add,remove}_visibility_filter` return `bool`, and
//! that return is the honest signal.

use gdcore::math::Vector2;
use gdplatform::network::{MockNetwork, PeerId};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::{CallableRef, Variant};

/// A lambda Callable, built the way the interpreter builds one: a fresh `Arc`
/// per lambda, which is the identity the comparison keys on.
///
/// The body and defaults are opaque placeholders — this filter is never
/// invoked, only stored and matched.
fn lambda() -> Variant {
    Variant::Callable(Box::new(CallableRef::Lambda {
        params: vec!["peer".into()],
        body: std::sync::Arc::new(()),
        captures: std::sync::Arc::new(std::collections::HashMap::new()),
        param_defaults: std::sync::Arc::new(()),
    }))
}

/// `Player(Node2D) -> Sync(MultiplayerSynchronizer)`, ticked until the tree has
/// entered the synchronizer into its replication registry.
///
/// ⚠️ The ticking is load-bearing, not warm-up.
/// `synchronizer_add_visibility_filter` stores nothing and returns `false`
/// while the node is absent from that registry, and the tree enters it on its
/// own poll — so adding a filter immediately after construction is a silent
/// no-op. Both sibling filter tests carry the same warning, each having hit it.
fn registered_sync() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player = tree
        .add_child(root, Node::new("Player", "Node2D"))
        .expect("player added");
    if let Some(n) = tree.get_node_mut(player) {
        n.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    }

    let sync = tree
        .add_child(player, Node::new("Sync", "MultiplayerSynchronizer"))
        .expect("synchronizer added");
    if let Some(n) = tree.get_node_mut(sync) {
        n.set_property("root_path", Variant::String("..".to_string()));
        n.set_property("replication_interval", Variant::Float(0.0));
        n.set_property(
            "_sync_properties",
            Variant::Array(vec![Variant::String("position".to_string())]),
        );
    }

    let (server, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server));

    let mut ml = MainLoop::new(tree);
    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }
    (ml, sync)
}

/// **The acceptance.** Handing back the same lambda removes the filter.
#[test]
fn a_lambda_visibility_filter_can_be_removed_with_the_same_callable() {
    let (mut ml, sync) = registered_sync();
    let f = lambda();

    // ⚠️ Asserted FIRST. A failed removal is meaningless if the filter was never
    // stored — a broken fixture reading as a broken feature is exactly how this
    // gap stayed hidden.
    assert!(
        ml.tree_mut()
            .synchronizer_add_visibility_filter(sync, f.clone()),
        "the filter must be accepted; false means the synchronizer was not in \
         the replication registry yet and the rest of this test proves nothing"
    );

    assert!(
        ml.tree_mut()
            .synchronizer_remove_visibility_filter(sync, f.clone()),
        "removing the very Callable that was added must match — false is the \
         old `lambdas are never equal` behaviour, where `retain` could never \
         match and the filter stayed installed forever"
    );

    // And it is genuinely gone, not merely reported gone: a second removal has
    // nothing left to match.
    assert!(
        !ml.tree_mut().synchronizer_remove_visibility_filter(sync, f),
        "the filter was already removed, so a second removal must report false"
    );
}

/// A *different* lambda removes nothing, even though it is written identically.
///
/// ⚠️ The half that keeps the fix honest. Both lambdas have the same parameter
/// list and the same (empty) captures, so an equality that fell back to
/// comparing fields rather than identity would pass the test above and fail
/// this one. Godot compares lambdas by reference, not by source text.
#[test]
fn a_different_lambda_does_not_remove_the_filter() {
    let (mut ml, sync) = registered_sync();

    assert!(
        ml.tree_mut()
            .synchronizer_add_visibility_filter(sync, lambda()),
        "the filter must have been stored for the removal attempt to mean anything"
    );
    assert!(
        !ml.tree_mut()
            .synchronizer_remove_visibility_filter(sync, lambda()),
        "a separately-built lambda is a DIFFERENT instance and must remove nothing"
    );
}
