//! pat-mgixk.58 acceptance: `process_mode`, `process_priority` and
//! `process_physics_priority` authored in a `.tscn` (or assigned from a script)
//! are real `Node` state that the running frame loop obeys.
//!
//! Godot ground truth: `scene/main/node.cpp` — `process_mode` is a property
//! backed by `set_process_mode` (PROCESS_MODE_INHERIT 0, PAUSABLE 1,
//! WHEN_PAUSED 2, ALWAYS 3, DISABLED 4), and `process_priority` /
//! `process_physics_priority` order the process groups.
//!
//! # What was broken
//!
//! `Node::set_property` stored every key in the property bag and intercepted
//! only `metadata/`. The field `should_process_node` / `effective_process_mode`
//! actually read is written **only** by `Node::set_process_mode`, whose sole
//! non-test callers are `behavioral_2d.rs` and `visibility_3d.rs`
//! (`VisibleOnScreenEnabler`). Nothing in `packed_scene.rs` or `scripting.rs`
//! forwarded the three keys. So:
//!
//! * a pause menu authored with `process_mode = 3` froze the moment the tree
//!   paused — the single most common use of the property;
//! * `process_mode = PROCESS_MODE_DISABLED` from a script did nothing;
//! * `process_priority = -5` in a `.tscn` reordered nothing.
//!
//! The one existing `.tscn` test for these keys
//! (`tests/scene_suite/scene_instancing_edge_cases_test.rs`) asserts on the
//! property **bag**, which was always populated — so it passed throughout.
//!
//! 🔑 Every clause below is asserted against *dispatch*, never against the bag.
//! Reading `get_property("process_mode")` back as `3` is exactly what already
//! worked, so a test built on that would be green before and after the change.
//! The two places the fix is observable are `should_process_node` during
//! `MainLoop::step` and the order `_process` is dispatched in.

use gdscene::node::{NodeId, ProcessMode};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;
const UNPAUSED_FRAMES: usize = 3;
const PAUSED_FRAMES: usize = 3;

/// `Menu` is ALWAYS, `Normal` takes the default; `Late` and `Early` report into
/// `Log` so the dispatch *order* is observable, and `Early` is authored LATER in
/// tree order with the LOWER priority so tree order and priority order
/// disagree.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]

[node name="Menu" type="Node2D" parent="."]
process_mode = 3

[node name="Normal" type="Node2D" parent="."]

[node name="SelfDisable" type="Node2D" parent="."]

[node name="Log" type="Node2D" parent="."]

[node name="Late" type="Node2D" parent="."]

[node name="Early" type="Node2D" parent="."]
process_priority = -5
"#;

const COUNTER: &str = "\
extends Node2D
var ticks = 0
func _process(delta):
    self.ticks = self.ticks + 1
";

/// Turns ITSELF off mid-run by assigning the property — the script half of the
/// bead. `Node.PROCESS_MODE_DISABLED` is the qualified spelling because Patina
/// resolves a class constant only through `Type.MEMBER`
/// (`interpreter.rs` falls back to `class_get_integer_constant` there); a bare
/// `PROCESS_MODE_DISABLED` is an @GlobalScope lookup and Node's constants are
/// not in that table. The *value* is what this clause is about either way.
const SELF_DISABLE: &str = "\
extends Node2D
var ticks = 0
func _process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 2:
        process_mode = Node.PROCESS_MODE_DISABLED
";

const LOG: &str = "\
extends Node2D
var seq = \"\"
func note(who):
    self.seq = self.seq + who
";

