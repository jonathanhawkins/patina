//! pat-nzgdu.6 — GDScript `await` resumption is driven by the SceneTree/MainLoop
//! tick, and `var v = await sig` binds the emitted argument.
//!
//! Before this bead the coroutine suspension machinery
//! (`poll_coroutines`/`notify_await_signal`/`tick_coroutine_frames`) existed and
//! was unit-tested but had no runtime driver: `await` never resumed under
//! `MainLoop`, and the resumed signal value was dropped. This app-level test
//! proves both halves of the fix through the real tick:
//!   1. `await get_tree().create_timer(0.1).timeout` resumes after enough frames
//!      elapse for the SceneTreeTimer to fire (`done == true`), driven by
//!      `MainLoop::step` -> `SceneTree::process_script_coroutines`, NOT a direct
//!      interpreter poll.
//!   2. `var v = await my_signal` suspends until `emit_signal` fires and binds
//!      the emitted argument as the awaited value (`got == 42`).

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

#[test]
fn await_resumes_via_tick_and_binds_signal_arg() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = Node::new("Main", "Node");
    let main_id = tree.add_child(root, main).unwrap();

    // `_ready` awaits a SceneTree timer (resumes on the tick), sets `done`, then
    // awaits `my_signal`, binding its emitted argument into `got`.
    // (`elif`/`+=` avoided for parser-independence, matching sibling tests.)
    let script_src = "\
extends Node
signal my_signal
var done = false
var got = -1
func _ready():
    var t = get_tree().create_timer(0.1)
    await t.timeout
    self.done = true
    var v = await my_signal
    self.got = v
";
    let script = GDScriptNodeInstance::from_source(script_src, main_id).unwrap();
    tree.attach_script(main_id, Box::new(script));

    let mut ml = MainLoop::new(tree);

    // Fire `_ready`: runs up to `await t.timeout`, then suspends. `done` is still
    // false because the post-await statement has not executed yet.
    ml.tree_mut().process_script_ready(main_id);
    {
        let script = ml.tree().get_script(main_id).expect("script attached");
        assert_eq!(
            script.get_property("done"),
            Some(Variant::Bool(false)),
            "await must suspend _ready before `done = true` runs",
        );
    }

    // Step frames: each `MainLoop::step` advances the SceneTreeTimer by delta and
    // drives `process_script_coroutines`. ~7 frames at 1/60s reach the 0.1s timer;
    // 20 frames give margin. After the timer fires the coroutine resumes on the
    // tick, sets `done = true`, and re-suspends on `await my_signal`.
    for _ in 0..20 {
        ml.step(1.0 / 60.0);
    }
    {
        let script = ml.tree().get_script(main_id).expect("script attached");
        assert_eq!(
            script.get_property("done"),
            Some(Variant::Bool(true)),
            "timer `await` must resume via the MainLoop tick (done == true)",
        );
        // Still parked on `await my_signal`; `got` unchanged until the signal fires.
        assert_eq!(
            script.get_property("got"),
            Some(Variant::Int(-1)),
            "second await must remain suspended until `my_signal` emits",
        );
    }

    // Emit the awaited signal with an argument. This resumes the parked coroutine
    // and binds the emitted value (42) as the result of `await my_signal`.
    ml.tree_mut()
        .emit_signal(main_id, "my_signal", &[Variant::Int(42)]);

    let script = ml.tree().get_script(main_id).expect("script attached");
    assert_eq!(
        script.get_property("got"),
        Some(Variant::Int(42)),
        "`var v = await my_signal` must bind the emitted argument (42)",
    );
}
