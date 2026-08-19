//! pat-s3rrv.17 — `remove_visibility_filter`, and `add` deduping like Godot's set.
//!
//! `add_visibility_filter` had no counterpart at either layer: no script arm and
//! no tree API. A script could install a filter and never take it back — the
//! only way to stop filtering was to free the synchronizer.
//!
//! ⚠️ Godot stores these in a `HashSet<Callable>`
//! (`multiplayer_synchronizer.h:59`); `add` is an `insert` (`:211`) and `remove`
//! an `erase` (`:216`). Ours pushed into a `Vec`, so the same Callable added
//! twice ran twice on every outbound pass — invisible for a pure predicate, and
//! wrong the moment one is expensive or has a side effect. Worse for removal:
//! one `remove` would have left a copy behind and the filtering would continue,
//! which reads as "remove is broken" rather than "add double-stored".
//!
//! Equality is `CallableRef`'s own impl (`gdvariant/src/variant.rs:127`),
//! comparing a `Method` by `target_id` + method name — so the
//! `Callable(self, "_deny")` written at the remove site matches the separate
//! value written at the add site. The counting test below is what proves that,
//! rather than assuming it.

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
            Variant::Array(vec![Variant::String("position".to_string())]),
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

/// ⚠️ Filters are registered from `_process`, NOT `_ready`.
///
/// `synchronizer_add_visibility_filter` returns false and stores nothing while
/// the node is not yet in the replication registry (`scene_tree.rs:2920`), and
/// the tree enters it there on its own poll — so a `_ready` registration is a
/// silent no-op. The sibling test `visibility_filter_callables_run.rs` says so
/// in its own comment; I registered in `_ready` anyway on the first attempt and
/// the filter never ran. Registering across the first few ticks is what makes
/// at least one call land after registration.
///
/// The dedupe probe: this adds the SAME Callable three times per tick, and
/// [`ADD_ONCE`] adds it once. Under Godot's set semantics both store one filter
/// and invoke it the same number of times; under the old `Vec::push` this one
/// runs it three times as often. Comparing the two counts is the discriminator
/// — an absolute number would depend on how many replication passes happen to
/// fit in the tick budget.
const ADD_THRICE: &str = "\
extends Node
var ticks = 0
var calls = 0
func _process(_delta):
    self.ticks = self.ticks + 1
    if self.ticks <= 5:
        add_visibility_filter(Callable(self, \"_deny\"))
        add_visibility_filter(Callable(self, \"_deny\"))
        add_visibility_filter(Callable(self, \"_deny\"))
func _deny(peer):
    self.calls = self.calls + 1
    return false
";

/// The control: the same script adding the filter once.
const ADD_ONCE: &str = "\
extends Node
var ticks = 0
var calls = 0
func _process(_delta):
    self.ticks = self.ticks + 1
    if self.ticks <= 5:
        add_visibility_filter(Callable(self, \"_deny\"))
func _deny(peer):
    self.calls = self.calls + 1
    return false
";

fn calls_after(src: &str, ticks: usize) -> i64 {
    let (mut ml, sync) = scripted_sync_loop(src);
    for _ in 0..ticks {
        ml.step(1.0 / 60.0);
    }
    match script_var(&ml, sync, "calls") {
        Variant::Int(n) => n,
        other => panic!("calls should be an int, got {other:?}"),
    }
}

/// 🔑 Adding the same Callable repeatedly stores ONE filter, as Godot's
/// `HashSet` does.
#[test]
fn adding_the_same_filter_repeatedly_stores_it_once() {
    let once = calls_after(ADD_ONCE, 14);
    let thrice = calls_after(ADD_THRICE, 14);

    assert!(
        once > 0,
        "the control must actually invoke the filter, or this comparison is \
         between two zeroes and proves nothing",
    );
    assert_eq!(
        thrice, once,
        "three adds of the same Callable must invoke it as often as one add — \
         a larger count means `Vec::push` stored duplicates, and one \
         `remove_visibility_filter` would then leave copies behind",
    );
}

/// 🔑 A removed filter stops being invoked.
///
/// ⚠️ `calls_at_remove` is sampled three ticks AFTER the remove, not on the same
/// tick. The outbound replication pass and `_process` both run within a tick and
/// this test should not depend on their order; sampling later makes the
/// assertion true for either ordering.
const ADD_THEN_REMOVE: &str = "\
extends Node
var ticks = 0
var calls = 0
var calls_at_remove = -1
func _process(_delta):
    self.ticks = self.ticks + 1
    if self.ticks <= 5:
        add_visibility_filter(Callable(self, \"_deny\"))
    if self.ticks == 8:
        remove_visibility_filter(Callable(self, \"_deny\"))
    if self.ticks == 11:
        self.calls_at_remove = self.calls
func _deny(peer):
    self.calls = self.calls + 1
    return false
";

#[test]
fn the_filter_stops_being_invoked_after_removal() {
    let (mut ml, sync) = scripted_sync_loop(ADD_THEN_REMOVE);
    for _ in 0..20 {
        ml.step(1.0 / 60.0);
    }

    let at_remove = match script_var(&ml, sync, "calls_at_remove") {
        Variant::Int(n) => n,
        other => panic!("calls_at_remove should be an int, got {other:?}"),
    };
    assert!(
        at_remove > 0,
        "the filter must have been invoked before the remove; 0 means \
         `add_visibility_filter` never stored it and this test proves nothing",
    );
    assert_eq!(
        script_var(&ml, sync, "calls"),
        Variant::Int(at_remove),
        "no further invocations after the remove — a growing count means the \
         Callable written at the remove site did not compare equal to the one \
         written at the add site",
    );
}

/// Removing a filter that was never added is silent, matching `HashSet::erase`.
const REMOVE_UNKNOWN: &str = "\
extends Node
var ticks = 0
var survived = false
func _process(_delta):
    self.ticks = self.ticks + 1
    remove_visibility_filter(Callable(self, \"_never_added\"))
    self.survived = true
";

#[test]
fn removing_a_filter_that_was_never_added_is_silent() {
    let (mut ml, sync) = scripted_sync_loop(REMOVE_UNKNOWN);
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        script_var(&ml, sync, "survived"),
        Variant::Bool(true),
        "an unknown filter must not raise — Godot's HashSet::erase is silent, \
         and a raise here would kill the calling script",
    );
}
