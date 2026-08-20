//! pat-s3rrv.218 — WebXR is driven by a platform backend and pumped from the
//! frame loop, not by a script calling a non-Godot method.
//!
//! `gdplatform/src/xr.rs` has held a complete WebXR session state machine since
//! pat-nzgdu.150, but its only input was a `WebXrStub` installed through
//! `xr._set_test_stub(..)` — a **script-visible method Godot does not have**,
//! whose own comment admitted "there is no real `navigator.xr` here". And the
//! signal drain ran only at the end of a script's own call into the interface
//! (`interpreter.rs`, after each `call_xr_interface_instance`), so an event that
//! arrived *between* script calls could never reach a connected Callable. A
//! headset session was impossible in both directions.
//!
//! This file pins the two halves that fix it:
//!
//! * the backend is installed at the **platform** level, so
//!   `XRServer.find_interface("WebXR")` hands back a wired interface with no
//!   script-side setup at all; and
//! * `selectstart`/`session_ended` reach the script **only** by advancing
//!   `MainLoop`, which is what `pump_xr_signals` on the frame tick buys.
//!
//! ⚠️ The load-bearing assertion is the *negative* one: `selects == 0` right
//! after `_ready()`. Without it this file would still pass if the drain had
//! stayed synchronous, which is precisely the bug being fixed.

use gdscene::node::NodeId;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Reads a script variable off the attached script.
fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

fn count(tree: &SceneTree, node: NodeId, name: &str) -> i64 {
    match script_var(tree, node, name) {
        Variant::Int(v) => v,
        other => panic!("`{name}` should be an int, got {other:?}"),
    }
}

/// The scene's script: connects the three signals and starts a session.
const SCRIPT: &str = "\
var started = 0
var selects = 0
var ended = 0
var ready_done = false
func _on_started():
    started += 1
func _on_select(id):
    selects += 1
func _on_ended():
    ended += 1
func _ready():
    var xr = XRServer.find_interface(\"WebXR\")
    xr.connect(\"session_started\", _on_started)
    xr.connect(\"selectstart\", _on_select)
    xr.connect(\"session_ended\", _on_ended)
    xr.set_session_mode(\"immersive-vr\")
    xr.set_required_features([\"local-floor\"])
    xr.initialize()
    self.ready_done = true
";

/// Installs a backend whose "browser" grants immersive-vr and then, on
/// successive frames, connects a controller, fires `selectstart`, and ends the
/// session — exactly the sequence `navigator.xr` would deliver.
fn install_session_backend() {
    gdplatform::set_default_backend(gdplatform::WebXrBackend {
        stub: gdplatform::WebXrStub {
            supported_modes: vec![gdplatform::WebXrSessionMode::ImmersiveVr],
            available_features: vec!["local-floor".to_string()],
            force_failure: None,
        },
        frame_events: vec![
            vec![gdplatform::WebXrFrameEvent::InputSourceConnected {
                input_source_id: 0,
                target_ray_mode: gdplatform::TargetRayMode::TrackedPointer,
            }],
            vec![gdplatform::WebXrFrameEvent::Input {
                event: gdplatform::WebXrInputEvent::SelectStart,
                input_source_id: 0,
            }],
            vec![gdplatform::WebXrFrameEvent::SessionEnded],
        ],
    });
}

fn scene() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let script = GDScriptNodeInstance::from_source(SCRIPT, root).expect("script parses");
    tree.attach_script(root, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(root);
    (ml, root)
}

#[test]
fn a_session_starts_from_the_platform_backend_with_no_script_setup() {
    gdplatform::clear_default_backend();
    install_session_backend();
    let (ml, root) = scene();

    assert_eq!(
        script_var(ml.tree(), root, "ready_done"),
        Variant::Bool(true),
        "`_ready` must run to completion — an earlier raise is what a missing \
         method looks like from here, and `_set_test_stub` is now gone"
    );
    assert_eq!(
        count(ml.tree(), root, "started"),
        1,
        "the session must start from the platform backend alone; the script \
         never installs a stub"
    );
}

#[test]
fn browser_events_reach_the_script_only_through_the_frame_tick() {
    gdplatform::clear_default_backend();
    install_session_backend();
    let (mut ml, root) = scene();

    // ⚠️ The control that gives this file its meaning. Before this bead the
    // drain ran at the end of the script's own interface call, so anything
    // synchronous would already have arrived here.
    assert_eq!(
        count(ml.tree(), root, "selects"),
        0,
        "no browser event may arrive before the frame loop has run"
    );
    assert_eq!(
        count(ml.tree(), root, "ended"),
        0,
        "the session must not have ended before the frame loop has run"
    );

    for _ in 0..6 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        count(ml.tree(), root, "selects"),
        1,
        "`selectstart` must reach the connected Callable purely by advancing \
         MainLoop, with no script-side call in between"
    );
    assert_eq!(
        count(ml.tree(), root, "ended"),
        1,
        "the browser's session `end` must reach `session_ended` the same way"
    );
}

#[test]
fn without_a_backend_there_is_no_session() {
    // The negative control for the backend seam: with nothing installed the
    // interface must behave like a browser with no WebXR, not like the last
    // test's browser. This is also what a native build must do.
    gdplatform::clear_default_backend();
    let (mut ml, root) = scene();
    for _ in 0..6 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        count(ml.tree(), root, "started"),
        0,
        "with no platform backend `initialize()` must not start a session"
    );
    assert_eq!(
        count(ml.tree(), root, "selects"),
        0,
        "and no browser events can arrive"
    );
}

#[test]
fn the_test_stub_seam_is_gone_from_the_script_table() {
    // The acceptance's other clause. `_set_test_stub` was never a Godot method;
    // a script must now get the same error it would get for any other name.
    gdplatform::clear_default_backend();
    let src = "\
func check():
    var xr = XRServer.find_interface(\"WebXR\")
    xr._set_test_stub([\"immersive-vr\"], [\"local-floor\"], \"\")
    return true
";
    let mut interp = gdscript_interop::interpreter::Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    let err = interp
        .call_instance_method(&mut inst, "check", &[])
        .expect_err("`_set_test_stub` must no longer resolve");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("_set_test_stub"),
        "the error must name the missing method, got {msg}"
    );
}
