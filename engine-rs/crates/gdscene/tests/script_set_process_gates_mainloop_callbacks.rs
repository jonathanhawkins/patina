//! pat-mgixk.57 acceptance: the runtime frame loop honours the per-node process
//! toggles and `process_physics_priority`, and GDScript can reach the whole of
//! `Node`'s process surface.
//!
//! Godot ground truth: `Node.set_process`/`set_physics_process` remove the node
//! from the matching process group (`scene/main/node.cpp`), and
//! `SceneTree::_process_group` orders the physics group by
//! `process_physics_priority` — a property deliberately independent of
//! `process_priority`, so a node can run late in `_process` and early in
//! `_physics_process`.
//!
//! # What was broken, and what this test can see
//!
//! The toggle filters landed in pat-mgixk.6 and are already honoured by
//! `MainLoop::step`, so clauses (1) and (3) below are **regression cover**, not
//! the discriminator. Two gaps remained, and they are what this test fails on
//! without the change:
//!
//! * **Physics priority was inert at runtime.**
//!   `all_nodes_in_physics_process_order` has existed since pat-0q2kx.221, but
//!   its only caller — `process_all_scripts_physics_process` — has no
//!   production caller. `MainLoop::step` drives
//!   `process_physics_frame_with_scripts`, which sorted by `process_priority`,
//!   the *idle* key. So the two orderings could never differ, which is the
//!   entire reason Godot has two properties.
//! * **`set_physics_process_priority` was unreachable from GDScript.** It had
//!   no arm in `call_node_method`, in `core_node_methods`, or in the
//!   interpreter, so the call raised `UndefinedFunction`.
//!
//! 🔑 The ordering assertion is a **sequence**, not a pair of per-node flags: a
//! test that only checked "both handlers ran" cannot tell "Second ran first"
//! from "First ran first", and a test relating the two nodes' own counters to
//! each other would read the same under either order. Each node reports into a
//! shared `Log` script, exactly as `input_propagation_order_parity` does for the
//! input stages.
//!
//! ⚠️ The priorities are set **from the script**, in `_ready`, rather than
//! poked onto the `Node` from Rust. That is deliberate: a Rust-side
//! `set_node_physics_process_priority` would leave the new GDScript arm
//! untested, and the pair together is what a game actually does.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;
/// More than one frame, so "ran once" cannot be mistaken for "ran every frame",
/// and the order log repeats rather than showing a single lucky pair.
const FRAMES: usize = 4;

/// `Main -> {Muted, Ticker, Log, First, Second}`. Flat on purpose: the physics
/// ordering under test must come from the priorities, and with every node a
/// sibling at the same depth tree order is the only competing explanation.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]

[node name="Muted" type="Node2D" parent="."]

[node name="Ticker" type="Node2D" parent="."]

[node name="Log" type="Node2D" parent="."]

[node name="First" type="Node2D" parent="."]

[node name="Second" type="Node2D" parent="."]
"#;

/// Opts out of `_process` and keeps `_physics_process`, then reports what
/// `is_processing()` says about itself.
const MUTED: &str = "\
extends Node2D
var process_calls = 0
var physics_calls = 0
var ready_ran = false
var still_processing = true
func _ready():
    self.ready_ran = true
    set_process(false)
    self.still_processing = is_processing()
func _process(delta):
    self.process_calls = self.process_calls + 1
func _physics_process(delta):
    self.physics_calls = self.physics_calls + 1
";

/// The CONTROL. Identical counters, no toggle. Without it, "Muted saw 0
/// `_process` calls" is indistinguishable from "this harness never dispatched
/// `_process` at all" — which is how a fixture that silently failed to boot
/// would read as a passing gate.
const TICKER: &str = "\
extends Node2D
var process_calls = 0
var physics_calls = 0
func _process(delta):
    self.process_calls = self.process_calls + 1
func _physics_process(delta):
    self.physics_calls = self.physics_calls + 1
";

const LOG: &str = "\
extends Node2D
var seq = \"\"
func note(who):
    self.seq = self.seq + who
";

/// Earlier in tree order, LATER in physics priority — so tree order and
/// priority order disagree, and the log can only come out one way per fix.
const FIRST: &str = "\
extends Node2D
var priority_readback = 99
func _ready():
    set_physics_process_priority(1)
    self.priority_readback = get_physics_process_priority()
