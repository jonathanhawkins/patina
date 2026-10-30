//! pat-s3rrv.17 — `add_visibility_filter` Callables actually run.
//!
//! # What was wrong
//!
//! `add_visibility_filter` existed only as a ClassDB `MethodInfo`. There was no
//! storage for a filter anywhere — `grep visibility_filter crates/gdplatform/src`
//! returned **0** — so registering one was a no-op and every peer saw
//! everything.
//!
//! # Why the filters could not live with the rest of the visibility state
//!
//! Every other piece of visibility (`public_visibility`, the per-peer overrides,
//! the `is_visible_to` gate) lives on gdplatform's synchronizer, so that is the
//! obvious home — and it is impossible. A filter is a `Callable` that has to be
//! INVOKED, and gdplatform depends only on `gdcore` + `gdvariant`; it has no
//! route to a script interpreter and physically cannot run one.
//!
//! So the filters are stored in gdscene and evaluated during the outbound pass,
//! and the verdict is written back into the synchronizer's per-peer visibility
//! map. That keeps `poll_outbound`'s existing `is_visible_to` check as the one
//! place visibility is enforced, rather than adding a second gate.
//!
//! # What each assertion is for
//!
//! `called_for` is set by the filter itself, so it proves the Callable was
//! genuinely invoked — the whole point, and the thing a stored-but-never-called
//! filter would fail. `vis` then proves the verdict reached the gate. Asserting
//! only `vis` would be weak: `false` is also what "we hide whenever any filter
//! exists" produces, which is why the allow-case control is here too.

use gdcore::math::Vector2;
use gdplatform::network::{MockNetwork, PeerId};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn scripted_sync_loop(src: &str) -> (MainLoop, NodeId) {
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
            Variant::Array(vec![Variant::String("position".to_string())].into()),
        );
    }

    tree.attach_script(
        sync,
        Box::new(GDScriptNodeInstance::from_source(src, sync).expect("script parses")),
    );

    let (server, _client) = MockNetwork::create_pair(PeerId(2));
    tree.get_multiplayer_mut()
        .set_multiplayer_peer(Box::new(server));

    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(sync);
    (ml, sync)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