const EARLY: &str = "\
extends Node2D
func _process(delta):
    get_node(\"../Log\").note(\"E\")
";

const LATE: &str = "\
extends Node2D
func _process(delta):
    get_node(\"../Log\").note(\"L\")
";

struct Boot {
    ml: MainLoop,
    menu: NodeId,
    normal: NodeId,
    self_disable: NodeId,
    log: NodeId,
    early: NodeId,
    late: NodeId,
}

fn boot() -> Boot {
    gdobject::class_db::register_all_classes();

    let packed = PackedScene::from_tscn(SCENE).expect("fixture scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &packed).expect("scene instances");

    let find = |name: &str| -> NodeId {
        tree.get_node_or_null(main, name)
            .unwrap_or_else(|| panic!("the fixture must contain a `{name}` node"))
    };
    let menu = find("Menu");
    let normal = find("Normal");
    let self_disable = find("SelfDisable");
    let log = find("Log");
    let early = find("Early");
    let late = find("Late");

    for (id, src) in [
        (menu, COUNTER),
        (normal, COUNTER),
        (self_disable, SELF_DISABLE),
        (log, LOG),
        (early, EARLY),
        (late, LATE),
    ] {
        tree.attach_script(
            id,
            Box::new(GDScriptNodeInstance::from_source(src, id).expect("script parses")),
        );
    }

    let ml = MainLoop::new(tree);
    Boot {
        ml,
        menu,
        normal,
        self_disable,
        log,
        early,
        late,
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

fn ticks(ml: &MainLoop, id: NodeId) -> i64 {
    match script_var(ml, id, "ticks") {
        Variant::Int(n) => n,
        other => panic!("`ticks` should be an Int, got {other:?}"),
    }
}

/// **The acceptance test.** A `.tscn` `process_mode = 3` survives pause, the
/// default sibling does not, a script can disable itself through the property,
/// and a `.tscn` `process_priority` reorders `_process` dispatch.
#[test]
fn tscn_process_mode_always_runs_under_pause() {
    let mut b = boot();

    // -- the authored values reached the TYPED state, not just the bag --------
    //
    // `effective_process_mode` is the function the frame loop consults; the bag
    // row was always written, so asserting on `get_property` here would prove
    // nothing. This is the precondition that makes the pause clause below
    // meaningful rather than vacuous.
    assert_eq!(
        b.ml.tree().effective_process_mode(b.menu),
        ProcessMode::Always,
        "`process_mode = 3` in the .tscn must reach the field \
         `effective_process_mode` reads — the bag row alone is what already \
         worked and is invisible to the frame loop"
    );
    assert_eq!(
        b.ml.tree().effective_process_mode(b.normal),
        ProcessMode::Pausable,
        "a node that authored nothing must still resolve to PAUSABLE, so the \
         two siblings differ only in the property under test"
    );
    assert_eq!(
        b.ml.tree()
            .get_node(b.early)
            .expect("Early is in the tree")
            .process_priority(),
        -5,
        "`process_priority = -5` in the .tscn must reach the typed field that \
         `all_nodes_in_process_order` sorts by"
    );
    // CONTROL for the ordering clause: `Late` is at the DEFAULT priority, so
    // the two differ only in the authored value. If a future fixture change
    // gave Late a priority too, "EL" would stop proving anything about -5.
    assert_eq!(
        b.ml.tree()
            .get_node(b.late)
            .expect("Late is in the tree")
            .process_priority(),
        0,
        "precondition: Late must keep the default priority"
    );

    // -- phase A: unpaused ---------------------------------------------------
    for _ in 0..UNPAUSED_FRAMES {
        b.ml.step(DT);
    }

    // CONTROL: both counters ran while unpaused. Without this, "Normal froze
    // under pause" cannot be told apart from "Normal never ran at all".
    assert_eq!(
        ticks(&b.ml, b.normal),
        UNPAUSED_FRAMES as i64,
        "precondition: the default-mode node must tick once per unpaused frame"
    );
    assert_eq!(
        ticks(&b.ml, b.menu),
        UNPAUSED_FRAMES as i64,
        "precondition: the ALWAYS node ticks at the same rate while unpaused"
    );

    // -- (2) a script disabled ITSELF through the property --------------------
    //
    // It stopped at 2 even though 3 unpaused frames ran, so the assignment took
    // effect on the frame it happened. At HEAD the write lands in the bag, the
    // field stays INHERIT, and this reads 3.
    assert_eq!(
        ticks(&b.ml, b.self_disable),
        2,
        "assigning `process_mode = Node.PROCESS_MODE_DISABLED` from `_process` \
         must stop this node's own `_process` from the next frame on; {} \
         unpaused frames ran",
        UNPAUSED_FRAMES
    );

    // -- (3) `.tscn` priority reorders dispatch ------------------------------
    //
    // 🔑 `Late` is EARLIER in tree order and `Early` has priority -5, so:
    //   * priority ordering → "EL" each frame → "ELELEL"
    //   * unordered / tree order → "LELELE"
    // Same length, same characters; only the sequence separates them.
    let seq = match script_var(&b.ml, b.log, "seq") {
        Variant::String(s) => s,
        other => panic!("`Log.seq` should be a String, got {other:?}"),
    };
    assert_eq!(
        seq, "ELELEL",
        "`_process` must dispatch in `process_priority` order: Early (-5) \
         before Late (0), every frame, even though Late is earlier in tree \
         order. \"LELELE\" is the bug — the authored priority never reached the \
         field `all_nodes_in_process_order` sorts by"
    );

    // -- (1) pause: ALWAYS keeps running, the default one stops ---------------
    b.ml.tree_mut().set_paused(true);
    for _ in 0..PAUSED_FRAMES {
        b.ml.step(DT);
    }

    assert_eq!(
        ticks(&b.ml, b.normal),
        UNPAUSED_FRAMES as i64,
        "a default-`process_mode` node must receive no `_process` while the \
         tree is paused"
    );
    assert_eq!(
        ticks(&b.ml, b.menu),
        (UNPAUSED_FRAMES + PAUSED_FRAMES) as i64,
        "the `.tscn`-authored `process_mode = 3` (ALWAYS) node must keep \
         counting across the pause — this is the pause-menu case, and at HEAD \
         it freezes with the rest of the scene"
    );
}

/// The remaining two `ProcessMode` values and the physics priority key, which
/// the acceptance test does not reach. Separate so a failure here names the
/// value rather than looking like a pause regression.
#[test]
fn tscn_process_mode_when_paused_and_disabled_and_physics_priority() {
    gdobject::class_db::register_all_classes();

    // WHEN_PAUSED (2) is the inverse of ALWAYS: silent while running, live
    // while paused. DISABLED (4) is silent in both states. Asserting both
    // directions is what rules out "the field is set but the mapping is off by
    // one" — an off-by-one would turn ALWAYS into WHEN_PAUSED and the
    // acceptance test's pause clause would still read plausibly.
    let scene = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]

[node name="OnlyPaused" type="Node2D" parent="."]
process_mode = 2

[node name="Never" type="Node2D" parent="."]
process_mode = 4

[node name="PhysicsEarly" type="Node2D" parent="."]
process_physics_priority = -3
"#;
    let packed = PackedScene::from_tscn(scene).expect("fixture parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &packed).expect("scene instances");
    let only_paused = tree
        .get_node_or_null(main, "OnlyPaused")
        .expect("OnlyPaused exists");
    let never = tree.get_node_or_null(main, "Never").expect("Never exists");
    let physics_early = tree
        .get_node_or_null(main, "PhysicsEarly")
        .expect("PhysicsEarly exists");

    assert_eq!(
        tree.effective_process_mode(only_paused),
        ProcessMode::WhenPaused,
        "`process_mode = 2` must map to WHEN_PAUSED"
    );
    assert_eq!(
        tree.effective_process_mode(never),
        ProcessMode::Disabled,
        "`process_mode = 4` must map to DISABLED"
    );
    assert_eq!(
        tree.get_node(physics_early)
            .expect("PhysicsEarly is in the tree")
            .process_physics_priority(),
        -3,
        "`process_physics_priority` must reach its OWN field, not the idle one"
    );
    // ...and it did not spill into the idle priority, which is the mistake a
    // shared intercept would make.
    assert_eq!(
        tree.get_node(physics_early)
            .expect("PhysicsEarly is in the tree")
            .process_priority(),
        0,
        "`process_physics_priority` must leave `process_priority` at its default"
    );

    // The dispatch consequence, both pause states, in one tree.
    assert!(
        !tree.should_process_node(only_paused),
        "a WHEN_PAUSED node must not process while the tree runs normally"
    );
    assert!(
        !tree.should_process_node(never),
        "a DISABLED node must not process while the tree runs normally"
    );
    tree.set_paused(true);
    assert!(
        tree.should_process_node(only_paused),
        "a WHEN_PAUSED node must process once the tree is paused"
    );
    assert!(
        !tree.should_process_node(never),
        "a DISABLED node must stay silent even while paused"
    );
}