func _physics_process(delta):
    get_node(\"../Log\").note(\"F\")
";

const SECOND: &str = "\
extends Node2D
var priority_readback = 99
func _ready():
    set_physics_process_priority(-1)
    self.priority_readback = get_physics_process_priority()
func _physics_process(delta):
    get_node(\"../Log\").note(\"S\")
";

struct Boot {
    ml: MainLoop,
    muted: NodeId,
    ticker: NodeId,
    log: NodeId,
    first: NodeId,
    second: NodeId,
}

/// Boots the fixture the way `patina-runner` does: parse the `.tscn`, instance
/// it into the tree, attach the scripts, fire `_ready`, then drive frames
/// through `MainLoop::step`.
fn boot() -> Boot {
    // The runner registers the class tables before loading a scene, so the
    // ClassDB-sourced property defaults this fixture sees match a real boot.
    gdobject::class_db::register_all_classes();

    let packed = PackedScene::from_tscn(SCENE).expect("fixture scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &packed).expect("scene instances");

    let find = |name: &str| -> NodeId {
        tree.get_node_or_null(main, name)
            .unwrap_or_else(|| panic!("the fixture must contain a `{name}` node"))
    };
    let muted = find("Muted");
    let ticker = find("Ticker");
    let log = find("Log");
    let first = find("First");
    let second = find("Second");

    for (id, src) in [
        (muted, MUTED),
        (ticker, TICKER),
        (log, LOG),
        (first, FIRST),
        (second, SECOND),
    ] {
        tree.attach_script(
            id,
            Box::new(GDScriptNodeInstance::from_source(src, id).expect("script parses")),
        );
    }

    let mut ml = MainLoop::new(tree);
    // `MainLoop::step` does not fire `_ready` — the host does it after loading a
    // scene, which is what `patina-runner` boot and every other scripted test
    // here do.
    for id in [muted, ticker, log, first, second] {
        ml.tree_mut().process_script_ready(id);
    }

    Boot {
        ml,
        muted,
        ticker,
        log,
        first,
        second,
    }
}

fn script_var(ml: &MainLoop, id: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(id)
        .and_then(|s| {
            s.list_properties()
                .into_iter()
                .find(|p| p.name == name)
                .map(|p| p.default_value)
        })
        .unwrap_or_else(|| panic!("the script on {id:?} must expose `{name}`"))
}

fn int_var(ml: &MainLoop, id: NodeId, name: &str) -> i64 {
    match script_var(ml, id, name) {
        Variant::Int(n) => n,
        other => panic!("`{name}` should be an Int, got {other:?}"),
    }
}

/// **The acceptance test.** `set_process(false)` silences the idle callback
/// across real frames while physics keeps running; `is_processing()` agrees; and
/// `_physics_process` runs in `process_physics_priority` order rather than tree
/// order.
#[test]
fn script_set_process_gates_mainloop_callbacks() {
    let mut b = boot();

    // CONTROL: `_ready` actually ran, so the `set_process(false)` inside it did
    // too. A fixture whose `_ready` never fired would report 0 `_process` calls
    // for the wrong reason entirely.
    assert_eq!(
        script_var(&b.ml, b.muted, "ready_ran"),
        Variant::Bool(true),
        "precondition: `_ready` must have run on the muted node"
    );

    for _ in 0..FRAMES {
        b.ml.step(DT);
    }

    // -- (1) the toggle gates the idle callback, not the physics one ----------
    //
    // CONTROL FIRST: the un-muted node proves this harness dispatches `_process`
    // at all. Without this line, a broken boot and a working gate are the same
    // observation.
    let control_process = int_var(&b.ml, b.ticker, "process_calls");
    assert!(
        control_process > 0,
        "precondition: the un-muted node must receive `_process` — got \
         {control_process} calls in {FRAMES} frames, so the fixture never ran \
         the idle phase and clause (1) below would pass vacuously"
    );
    assert_eq!(
        int_var(&b.ml, b.muted, "process_calls"),
        0,
        "`set_process(false)` in `_ready` must stop this node's `_process` for \
         every subsequent `MainLoop::step`"
    );
    assert_eq!(
        int_var(&b.ml, b.muted, "physics_calls"),
        FRAMES as i64,
        "`set_process(false)` must leave `_physics_process` alone — the two \
         toggles are independent in Godot, and one tick ran per step"
    );

    // -- (3) and the node agrees about itself --------------------------------
    assert_eq!(
        script_var(&b.ml, b.muted, "still_processing"),
        Variant::Bool(false),
        "`is_processing()` must report false right after `set_process(false)`"
    );

    // -- (2) physics runs in PHYSICS priority order, not tree order ----------
    //
    // CONTROL: the priorities the scripts set were actually stored. Without
    // this, a `set_physics_process_priority` that silently did nothing would
    // leave both nodes at 0 and the order assertion below would be testing tree
    // order against itself.
    assert_eq!(
        int_var(&b.ml, b.first, "priority_readback"),
        1,
        "`set_physics_process_priority(1)` must round-trip through \
         `get_physics_process_priority()`"
    );
    assert_eq!(
        int_var(&b.ml, b.second, "priority_readback"),
        -1,
        "`set_physics_process_priority(-1)` must round-trip too"
    );

    // 🔑 THE DISCRIMINATOR. `First` is earlier in tree order and `Second` has
    // the lower physics priority, so:
    //   * physics ordering  → "SF" each frame → "SFSFSFSF"
    //   * idle ordering (both `process_priority` 0, falls back to tree order)
    //     → "FSFSFSFS"
    // The two strings are the same length and the same multiset of characters;
    // only the sequence tells them apart.
    let seq = match script_var(&b.ml, b.log, "seq") {
        Variant::String(s) => s,
        other => panic!("`Log.seq` should be a String, got {other:?}"),
    };
    assert_eq!(
        seq, "SFSFSFSF",
        "`_physics_process` must run in `process_physics_priority` order: \
         Second (-1) before First (+1), every frame, even though First is \
         earlier in tree order. \"FSFSFSFS\" is the bug this bead is about — \
         `MainLoop::step` sorting the physics phase by the IDLE priority"
    );
}

/// The rest of `Node`'s process surface, which had no dispatch arm anywhere:
/// per-stage input toggles, the idle priority, `can_process` and the two delta
/// getters. Separate from the acceptance test so a failure here names the gap
/// rather than looking like an ordering regression.
#[test]
fn script_reaches_the_rest_of_the_process_surface() {
    let src = "\
extends Node2D
var input_before = false
var input_after = true
var unhandled_after = false
var key_after = false
var shortcut_after = false
var priority_readback = 99
var can_run = false
var process_delta = -1.0
var physics_delta = -1.0
func _ready():
    self.input_before = is_processing_input()
    set_process_input(false)
    self.input_after = is_processing_input()
    self.unhandled_after = is_processing_unhandled_input()
    set_process_unhandled_key_input(false)
    self.key_after = is_processing_unhandled_key_input()
    set_process_shortcut_input(false)
    self.shortcut_after = is_processing_shortcut_input()
    set_process_priority(7)
    self.priority_readback = get_process_priority()
    self.can_run = can_process()
func _physics_process(delta):
    self.physics_delta = get_physics_process_delta_time()
func _process(delta):
    self.process_delta = get_process_delta_time()
";
    gdobject::class_db::register_all_classes();
    let packed = PackedScene::from_tscn(SCENE).expect("fixture scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &packed).expect("scene instances");
    let node = tree
        .get_node_or_null(main, "Ticker")
        .expect("the fixture has a Ticker node");
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).expect("script parses")),
    );
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(node);
    ml.step(DT);

    // The toggle moved — reading `true` both sides would mean the setter was a
    // no-op, which is indistinguishable from a missing arm if only the
    // post-state is asserted.
    assert_eq!(
        script_var(&ml, node, "input_before"),
        Variant::Bool(true),
        "a node processes input by default, as in Godot"
    );
    assert_eq!(
        script_var(&ml, node, "input_after"),
        Variant::Bool(false),
        "`set_process_input(false)` must be visible to `is_processing_input()`"
    );
    // 🔑 Per-stage, not one shared flag: `set_process_input(false)` must leave
    // `_unhandled_input` armed. Godot keeps a separate node group per stage, and
    // "handle only what nobody else handled" is the idiom a single flag made
    // impossible.
    assert_eq!(
        script_var(&ml, node, "unhandled_after"),
        Variant::Bool(true),
        "`set_process_input(false)` must NOT disarm `_unhandled_input`"
    );
    assert_eq!(
        script_var(&ml, node, "key_after"),
        Variant::Bool(false),
        "`set_process_unhandled_key_input(false)` must disarm its OWN stage"
    );
    assert_eq!(
        script_var(&ml, node, "shortcut_after"),
        Variant::Bool(false),
        "`set_process_shortcut_input(false)` must disarm its OWN stage"
    );
    assert_eq!(
        int_var(&ml, node, "priority_readback"),
        7,
        "`set_process_priority` must round-trip through `get_process_priority`"
    );
    assert_eq!(
        script_var(&ml, node, "can_run"),
        Variant::Bool(true),
        "`can_process()` is the pause question: true on an unpaused tree, even \
         after `set_process_input(false)`"
    );

    // The deltas are the frame's real timestep, not a placeholder. Asserted
    // against DT rather than merely "> 0" so a hardcoded constant fails.
    for name in ["process_delta", "physics_delta"] {
        let got = match script_var(&ml, node, name) {
            Variant::Float(f) => f,
            other => panic!("`{name}` should be a Float, got {other:?}"),
        };
        assert!(
            (got - DT).abs() < 1e-6,
            "`{name}` must report the frame's timestep {DT}, got {got}"
        );
    }
}