/// ⚠️ The filter is registered every frame rather than guarded by a tick test.
/// The synchronizer is entered into the replication registry by the tree's own
/// poll, so an early call is a legitimate no-op.
///
/// ⚠️ Re-registering is now a genuine NO-OP rather than an accumulation. This
/// comment used to say the repeat calls "exercise that accumulation" of N
/// copies; since pat-s3rrv.17 wired `add_visibility_filter` to Godot's
/// `HashSet` semantics (`multiplayer_synchronizer.h:59`), the second and later
/// calls store nothing. The assertions below never depended on the count — they
/// read `called_for` and `vis` — so they are unaffected either way.
///
/// `is_visible_to(self.called_for)` is safe before the filter has ever run:
/// `called_for` starts at -1, which is not a valid peer id and answers with the
/// public default.
const DENY_SRC: &str = "\
extends Node
var ticks = 0
var called_for = -1
var vis = true
func _process(_delta):
    self.ticks = self.ticks + 1
    add_visibility_filter(Callable(self, \"_deny\"))
    self.vis = is_visible_to(self.called_for)
func _deny(peer):
    self.called_for = peer
    return false
";

const ALLOW_SRC: &str = "\
extends Node
var ticks = 0
var called_for = -1
var vis = false
func _process(_delta):
    self.ticks = self.ticks + 1
    add_visibility_filter(Callable(self, \"_allow\"))
    self.vis = is_visible_to(self.called_for)
func _allow(peer):
    self.called_for = peer
    return true
";

fn run(src: &str) -> (Variant, Variant, Variant) {
    let (mut ml, sync) = scripted_sync_loop(src);
    for _ in 0..8 {
        ml.step(1.0 / 60.0);
    }
    (
        script_var(&ml, sync, "ticks"),
        script_var(&ml, sync, "called_for"),
        script_var(&ml, sync, "vis"),
    )
}

/// **The headline.** A registered filter is invoked with the peer id, and
/// returning `false` hides the synchronizer from that peer.
#[test]
fn a_visibility_filter_callable_is_invoked_and_can_hide_a_peer() {
    let (ticks, called_for, vis) = run(DENY_SRC);

    match ticks {
        Variant::Int(n) if n > 0 => {}
        other => panic!("_process never ran (ticks = {other:?}); the rest is vacuous"),
    }

    // The filter ran. This is the assertion that fails on the old code, where
    // `add_visibility_filter` had nowhere to store anything.
    match called_for {
        Variant::Int(p) if p >= 0 => {}
        other => panic!(
            "the filter Callable was never invoked (called_for = {other:?}); \
             it was registered but never run"
        ),
    }

    assert_eq!(
        vis,
        Variant::Bool(false),
        "a filter returning false must hide that peer — the verdict is written \
         into the per-peer visibility map that poll_outbound already gates on"
    );
}

/// The control: same machinery, filter returns `true`, and the peer stays
/// visible. Without this, `false` above could equally mean "any synchronizer
/// carrying a filter is hidden from everyone".
#[test]
fn a_filter_returning_true_leaves_the_peer_visible() {
    let (ticks, called_for, vis) = run(ALLOW_SRC);

    match ticks {
        Variant::Int(n) if n > 0 => {}
        other => panic!("_process never ran (ticks = {other:?}); the rest is vacuous"),
    }
    match called_for {
        Variant::Int(p) if p >= 0 => {}
        other => panic!("the filter Callable was never invoked (called_for = {other:?})"),
    }

    assert_eq!(
        vis,
        Variant::Bool(true),
        "a filter returning true must leave the peer visible"
    );
}

/// ⚠️ A **passing** filter must not wipe out an explicit `set_visibility_for`.
///
/// Godot evaluates filters at query time inside `is_visible_to` and never
/// touches `peer_visibility` (multiplayer_synchronizer.cpp:193-207), so the two
/// AND together. An implementation that stores the filter verdict instead
/// overwrites the explicit call on the very next tick — the entity the project
/// asked to hide quietly comes back.
///
/// The peer used is the one the filter reports being called with, so this does
/// not depend on what `replication_targets()` yields; it is guaranteed to be a
/// peer the outbound pass actually evaluates, which is precisely the peer a
/// storing implementation would clobber.
const EXPLICIT_SURVIVES_SRC: &str = "\
extends Node
var ticks = 0
var called_for = -1
var vis = true
func _process(_delta):
    self.ticks = self.ticks + 1
    add_visibility_filter(Callable(self, \"_allow\"))
    set_visibility_for(self.called_for, false)
    self.vis = is_visible_to(self.called_for)
func _allow(peer):
    self.called_for = peer
    return true
";

#[test]
fn a_passing_filter_does_not_overwrite_an_explicit_set_visibility_for() {
    let (ticks, called_for, vis) = run(EXPLICIT_SURVIVES_SRC);

    match ticks {
        Variant::Int(n) if n > 0 => {}
        other => panic!("_process never ran (ticks = {other:?}); the rest is vacuous"),
    }
    match called_for {
        Variant::Int(p) if p >= 0 => {}
        other => panic!("the filter was never invoked (called_for = {other:?})"),
    }

    assert_eq!(
        vis,
        Variant::Bool(false),
        "the explicit set_visibility_for(peer, false) must survive a filter that \
         returns true. `true` here means the filter verdict was written into the \
         per-peer visibility map and clobbered it"
    );
}

/// ⚠️ Godot fails **closed**: `ERR_FAIL_COND_V(err != CALL_OK || ret is not
/// BOOL, false)`. A filter naming a method that does not exist returns no bool
/// at all, and the peer must be hidden rather than waved through.
///
/// This is the opposite of what a "be forgiving" instinct suggests, which is
/// why it is pinned: a broken filter silently granting visibility is a
/// data-leak-shaped bug, not a resilience feature.
const BROKEN_FILTER_SRC: &str = "\
extends Node
var ticks = 0
var called_for = 0
var vis = true
func _process(_delta):
    self.ticks = self.ticks + 1
    add_visibility_filter(Callable(self, \"_no_such_method\"))
    self.vis = is_visible_to(1)
";

#[test]
fn a_filter_that_returns_no_bool_hides_the_peer() {
    let (ticks, _called_for, vis) = run(BROKEN_FILTER_SRC);

    match ticks {
        Variant::Int(n) if n > 0 => {}
        other => panic!("_process never ran (ticks = {other:?}); the rest is vacuous"),
    }

    assert_eq!(
        vis,
        Variant::Bool(false),
        "a filter that cannot produce a bool must hide the peer, matching \
         Godot's ERR_FAIL_COND_V(..., false)"
    );
}
